use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::error::Category;
use serde_json::Value;
use std::time::Duration;

use crate::domain::session::Message;

const MAX_HTTP_ATTEMPTS: u8 = 3;
const RETRY_BASE_DELAY_MS: u64 = 1_000;
const RETRY_DELAY_FACTOR: u64 = 4;

fn is_transient(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_body() || error.is_decode()
}

/// A body that was cut off mid-payload classifies as `Category::Eof`; a payload
/// whose *shape* the deserializer cannot accept classifies as `Data`/`Syntax`.
///
/// Only truncation earns another round trip. A structurally wrong response —
/// wrong field types, an envelope this provider adapter does not model — fails
/// identically on every attempt, so retrying it only multiplies the wait before
/// the same error surfaces.
fn is_truncated(error: &serde_json::Error) -> bool {
    matches!(error.classify(), Category::Eof)
}

fn retry_backoff(attempt: u8) -> Duration {
    let factor = RETRY_DELAY_FACTOR.saturating_pow(u32::from(attempt).saturating_sub(1));
    Duration::from_millis(RETRY_BASE_DELAY_MS.saturating_mul(factor))
}

async fn retry_delay(attempt: u8, reason: &str) {
    let delay = retry_backoff(attempt);
    tracing::warn!(
        "Transient LLM HTTP failure on attempt {}/{}: {}. Retrying in {:?}.",
        attempt,
        MAX_HTTP_ATTEMPTS,
        reason,
        delay,
    );
    tokio::time::sleep(delay).await;
}

/// Request keys Atoma owns outright; `extra_body` may not set them.
const RESERVED_KEYS: [&str; 2] = ["model", "messages"];

/// Reconcile an `extra_body` `tools` value with the runtime tool definitions.
///
/// Returns the value to store, or `None` when a plain insert is correct.
///
/// A plain insert would REPLACE the runtime tools. That silently strips every
/// MCP tool's JSON Schema from the request, and because the system prompt lists
/// only tool *names*, the model is then left to guess argument shapes — observed
/// in production as a stream of wrong-typed and missing arguments.
///
/// OpenRouter's server tools (`{"type": "openrouter:web_search"}`) are declared
/// in this same array and are documented to work alongside user-defined tools,
/// so both sets belong in it: append rather than overwrite.
fn reconcile_tools(runtime: Option<&Value>, extra: &Value) -> Option<Value> {
    // No runtime tools to protect: whatever the agent supplied stands alone.
    let runtime = runtime?.as_array()?;

    match extra.as_array() {
        Some(extra) => {
            let mut merged = runtime.clone();
            merged.extend(extra.iter().cloned());
            Some(Value::Array(merged))
        }
        None => {
            tracing::warn!(
                "extra_body.tools is not an array; ignoring it and keeping the \
                 {} runtime tool definition(s)",
                runtime.len(),
            );
            Some(Value::Array(runtime.clone()))
        }
    }
}

/// Merge an agent's `extra_body` into an assembled request body.
///
/// Reserved keys are dropped; `tools` is merged with what the runtime already
/// put there; everything else overrides.
fn merge_extra_body(
    body: &mut serde_json::Map<String, Value>,
    extra_body: &std::collections::HashMap<String, Value>,
) {
    for (key, value) in extra_body {
        if RESERVED_KEYS.contains(&key.as_str()) {
            continue;
        }
        if key == "tools" {
            if let Some(reconciled) = reconcile_tools(body.get("tools"), value) {
                body.insert(key.clone(), reconciled);
                continue;
            }
        }
        body.insert(key.clone(), value.clone());
    }
}

/// Shared HTTP response types (OpenAI-compatible wire format).
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: Message,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// POST a request and deserialize its JSON body, retrying transport-level
/// failures.
///
/// `build_request` is a closure rather than a prepared `RequestBuilder` because
/// each attempt needs a fresh one.
///
/// Retried: connect/timeout/body/decode errors, HTTP 429, HTTP 5xx, and a
/// truncated response body. Not retried: any other status, a provider error
/// object returned under HTTP 200, and a structurally invalid payload — see
/// [`is_truncated`].
pub(crate) async fn send_json_with_retry<T: DeserializeOwned>(
    label: &str,
    build_request: impl Fn() -> reqwest::RequestBuilder,
) -> Result<T> {
    for attempt in 1..=MAX_HTTP_ATTEMPTS {
        let response = match build_request().send().await {
            Ok(response) => response,
            Err(error) if attempt < MAX_HTTP_ATTEMPTS && is_transient(&error) => {
                retry_delay(attempt, &error.to_string()).await;
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to send {label} request"))
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let retryable =
                status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            if attempt < MAX_HTTP_ATTEMPTS && retryable {
                retry_delay(
                    attempt,
                    &format!("{label} API error ({status}): {error_text}"),
                )
                .await;
                continue;
            }
            anyhow::bail!("{} API error ({}): {}", label, status, error_text);
        }

        let body = match response.text().await {
            Ok(body) => body,
            Err(error) if attempt < MAX_HTTP_ATTEMPTS && is_transient(&error) => {
                retry_delay(attempt, &error.to_string()).await;
                continue;
            }
            Err(error) => return Err(error).context("Failed to read response body"),
        };

        // Some providers (e.g. OpenRouter) return HTTP 200 with an error object.
        // Detect this and provide a clear error message.
        if let Ok(val) = serde_json::from_str::<Value>(&body) {
            if let Some(error_obj) = val.get("error").filter(|e| !e.is_null()) {
                let msg = error_obj
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown provider error");
                let code = error_obj
                    .get("code")
                    .and_then(|c| c.as_i64())
                    .map(|c| format!(" (code: {})", c))
                    .unwrap_or_default();
                anyhow::bail!("LLM provider error{}: {}", code, msg);
            }
        }

        match serde_json::from_str::<T>(&body) {
            Ok(parsed) => return Ok(parsed),
            Err(error) if attempt < MAX_HTTP_ATTEMPTS && is_truncated(&error) => {
                retry_delay(attempt, &format!("truncated response body: {error}")).await;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to parse {label} response. \
                         The model may be unavailable or returning an unexpected format."
                    )
                })
            }
        }
    }

    // Unreachable: on the final attempt every arm above returns or bails.
    anyhow::bail!("{} HTTP retry loop exhausted without a response", label)
}

/// Move a tool result's pictures into a following `user` message.
///
/// The OpenAI schema has no way to return an image from a tool: a `tool`
/// message's content is text. So a result that carries one becomes two
/// messages — the tool result with its text, then a user message holding the
/// pictures. It is the only route by which a model on this API ever sees them.
///
/// The synthetic message is built here and nowhere else. The session keeps the
/// single tool message it recorded, so nothing about this reaches disk, and a
/// run resumed against Anthropic — which can carry an image in a tool result —
/// takes that path instead with no trace of this one.
///
/// Everything without pictures passes through as one message, unchanged.
fn split_images_out_of_tool_message(message: Value) -> Vec<Value> {
    if message.get("role").and_then(Value::as_str) != Some("tool") {
        return vec![message];
    }
    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
        return vec![message];
    };

    let mut text = String::new();
    let mut images = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(t);
                }
            }
            Some("image") => {
                let (Some(data), Some(mime)) = (
                    block.get("data").and_then(Value::as_str),
                    block.get("mimeType").and_then(Value::as_str),
                ) else {
                    continue;
                };
                images.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:{};base64,{}", mime, data) },
                }));
            }
            _ => {}
        }
    }

    if images.is_empty() {
        return vec![message];
    }

    let mut tool_message = message;
    if let Some(obj) = tool_message.as_object_mut() {
        obj.insert("content".to_string(), Value::String(text));
    }
    vec![
        tool_message,
        serde_json::json!({ "role": "user", "content": images }),
    ]
}

/// Shared OpenAI-compatible HTTP call used by OpenAI and Copilot providers.
#[allow(clippy::too_many_arguments)]
pub async fn openai_compat_call(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    extra_headers: &[(String, String)],
    model: &str,
    messages: &[Message],
    tools: Option<&[Value]>,
    extra_body: &std::collections::HashMap<String, Value>,
) -> Result<ChatResponse> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let llm_messages: Vec<Value> = messages
        .iter()
        .flat_map(|m| split_images_out_of_tool_message(m.to_llm_value()))
        .collect();
    let mut body = serde_json::json!({
        "model": model,
        "messages": llm_messages,
    });

    if let Some(tools) = tools {
        body["tools"] = serde_json::json!(tools);
        body["tool_choice"] = serde_json::json!("auto");
    }

    if let Some(obj) = body.as_object_mut() {
        merge_extra_body(obj, extra_body);
    }

    tracing::debug!("Request URL: {}", url);
    tracing::debug!("Request body: {}", serde_json::to_string_pretty(&body)?);

    send_json_with_retry("LLM", || {
        let mut request = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json");

        for (name, value) in extra_headers {
            request = request.header(name.as_str(), value.as_str());
        }

        request.json(&body)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const VALID_BODY: &str = r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":null}"#;

    fn http_200(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        )
    }

    /// Serve one raw HTTP response per element of `responses`, counting requests.
    fn spawn_server(
        responses: Vec<String>,
    ) -> (
        tokio::task::JoinHandle<()>,
        std::net::SocketAddr,
        Arc<AtomicUsize>,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);

        let handle = tokio::spawn(async move {
            let listener = TcpListener::from_std(listener).unwrap();
            for response in responses {
                let (mut stream, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => return,
                };
                let mut request = vec![0; 8192];
                let _ = stream.read(&mut request).await;
                server_requests.fetch_add(1, Ordering::SeqCst);
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });

        (handle, address, requests)
    }

    async fn call(address: std::net::SocketAddr) -> Result<ChatResponse> {
        openai_compat_call(
            &reqwest::Client::new(),
            &format!("http://{address}"),
            "test-key",
            &[],
            "test-model",
            &[Message::user("hello")],
            None,
            &HashMap::new(),
        )
        .await
    }

    #[tokio::test]
    async fn retries_when_a_success_response_body_is_truncated() {
        // Content-Length promises more bytes than are sent: a transport-level
        // decode error, surfaced by `response.text()`.
        let (server, address, requests) = spawn_server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{".to_string(),
            http_200(VALID_BODY),
        ]);

        let response = call(address).await.unwrap();

        server.await.unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert_eq!(response.choices.len(), 1);
    }

    #[tokio::test]
    async fn retries_when_a_complete_response_holds_truncated_json() {
        // A well-formed HTTP response whose body is valid up to a point and then
        // simply stops. `response.text()` succeeds; only the JSON parse fails.
        // This is the shape observed in production behind a stalling provider.
        let cut = &VALID_BODY[..40];
        let (server, address, requests) = spawn_server(vec![http_200(cut), http_200(VALID_BODY)]);

        let response = call(address).await.unwrap();

        server.await.unwrap();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            2,
            "truncated JSON should be retried once"
        );
        assert_eq!(response.choices.len(), 1);
    }

    #[tokio::test]
    async fn does_not_retry_structurally_invalid_json() {
        // `choices` is an object where a sequence is required: no amount of
        // retrying changes the shape, so exactly one request must be made.
        let wrong_shape = r#"{"choices":{"message":"nope"},"usage":null}"#;
        let (server, address, requests) = spawn_server(vec![
            http_200(wrong_shape),
            http_200(VALID_BODY),
            http_200(VALID_BODY),
        ]);

        let error = call(address).await.unwrap_err();

        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "a structurally invalid payload must fail on the first attempt"
        );
        assert!(
            format!("{error:#}").contains("Failed to parse LLM response"),
            "unexpected error: {error:#}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn surfaces_provider_error_object_returned_under_http_200() {
        let body = r#"{"error":{"message":"upstream timed out","code":504}}"#;
        let (server, address, requests) = spawn_server(vec![http_200(body), http_200(VALID_BODY)]);

        let error = call(address).await.unwrap_err();

        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let rendered = format!("{error:#}");
        assert!(rendered.contains("upstream timed out"), "got: {rendered}");
        assert!(rendered.contains("504"), "got: {rendered}");
        server.abort();
    }

    fn body_with_runtime_tools() -> serde_json::Map<String, Value> {
        let body = serde_json::json!({
            "model": "test-model",
            "messages": [],
            "tools": [
                { "type": "function", "function": { "name": "github__get_issue" } },
                { "type": "function", "function": { "name": "atoma_builtin__load_skill" } },
            ],
            "tool_choice": "auto",
        });
        body.as_object().unwrap().clone()
    }

    fn tool_names(body: &serde_json::Map<String, Value>) -> Vec<String> {
        body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                t.pointer("/function/name")
                    .or_else(|| t.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn extra_body_tools_are_appended_not_substituted_for_runtime_tools() {
        let mut body = body_with_runtime_tools();
        let extra = HashMap::from([(
            "tools".to_string(),
            serde_json::json!([
                { "type": "openrouter:web_search" },
                { "type": "openrouter:web_fetch" },
            ]),
        )]);

        merge_extra_body(&mut body, &extra);

        assert_eq!(
            tool_names(&body),
            vec![
                "github__get_issue",
                "atoma_builtin__load_skill",
                "openrouter:web_search",
                "openrouter:web_fetch",
            ],
            "runtime tool schemas must survive alongside the agent's server tools"
        );
    }

    #[test]
    fn extra_body_tools_stand_alone_when_there_are_no_runtime_tools() {
        let mut body = serde_json::json!({ "model": "m", "messages": [] })
            .as_object()
            .unwrap()
            .clone();
        let extra = HashMap::from([(
            "tools".to_string(),
            serde_json::json!([{ "type": "openrouter:web_search" }]),
        )]);

        merge_extra_body(&mut body, &extra);

        assert_eq!(tool_names(&body), vec!["openrouter:web_search"]);
    }

    #[test]
    fn a_non_array_extra_body_tools_cannot_strip_the_runtime_tools() {
        let mut body = body_with_runtime_tools();
        let extra = HashMap::from([("tools".to_string(), serde_json::json!("web_search"))]);

        merge_extra_body(&mut body, &extra);

        assert_eq!(
            tool_names(&body),
            vec!["github__get_issue", "atoma_builtin__load_skill"]
        );
    }

    #[test]
    fn extra_body_overrides_other_keys_and_never_reserved_ones() {
        let mut body = body_with_runtime_tools();
        let extra = HashMap::from([
            ("model".to_string(), serde_json::json!("hijacked")),
            ("messages".to_string(), serde_json::json!(["hijacked"])),
            ("tool_choice".to_string(), serde_json::json!("none")),
            ("temperature".to_string(), serde_json::json!(0)),
            (
                "provider".to_string(),
                serde_json::json!({ "order": ["Xiaomi"], "allow_fallbacks": false }),
            ),
        ]);

        merge_extra_body(&mut body, &extra);

        assert_eq!(body["model"], serde_json::json!("test-model"));
        assert_eq!(body["messages"], serde_json::json!([]));
        assert_eq!(body["tool_choice"], serde_json::json!("none"));
        assert_eq!(body["temperature"], serde_json::json!(0));
        assert_eq!(body["provider"]["order"], serde_json::json!(["Xiaomi"]));
    }

    #[test]
    fn backoff_grows_between_attempts() {
        assert_eq!(retry_backoff(1), Duration::from_millis(1_000));
        assert_eq!(retry_backoff(2), Duration::from_millis(4_000));
        assert!(retry_backoff(2) > retry_backoff(1));
    }

    #[test]
    fn truncation_and_shape_errors_are_classified_apart() {
        let truncated = serde_json::from_str::<ChatResponse>(&VALID_BODY[..40]).unwrap_err();
        assert!(is_truncated(&truncated), "expected Eof classification");

        let wrong_shape = r#"{"choices":{"a":1},"usage":null}"#;
        let error = serde_json::from_str::<ChatResponse>(wrong_shape).unwrap_err();
        assert!(
            !is_truncated(&error),
            "wrong shape must not be treated as truncation"
        );
    }
}

#[cfg(test)]
mod tool_image_tests {
    use super::split_images_out_of_tool_message;
    use serde_json::{json, Value};

    fn tool_with_image() -> Value {
        json!({
            "role": "tool",
            "tool_call_id": "call_1",
            "content": [
                {"type": "text", "text": "Here is the screen:"},
                {"type": "image", "data": "AAAA", "mimeType": "image/png"},
            ],
        })
    }

    // The OpenAI schema has no way to return an image from a tool, so the only
    // route by which a model on this API ever sees one is a following user
    // message.
    #[test]
    fn a_tool_result_with_a_picture_becomes_two_messages() {
        let out = split_images_out_of_tool_message(tool_with_image());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "tool");
        assert_eq!(out[0]["content"], "Here is the screen:");
        assert_eq!(out[0]["tool_call_id"], "call_1");
        assert_eq!(out[1]["role"], "user");
        assert_eq!(
            out[1]["content"][0]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
    }

    #[test]
    fn a_text_only_tool_result_stays_one_message() {
        let msg = json!({"role": "tool", "tool_call_id": "c", "content": "done"});
        assert_eq!(split_images_out_of_tool_message(msg.clone()), vec![msg]);
    }

    #[test]
    fn other_roles_are_untouched() {
        let msg = json!({"role": "user", "content": "hello"});
        assert_eq!(split_images_out_of_tool_message(msg.clone()), vec![msg]);
    }
}
