use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::error::Category;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

use crate::domain::ports::{DeltaHandler, FinishReason, LlmChoice, LlmResponse, LlmUsage};
use crate::domain::session::{Message, ToolCall, ToolCallFunction};

const MAX_HTTP_ATTEMPTS: u8 = 3;

/// How many times a turn whose answer stopped arriving is asked for again.
///
/// The retry that matters is the one [`send_sse_with_retry`] refuses to make. Once the
/// stream has started, a transport failure ends the *request* — but it does not
/// invalidate the *turn*, and the two were being treated as the same thing: the run
/// failed, and 14 minutes of an agent's work went with it.
///
/// Nothing has been decided when that happens. Deltas reach the repetition detector
/// and nothing else; tool calls are executed from the assembled response, which never
/// assembled, so no tool ran. Re-asking therefore cannot duplicate a side effect —
/// which is the one thing that made re-sending a half-read stream unsafe to do inside
/// the provider layer.
///
/// Bounded, unlike the `--loop-retries` re-ask: a loop is a property of one sample and
/// can be re-sampled as long as the clock allows, while a connection that dies this way
/// three times running is a provider or network condition that another try will not
/// change.
pub(crate) const MAX_STREAM_RETRIES: u8 = 3;

/// A completion that stopped arriving before the provider said it was finished.
///
/// Its own type so the runner can tell it apart without matching on a message, the
/// same reason [`crate::domain::repetition::Repetition`] is one. What it carries is
/// the part that DID arrive: already paid for, and the thing the model continues from
/// rather than starting the answer over.
#[derive(Debug)]
pub struct StreamInterrupted {
    /// The answer text received before the connection failed. May be empty.
    pub received: String,
    /// What actually failed, for the note and for the log.
    pub cause: String,
}

impl std::fmt::Display for StreamInterrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the response stopped arriving before it finished: {}",
            self.cause
        )
    }
}

impl std::error::Error for StreamInterrupted {}

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
///
/// Every name any adapter assembles itself, not only the ones this dialect uses.
/// Each adapter used to carry its own list — `["model","messages"]` here,
/// `["model","input","messages"]` in the Responses adapter, six names inline in the
/// Anthropic one, and a fourth copy in `application::validator`. Four lists meant four
/// answers to "may an agent set this", and `atoma validate` was checking against the
/// one that did not apply.
///
/// `tools` is deliberately NOT here: an agent may add to it, which is what
/// [`reconcile_tools`] is for. Reserving it would break OpenRouter's server tools;
/// letting it through plainly would drop every MCP schema.
pub const RESERVED_KEYS: [&str; 5] = ["model", "messages", "input", "system", "store"];

/// The callable inside a tool definition.
///
/// Atoma's internal representation of a tool IS Chat Completions' shape --
/// `{"type": "function", "function": {…}}` -- and every producer emits it: the skill
/// loader (`application::tools`), the MCP registry (`infra::mcp::tool_definitions`),
/// and the test mock. `shared::chat_completion` sends that form verbatim, which is
/// why it is the internal one.
///
/// Both dialect adapters used to write `tool.get("function").unwrap_or(tool)`, which
/// says a bare definition is a second legitimate shape. Nothing produces one. What
/// the fallback actually did was turn a malformed definition into a tool named `""`
/// with an empty schema, sent to the provider as though it were real.
pub fn tool_function(tool: &Value) -> Result<&Value> {
    tool.get("function").ok_or_else(|| {
        anyhow::anyhow!(
            "a tool definition has no `function` object: {}",
            serde_json::to_string(tool).unwrap_or_else(|_| "<unserializable>".to_string())
        )
    })
}

/// Reconcile an agent's `extra_body.tools` with the runtime tool definitions.
///
/// Appending rather than replacing is the point. A plain insert would REPLACE the
/// runtime tools, silently stripping every MCP tool's JSON Schema from the request —
/// and because the system prompt lists only tool *names*, the model is then left to
/// guess argument shapes, observed in production as a stream of wrong-typed and
/// missing arguments.
///
/// OpenRouter's server tools (`{"type": "openrouter:web_search"}`) are declared in
/// this same array and are documented to work alongside user-defined tools, so both
/// sets belong in it.
///
/// Total, because its caller has already established that `extra` is an array. It
/// used to take a `Value` and, when that was not an array, log a warning and keep the
/// runtime tools — accepting a malformed declaration in place of refusing it, and
/// leaving the agent's `tools` silently absent from the request it was written for.
/// The check moved to [`merge_extra_body`], where it can be made once for every
/// shape of that key rather than only when there are runtime tools to protect.
fn reconcile_tools(runtime: Option<&Value>, extra: &[Value]) -> Value {
    // No runtime tools to protect: whatever the agent supplied stands alone.
    let Some(runtime) = runtime.and_then(Value::as_array) else {
        return Value::Array(extra.to_vec());
    };
    let mut merged = runtime.clone();
    merged.extend(extra.iter().cloned());
    Value::Array(merged)
}

/// Merge an agent's `extra_body` into an assembled request body.
///
/// Reserved keys are dropped; `tools` is merged with what the runtime already
/// put there; everything else overrides.
///
/// `pub` because it is the policy rather than this dialect's helper. The Responses
/// adapter assembled its own version of this loop and left out the `tools`
/// reconciliation, so on that path an agent's `extra_body.tools` REPLACED every MCP
/// tool definition — the exact failure documented on [`reconcile_tools`], on the path
/// this repository's own agents actually use.
pub fn merge_extra_body(
    body: &mut serde_json::Map<String, Value>,
    extra_body: &std::collections::HashMap<String, Value>,
) -> Result<()> {
    for (key, value) in extra_body {
        if RESERVED_KEYS.contains(&key.as_str()) {
            continue;
        }
        if key == "tools" {
            // Checked here rather than inside `reconcile_tools`, so the answer does not
            // depend on whether there happened to be runtime tools to merge with. A
            // `tools` that is not an array used to be a warning on one path and a plain
            // insert on the other -- the same malformed declaration, tolerated twice in
            // two different ways, with the agent's own tools going nowhere either time.
            let extra = value.as_array().ok_or_else(|| {
                anyhow::anyhow!(
                    "this agent's `extra_body.tools` is not an array, so it cannot be \
                     added to the tools this run offers. Write it as a list of tool \
                     definitions, or remove it."
                )
            })?;
            body.insert(key.clone(), reconcile_tools(body.get("tools"), extra));
            continue;
        }
        body.insert(key.clone(), value.clone());
    }
    Ok(())
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

/// Not `Copy`: `unread` is a map. Nothing takes this by value twice -- its one
/// consumer maps it into `LlmUsage`, which stays `Copy`.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// The cache breakdown, for a provider that sends one.
    ///
    /// Absent for one that does not, and absent is the answer -- see
    /// `LlmUsage::cached_prompt_tokens` for why it must not become zero on the way
    /// through.
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    /// The cached part a provider WROTE, which nothing on any wire read here
    /// reports today.
    ///
    /// Anthropic's translation fills it in -- that API is the one charging for a
    /// cache write -- and it lives on this struct rather than beside it because this
    /// is the shape every chat-completions adapter hands to `chat_response_to_llm`.
    #[serde(default)]
    pub prompt_cache_write_tokens: Option<u64>,
    /// Every usage field nothing above reads.
    ///
    /// Kept so that "this provider reports no cache" can be told apart from "this
    /// provider spells it something we do not read". Without it the two are the same
    /// silence, and the second is diagnosed by guessing a field name and shipping a
    /// release to find out whether the guess was right.
    #[serde(flatten)]
    pub unread: BTreeMap<String, Value>,
}

/// What a provider says about the cached part of a prompt.
///
/// `Copy` because `LlmUsage` is: this is read out of a `Usage` by value on the way
/// through, and it is one machine word behind an `Option`.
#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u64>,
}

/// What a provider sent back: the body an adapter asked for, and what that provider
/// calls the request which produced it.
///
/// A pair rather than a second request function, because the id is not an extra fact
/// about some calls -- it is how any one call is named afterwards, and a function that
/// returns the body alone is exactly what dropped it. Every caller of
/// [`send_json_with_retry`] is an adapter assembling an `LlmResponse`, and all three
/// have somewhere to put it.
///
/// `Debug` because a test asserting on a failed call unwraps the error, which asks the
/// success type to be printable.
#[derive(Debug)]
pub struct ProviderReply<T> {
    pub body: T,
    /// The provider's id for this request, or `None` if it sent none this adapter
    /// reads. See `LlmResponse::request_id` for why absent must stay absent.
    pub request_id: Option<String>,
}

/// Header names a provider returns its own request id under, tried in order.
///
/// One shared list rather than a name per adapter, and that is a deliberate departure
/// from where dialect differences usually live. The spelling is chosen by the endpoint
/// that answers, not by the wire format it speaks: `openai_compat_call` alone carries
/// OpenAI, OpenRouter, orcarouter and GitHub Copilot, and `PROVIDERS` documents
/// `OPENAI_BASE_URL` as the way to reach any other host speaking either dialect. An
/// adapter asked to name "its" header would therefore be answering for a host it does
/// not know, and would be wrong in precisely the case that table exists to support.
///
/// - `x-request-id` is OpenAI's, and what endpoints built to its shape return with it.
/// - `request-id` is Anthropic's -- the id its support asks for.
/// - `apim-request-id` and `x-ms-request-id` are Azure OpenAI's, reachable here by
///   pointing `OPENAI_BASE_URL` at a deployment.
///
/// First match wins. A gateway that stamps its own `x-request-id` in front of an
/// upstream's id answers for itself, which is the right answer: that gateway is the
/// party a support conversation would be had with.
///
/// A provider spelling it something absent from this list reports the same silence as
/// one that sends no id at all, and the fix is a line here rather than anything
/// structural.
const REQUEST_ID_HEADERS: [&str; 4] = [
    "x-request-id",
    "request-id",
    "apim-request-id",
    "x-ms-request-id",
];

/// The provider's id for a response, read off the headers before anything consumes it.
fn provider_request_id(headers: &reqwest::header::HeaderMap) -> Option<String> {
    REQUEST_ID_HEADERS
        .iter()
        .find_map(|name| headers.get(*name)?.to_str().ok())
        // A header present but empty is not an id. Without this the blank is carried
        // all the way into the log line, which prints `request=` followed by nothing --
        // the exact shape three doc comments here promise never to produce.
        .filter(|id| !id.is_empty())
        .map(str::to_string)
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
///
/// Returns the headers' correlation key alongside the body. It used to return the
/// body alone, so the request id arrived in this process and was dropped one line
/// before anything could keep it — see [`ProviderReply`].
pub(crate) async fn send_json_with_retry<T: DeserializeOwned>(
    label: &str,
    build_request: impl Fn() -> reqwest::RequestBuilder,
) -> Result<ProviderReply<T>> {
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

        // Read before anything consumes the response: `text()` takes it by value, and
        // the headers are the only place the correlation key ever appears.
        let request_id = provider_request_id(response.headers());

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
            // The refused call named, when the provider named it. A status and a
            // sentence of error text is all a killed run leaves behind, and on its own
            // it does not say which of the provider's records to ask about.
            let attribution = request_id
                .as_deref()
                .map_or_else(String::new, |id| format!(" [request {id}]"));
            anyhow::bail!(
                "{} API error ({}){}: {}",
                label,
                status,
                attribution,
                error_text
            );
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
            Ok(parsed) => {
                return Ok(ProviderReply {
                    body: parsed,
                    request_id,
                })
            }
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

// ── Streaming ─────────────────────────────────────────────────────────────────

/// One `data:` payload from an SSE stream, or `None` for a line that carries none.
///
/// The SSE framing is three things and this reads all three: `data:` lines carry the
/// payload, `event:` and `id:` lines are metadata nothing here uses, and a blank line
/// ends an event. A payload of `[DONE]` is OpenAI's end-of-stream marker rather than
/// JSON, and is reported as `None` so the caller stops on it rather than trying to
/// parse it.
///
/// Multi-line `data:` fields are joined with newlines, which the specification
/// requires and which no provider read here actually sends. Doing it anyway costs one
/// branch and means a provider that does is not silently truncated.
fn sse_data(line: &str) -> Option<&str> {
    let payload = line.strip_prefix("data:")?.trim_start();
    if payload == "[DONE]" {
        return None;
    }
    Some(payload)
}

/// Read an SSE body, handing each JSON payload to `on_event`.
///
/// `on_event` returns `Err` to stop reading, and that error is what this returns. It
/// is the abort path: the response body is dropped, the connection is closed, and the
/// provider stops generating. Nothing else in this file can interrupt a completion
/// that is already under way.
///
/// Chunk boundaries do not respect line boundaries, so a partial line is held until
/// the rest of it arrives. Without that, a delta split across two TCP reads would be
/// parsed as two malformed payloads and the completion would fail on a healthy stream.
async fn read_sse(
    mut response: reqwest::Response,
    mut on_event: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let mut buffer = String::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Failed to read a chunk of the streamed response")?
    {
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Everything up to the last newline is complete lines; the remainder is a
        // partial one and stays in the buffer.
        while let Some(newline) = buffer.find('\n') {
            let line = buffer[..newline].trim_end_matches('\r').to_string();
            buffer.drain(..=newline);
            if let Some(payload) = sse_data(&line) {
                on_event(payload)?;
            }
        }
    }
    Ok(())
}

/// What a streamed chat-completions reply accumulates to.
///
/// The wire format spreads one message across many chunks: the role arrives first,
/// then text a piece at a time, then each tool call's name and arguments in fragments
/// that must be concatenated by index. This gathers all of that back into the single
/// message the non-streaming path would have returned, so the inference loop reads one
/// shape whichever path it took.
#[derive(Default)]
struct StreamAccumulator {
    text: String,
    /// The chain of thought, gathered the same way the text is.
    ///
    /// Kept and handed back rather than dropped -- see `Message::reasoning_content`.
    /// A thinking-mode provider that carries `tools` requires it on the next request,
    /// and a streamed reply is no different from a non-streamed one in that respect.
    reasoning: String,
    /// Tool calls by their `index`, because that is the only field the fragments of
    /// one call share. `id` and `name` arrive once, `arguments` in pieces.
    tool_calls: BTreeMap<u64, PartialToolCall>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl StreamAccumulator {
    /// Fold one chunk's `choices[0]` into what has been gathered so far.
    ///
    /// Returns the text this chunk added, if any, so the caller can hand it to the
    /// delta handler. Tool-call fragments are not returned: they are not text, and the
    /// repetition detector watches text.
    fn absorb(&mut self, chunk: &Value) -> String {
        if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            if let Ok(parsed) = serde_json::from_value::<Usage>(usage.clone()) {
                self.usage = Some(parsed);
            }
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return String::new();
        };

        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(reason.to_string());
        }

        let Some(delta) = choice.get("delta") else {
            return String::new();
        };

        let mut added = String::new();
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            self.text.push_str(text);
            added.push_str(text);
        }

        // Gathered AND returned as a delta, so the repetition detector sees it.
        //
        // A reasoning model that has fallen into a loop does so inside its chain of
        // thought, and the loop is visible there long before it reaches the answer --
        // which is the whole point of watching the stream. A chain of thought that
        // circles is not "thinking"; it is the pathology this exists for, and the
        // detector's window is wide enough (500 tokens, a quarter of the output
        // ceiling at its smallest) that ordinary deliberation never approaches it.
        if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
            self.reasoning.push_str(reasoning);
            added.push_str(reasoning);
        }

        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
            let entry = self.tool_calls.entry(index).or_default();
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                entry.id = id.to_string();
            }
            if let Some(function) = call.get("function") {
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    entry.name.push_str(name);
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    entry.arguments.push_str(arguments);
                }
            }
        }

        added
    }

    /// The answer text received so far, taken out for an interrupted stream.
    ///
    /// The same text [`Self::into_response`] would have put in an assistant message, so
    /// a caller that continues a cut-off answer continues the real thing rather than a
    /// reconstruction of it. Tool-call fragments are deliberately not included: a half
    /// assembled call is not a call, and the runner discards those anyway — only the
    /// text is carried into the note that asks for the rest.
    fn into_received_text(self) -> String {
        self.text
    }

    /// The reply the non-streaming path would have produced.
    fn into_response(self, request_id: Option<String>) -> LlmResponse {
        let tool_calls: Vec<ToolCall> = self
            .tool_calls
            .into_values()
            .map(|call| ToolCall {
                id: call.id,
                type_: "function".to_string(),
                function: ToolCallFunction {
                    name: call.name,
                    arguments: call.arguments,
                },
            })
            .collect();

        LlmResponse {
            choices: vec![LlmChoice {
                message: Message {
                    reasoning_content: (!self.reasoning.is_empty()).then_some(self.reasoning),
                    ..Message::assistant(
                        (!self.text.is_empty()).then_some(self.text.as_str()),
                        (!tool_calls.is_empty()).then_some(tool_calls),
                    )
                },
                finish_reason: self
                    .finish_reason
                    .as_deref()
                    .and_then(FinishReason::from_openai),
            }],
            usage: self.usage.map(|u| {
                let cached = u.prompt_tokens_details.and_then(|d| d.cached_tokens);
                if cached.is_none() {
                    report_unread_usage(&u.unread);
                }
                LlmUsage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                    cached_prompt_tokens: cached,
                    written_prompt_tokens: u.prompt_cache_write_tokens,
                }
            }),
            request_id,
        }
    }
}

/// POST a request and read its SSE body, handing each JSON payload to `on_event`.
///
/// The streaming counterpart of [`send_json_with_retry`], and deliberately not a
/// retry loop once the stream has started. A stream that fails halfway has already
/// produced text the caller has seen and acted on, so re-sending it would duplicate
/// that text in the session. The retry that matters -- a connection that never
/// opened -- is still made, because nothing has been emitted at that point.
///
/// `on_event` returning `Err` aborts: the body is dropped and the error is returned
/// unchanged, so the caller's own error type survives the trip. That is the abort
/// path the repetition circuit breaker uses.
///
/// Returns the provider's request id, which arrives in a header and so is read before
/// the body is consumed.
pub(crate) async fn send_sse_with_retry(
    label: &str,
    build_request: impl Fn() -> reqwest::RequestBuilder,
    mut on_event: impl FnMut(&str) -> Result<()>,
) -> Result<Option<String>> {
    let mut attempt = 1;
    loop {
        let response = match build_request().send().await {
            Ok(response) => response,
            Err(error) if attempt < MAX_HTTP_ATTEMPTS && is_transient(&error) => {
                retry_delay(attempt, &error.to_string()).await;
                attempt += 1;
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to send {label} request"))
            }
        };

        let request_id = provider_request_id(response.headers());

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
                attempt += 1;
                continue;
            }
            let attribution = request_id
                .as_deref()
                .map_or_else(String::new, |id| format!(" [request {id}]"));
            anyhow::bail!(
                "{} API error ({}){}: {}",
                label,
                status,
                attribution,
                error_text
            );
        }

        // The abort error is kept apart from the transport's, because the two mean
        // different things and the caller needs to tell them apart: one is the
        // content being refused, the other is the connection failing.
        let mut abort: Option<anyhow::Error> = None;
        let outcome = read_sse(response, |payload| match on_event(payload) {
            Ok(()) => Ok(()),
            Err(e) => {
                abort = Some(e);
                anyhow::bail!("stream aborted by the caller")
            }
        })
        .await;

        return match (outcome, abort) {
            (_, Some(abort)) => Err(abort),
            (Err(e), None) => Err(e),
            (Ok(()), None) => Ok(request_id),
        };
    }
}

/// POST a request and read its SSE body, gathering a chat-completions reply.
///
/// The chat-completions counterpart of [`send_sse_with_retry`], which does the
/// transport work. This adds the dialect: the accumulator, the mid-stream provider
/// error check, and the delta handler.
pub(crate) async fn send_streaming_with_retry(
    label: &str,
    build_request: impl Fn() -> reqwest::RequestBuilder,
    on_delta: &mut DeltaHandler<'_>,
) -> Result<LlmResponse> {
    let mut accumulator = StreamAccumulator::default();

    let outcome = send_sse_with_retry(label, build_request, |payload| {
        let chunk: Value = serde_json::from_str(payload)
            .with_context(|| format!("Failed to parse a {label} stream chunk: {payload}"))?;

        // A provider error delivered mid-stream, which OpenRouter does. The
        // non-streaming path checks for the same shape under HTTP 200.
        if let Some(error_obj) = chunk.get("error").filter(|e| !e.is_null()) {
            let msg = error_obj
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown provider error");
            anyhow::bail!("LLM provider error: {}", msg);
        }

        let added = accumulator.absorb(&chunk);
        if added.is_empty() {
            return Ok(());
        }
        on_delta(&added)
    })
    .await;

    let request_id = match outcome {
        Ok(request_id) => request_id,
        Err(e) => {
            // A transport failure that arrived after the stream had started is handed
            // back as its own type, with the part already received, so the caller can
            // re-ask the TURN rather than the REQUEST. Retrying here is not enough: the
            // request is rebuilt from the session, which does not hold the half-written
            // answer (see [`StreamInterrupted`]).
            //
            // Everything else is passed through unchanged — a repetition abort, a
            // provider error delivered mid-stream, a malformed chunk, a failed
            // connection. None of those is the same question as "the answer stopped
            // arriving", and only the last one is worth asking again.
            if !is_stream_interrupted(&e) {
                return Err(e);
            }
            return Err(anyhow::Error::new(StreamInterrupted {
                received: accumulator.into_received_text(),
                cause: e.to_string(),
            }));
        }
    };

    Ok(accumulator.into_response(request_id))
}

/// Whether this is a stream that stopped arriving, rather than an answer.
///
/// Matched on the one context string [`read_sse`] adds, which is the only place a
/// response body is read a chunk at a time. A repetition abort does not come from
/// there (it comes back through `abort`, and its own type is looked for first), and
/// neither does a non-2xx status, a failed connection, or a provider error delivered
/// mid-stream as a chunk.
fn is_stream_interrupted(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("Failed to read a chunk"))
}

/// Text of the assistant message bridging a tool result to its pictures.
const IMAGE_BRIDGE: &str = "Passing along the image from that tool result.";

/// A `data:` URI for an MCP image block, or `None` if it is not one.
pub(crate) fn mcp_image_data_uri(block: &Value) -> Option<String> {
    if block.get("type").and_then(Value::as_str) != Some("image") {
        return None;
    }
    let data = block.get("data").and_then(Value::as_str)?;
    let mime = block.get("mimeType").and_then(Value::as_str)?;
    Some(format!("data:{};base64,{}", mime, data))
}

/// Rewrite MCP content blocks into the parts an OpenAI message takes.
///
/// A picture reaches a run from two directions: a tool that returns one, and a
/// person who attached one to the issue. Only the first needs the tool-message
/// workaround above — a `user` message carries `image_url` parts natively, which
/// is why this is a plain rewrite rather than a reshuffle.
///
/// Applied after that split, and harmless there: what the split emits is already
/// `image_url`, and only `type: "image"` is touched.
fn mcp_blocks_to_openai(message: Value) -> Value {
    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
        return message;
    };
    if !blocks.iter().any(|b| mcp_image_data_uri(b).is_some()) {
        return message;
    }

    let parts: Vec<Value> = blocks
        .iter()
        .map(|block| match mcp_image_data_uri(block) {
            Some(url) => serde_json::json!({ "type": "image_url", "image_url": { "url": url } }),
            None => block.clone(),
        })
        .collect();

    let mut out = message;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("content".to_string(), Value::Array(parts));
    }
    out
}

/// Move a tool result's pictures into a following `user` message.
///
/// A workaround, and worth naming as one. The Chat Completions schema has no
/// way to return an image from a tool — a `tool` message's content is text — so
/// on this API the only route to the model is a later message. The API that
/// does it properly is OpenAI's Responses API, whose `function_call_output`
/// carries image parts; `infra/llm/openai_responses.rs` takes that path.
///
/// Three messages, not two: tool result, a short assistant line, then the user
/// message with the pictures. The bridge exists because providers that enforce
/// strict role alternation reject a `user` straight after a `tool` with
/// `400 Unexpected role 'user' after role 'tool'` — reported against Mistral-
/// backed endpoints, and the same fix others landed. OpenAI and Google accept
/// either shape, so the stricter one is the one to send.
///
/// The synthetic messages are built here and nowhere else. The session keeps
/// the single tool message it recorded, so nothing about this reaches disk, and
/// a run resumed against Anthropic — which can carry an image in a tool result
/// — takes that path instead with no trace of this one.
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
                let Some(url) = mcp_image_data_uri(block) else {
                    continue;
                };
                images.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": { "url": url },
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
        serde_json::json!({ "role": "assistant", "content": IMAGE_BRIDGE }),
        serde_json::json!({ "role": "user", "content": images }),
    ]
}

/// Shared OpenAI-compatible HTTP call used by OpenAI and Copilot providers.
///
/// Returns the reply with the provider's request id beside it, because the four
/// vendors reached through here are four different support conversations and each
/// needs its own side of the call named.
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
) -> Result<ProviderReply<ChatResponse>> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = chat_completions_body(model, messages, tools, extra_body)?;

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

/// The same call, streamed.
///
/// `stream: true` is added to the body here rather than by the caller, so the two
/// paths cannot disagree about it -- and so an agent's `extra_body` cannot turn
/// streaming off for a call that is being read as a stream.
#[allow(clippy::too_many_arguments)]
pub async fn openai_compat_call_streaming(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    extra_headers: &[(String, String)],
    model: &str,
    messages: &[Message],
    tools: Option<&[Value]>,
    extra_body: &std::collections::HashMap<String, Value>,
    on_delta: &mut DeltaHandler<'_>,
) -> Result<LlmResponse> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut body = chat_completions_body(model, messages, tools, extra_body)?;
    body["stream"] = Value::Bool(true);
    // Ask for the usage block in the final chunk. Without it a streamed call reports
    // no tokens at all, and the per-inference cost line -- the reason the counts are
    // gathered -- would go silent on exactly the path that is now the default.
    body["stream_options"] = serde_json::json!({ "include_usage": true });

    tracing::debug!("Request URL: {}", url);
    tracing::debug!("Request body: {}", serde_json::to_string_pretty(&body)?);

    send_streaming_with_retry(
        "LLM",
        || {
            let mut request = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json");

            for (name, value) in extra_headers {
                request = request.header(name.as_str(), value.as_str());
            }

            request.json(&body)
        },
        on_delta,
    )
    .await
}

/// Assemble a chat-completions request body, without the streaming flags.
///
/// Shared by both paths so the message rewriting, the tool merge and the reserved-key
/// policy are written once. They were about to be written twice, which is how the two
/// paths would have drifted -- and the drift would have been invisible, because each
/// path is exercised by different providers.
fn chat_completions_body(
    model: &str,
    messages: &[Message],
    tools: Option<&[Value]>,
    extra_body: &std::collections::HashMap<String, Value>,
) -> Result<Value> {
    let llm_messages: Vec<Value> = messages
        .iter()
        .flat_map(|m| split_images_out_of_tool_message(m.to_llm_value()))
        .map(mcp_blocks_to_openai)
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
        merge_extra_body(obj, extra_body)?;
    }

    Ok(body)
}

/// Turns a chat-completions reply into the domain reply.
///
/// Three adapters speak this dialect -- OpenAI, Copilot, and Anthropic after its own
/// translation -- and each used to carry its own copy of this mapping. A field added
/// to `LlmUsage` then had to be remembered in three places, and the one that forgot
/// would report the same call differently from the others.
/// Says once, per process, that a provider sent usage fields nothing here reads and
/// no cached-prompt figure under any name it does.
///
/// Those two together are the signature of a provider spelling the cache something
/// new, which is otherwise indistinguishable from a provider that has no cache to
/// report: the metrics downstream say `unknown` for both, correctly and uselessly.
/// Once per process because the answer is a field name -- seeing it a second time
/// adds nothing, and every inference of every run would say it.
///
/// Names only, never values. The names are what identifies the spelling, and this
/// line is printed into a workflow log anyone can read.
pub(crate) fn report_unread_usage(unread: &BTreeMap<String, Value>) {
    if unread.is_empty() {
        return;
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        tracing::warn!(
            "Provider reported usage but no cached-prompt figure under any name this \
             adapter reads. Usage fields it sent that nothing here reads: {}. If one of \
             them is the cache figure, it belongs in `Usage`.",
            unread.keys().cloned().collect::<Vec<_>>().join(", "),
        );
    });
}

/// `request_id` is passed beside the body rather than carried inside `ChatResponse`,
/// because it is not part of this dialect's payload: it arrives in a header, and the
/// Anthropic adapter — which produces a `ChatResponse` by translation rather than by
/// deserialising one — has it in hand at the same point.
pub fn chat_response_to_llm(resp: ChatResponse, request_id: Option<String>) -> LlmResponse {
    LlmResponse {
        choices: resp
            .choices
            .into_iter()
            .map(|c| LlmChoice {
                message: c.message,
                // The canonical spelling is this dialect's own, so a value that does not
                // read is a provider inventing one -- `None`, and the runner says so,
                // rather than being quietly taken for `stop`.
                finish_reason: c
                    .finish_reason
                    .as_deref()
                    .and_then(FinishReason::from_openai),
            })
            .collect(),
        usage: resp.usage.map(|u| {
            // A provider that sends no cache breakdown stays `None`: zero would be a
            // claim that the cache did nothing, indistinguishable afterwards from a
            // provider that never reports. Which of the two it is, the line below
            // answers -- rather than a guess at what this provider might call it.
            let cached = u.prompt_tokens_details.and_then(|d| d.cached_tokens);
            if cached.is_none() {
                report_unread_usage(&u.unread);
            }
            LlmUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
                cached_prompt_tokens: cached,
                written_prompt_tokens: u.prompt_cache_write_tokens,
            }
        }),
        request_id,
    }
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

    /// The same, with one extra header line written verbatim -- which is how a
    /// provider's correlation key actually arrives.
    fn http_200_with(header: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            header,
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

    async fn call(address: std::net::SocketAddr) -> Result<ProviderReply<ChatResponse>> {
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
        assert_eq!(response.body.choices.len(), 1);
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
        assert_eq!(response.body.choices.len(), 1);
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

    /// The provider's own name for this request, kept rather than dropped with the
    /// rest of the headers.
    ///
    /// It is the only thing that lets one inference here and one record on the
    /// provider's side be shown to be the same call. Without it, "the cache hit on
    /// this turn and missed on the next" is a conclusion drawn from our own numbers
    /// and cannot be checked against theirs.
    #[tokio::test]
    async fn a_providers_request_id_is_read_off_the_response() {
        let served = http_200_with("x-request-id: req_0123456789", VALID_BODY);
        let (server, address, _) = spawn_server(vec![served]);

        let reply = call(address).await.unwrap();

        server.await.unwrap();
        assert_eq!(reply.request_id.as_deref(), Some("req_0123456789"));
    }

    /// Anthropic spells it without the prefix, so both spellings are read here.
    ///
    /// Which one arrives is the answering endpoint's choice, not the dialect's: this
    /// call speaks chat-completions and still finds it, which is the point of keeping
    /// the names in one list instead of one per adapter.
    #[tokio::test]
    async fn the_unprefixed_spelling_is_read_as_well() {
        let served = http_200_with("request-id: req_anthropic", VALID_BODY);
        let (server, address, _) = spawn_server(vec![served]);

        let reply = call(address).await.unwrap();

        server.await.unwrap();
        assert_eq!(reply.request_id.as_deref(), Some("req_anthropic"));
    }

    /// A provider that sends none reports none. An empty string would read as an
    /// identifier -- one that no support conversation could ever find, asked about by
    /// somebody who believed they had one.
    #[tokio::test]
    async fn a_response_without_one_is_recorded_as_absent_rather_than_blank() {
        let (server, address, _) = spawn_server(vec![http_200(VALID_BODY)]);

        let reply = call(address).await.unwrap();

        server.await.unwrap();
        assert_eq!(reply.request_id, None);
    }

    /// A header that is present and empty is not an id. A gateway that answers
    /// `x-request-id:` with nothing after it used to reach `LlmResponse` as
    /// `Some("")`, and the log line then printed `request=` followed by nothing --
    /// which reads as a measurement rather than as the absence it is. The test above
    /// does not cover it: there the header is missing, not blank.
    #[tokio::test]
    async fn a_header_that_is_present_and_empty_is_absent_too() {
        let served = http_200_with("x-request-id:", VALID_BODY);
        let (server, address, _) = spawn_server(vec![served]);

        let reply = call(address).await.unwrap();

        server.await.unwrap();
        assert_eq!(reply.request_id, None);
    }

    /// The failing side needs it at least as much: a 400 kills the run, and the
    /// status and error text alone do not say which of the provider's records to ask
    /// about.
    #[tokio::test]
    async fn a_refused_request_names_itself_in_the_error() {
        let body = r#"{"error":"bad request"}"#;
        let served = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nx-request-id: req_refused\r\n\
             Connection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        let (server, address, _) = spawn_server(vec![served]);

        let error = call(address).await.unwrap_err();

        server.await.unwrap();
        let rendered = format!("{error:#}");
        assert!(rendered.contains("req_refused"), "got: {rendered}");
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

        merge_extra_body(&mut body, &extra).expect("these keys merge");

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

        merge_extra_body(&mut body, &extra).expect("these keys merge");

        assert_eq!(tool_names(&body), vec!["openrouter:web_search"]);
    }

    /// It used to warn and keep the runtime tools, which reads as "handled" and is
    /// not: the agent declared tools that never reached the request, and only a log
    /// line nobody was watching said so. The same value with no runtime tools to
    /// protect was inserted verbatim, so one malformed declaration had two different
    /// tolerations and no refusal.
    #[test]
    fn a_non_array_extra_body_tools_is_refused_rather_than_absorbed() {
        let mut body = body_with_runtime_tools();
        let extra = HashMap::from([("tools".to_string(), serde_json::json!("web_search"))]);

        let refused = merge_extra_body(&mut body, &extra).expect_err("a string is not a tool list");
        assert!(refused.to_string().contains("not an array"), "{refused}");
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

        merge_extra_body(&mut body, &extra).expect("these keys merge");

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

    /// The cache figure a chat-completions provider sends, carried through as the
    /// part of the prompt it is.
    ///
    /// A run here is overwhelmingly prompt, re-sent in full every round trip, and a
    /// cached prompt token costs a fraction of a fresh one. Without this the bill
    /// cannot be read off the numbers the run records.
    #[test]
    fn a_cached_prompt_is_carried_through_as_part_of_the_prompt() {
        let resp: ChatResponse = serde_json::from_value(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 50,
                "total_tokens": 1050,
                "prompt_tokens_details": {"cached_tokens": 800}
            }
        }))
        .unwrap();

        let usage = chat_response_to_llm(resp, None).usage.expect("usage");
        // 800 of the 1000, not 1800: this dialect counts the cached part inside.
        assert_eq!(usage.prompt_tokens, 1000);
        assert_eq!(usage.cached_prompt_tokens, Some(800));
    }

    /// A provider that says nothing about its cache is reported as saying nothing.
    ///
    /// Zero is a claim that the cache did nothing, which afterwards is
    /// indistinguishable from a provider that never reports -- and the two want
    /// opposite responses, one a prompt to fix, the other nothing at all.
    #[test]
    fn a_provider_that_reports_no_cache_is_not_recorded_as_a_cache_that_missed() {
        let resp: ChatResponse = serde_json::from_value(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1000, "completion_tokens": 50, "total_tokens": 1050}
        }))
        .unwrap();

        let usage = chat_response_to_llm(resp, None).usage.expect("usage");
        assert_eq!(usage.cached_prompt_tokens, None);
    }

    /// A usage object nothing here fully reads keeps what it could not read.
    ///
    /// Without this, a provider that spells the cache something new and a provider
    /// that has no cache are the same silence -- and the answer was being guessed at
    /// by shipping a field name and waiting to see whether a number appeared.
    #[test]
    fn a_usage_field_nothing_reads_is_kept_rather_than_dropped() {
        let resp: ChatResponse = serde_json::from_value(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 50,
                "total_tokens": 1050,
                "prompt_cache_hit_tokens": 768,
                "prompt_cache_miss_tokens": 232
            }
        }))
        .unwrap();

        let usage = resp.usage.as_ref().expect("usage");
        assert_eq!(
            usage.unread.keys().cloned().collect::<Vec<_>>(),
            vec!["prompt_cache_hit_tokens", "prompt_cache_miss_tokens"],
        );
        // Still unknown, because nothing here reads those names -- but now the run
        // says which names it saw instead of leaving it to be guessed.
        assert_eq!(
            chat_response_to_llm(resp, None)
                .usage
                .expect("usage")
                .cached_prompt_tokens,
            None,
        );
    }

    /// Nothing unread is not a complaint. A provider reporting exactly what this
    /// adapter models, and no cache, has nothing to diagnose.
    #[test]
    fn a_provider_that_reports_only_what_is_modelled_leaves_nothing_unread() {
        let resp: ChatResponse = serde_json::from_value(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1000, "completion_tokens": 50, "total_tokens": 1050}
        }))
        .unwrap();

        assert!(resp.usage.as_ref().expect("usage").unread.is_empty());
    }
    /// The mapping is one function because every adapter speaking this dialect must
    /// report the same reply the same way, finish reason included.
    #[test]
    fn a_finish_reason_this_dialect_does_not_define_is_not_taken_for_a_normal_stop() {
        let resp: ChatResponse = serde_json::from_value(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "banana"}],
            "usage": null
        }))
        .unwrap();

        assert_eq!(
            chat_response_to_llm(resp, None).choices[0].finish_reason,
            None
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
    fn a_tool_result_with_a_picture_becomes_three_messages() {
        let out = split_images_out_of_tool_message(tool_with_image());
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["role"], "tool");
        assert_eq!(out[0]["content"], "Here is the screen:");
        assert_eq!(out[0]["tool_call_id"], "call_1");
        // The bridge is what keeps a strict provider from rejecting the
        // sequence with "Unexpected role 'user' after role 'tool'".
        assert_eq!(out[1]["role"], "assistant");
        assert_eq!(out[2]["role"], "user");
        assert_eq!(
            out[2]["content"][0]["image_url"]["url"],
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

#[cfg(test)]
mod user_image_tests {
    use super::mcp_blocks_to_openai;
    use serde_json::json;

    // The other direction a picture arrives from: attached to the issue, not
    // returned by a tool. A user message takes image parts natively, so this is
    // a rewrite rather than the reshuffle a tool result needs.
    #[test]
    fn a_user_message_picture_becomes_an_image_url_part() {
        let out = mcp_blocks_to_openai(json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "look at this"},
                {"type": "image", "data": "AAAA", "mimeType": "image/png"},
            ],
        }));
        assert_eq!(out["content"][0]["type"], "text");
        assert_eq!(out["content"][1]["type"], "image_url");
        assert_eq!(
            out["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
    }

    #[test]
    fn a_string_content_message_is_untouched() {
        let msg = json!({"role": "user", "content": "hello"});
        assert_eq!(mcp_blocks_to_openai(msg.clone()), msg);
    }

    // It runs after the tool split, whose output is already `image_url`.
    #[test]
    fn parts_that_are_already_openai_shaped_are_left_alone() {
        let msg = json!({
            "role": "user",
            "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}],
        });
        assert_eq!(mcp_blocks_to_openai(msg.clone()), msg);
    }
}

#[cfg(test)]
mod reasoning_history_tests {
    use super::*;
    use serde_json::json;

    /// A provider's `reasoning_content` must come back out in the next request.
    ///
    /// This is the opposite of what the countermeasure was first written to do, and
    /// the reversal is the point. DeepSeek's documentation is explicit: when a request
    /// carries `tools` -- which every atoma run does -- "the `reasoning_content` of all
    /// previous turns should be passed back to the API and will be concatenated into
    /// the context", and "if your code does not correctly pass back `reasoning_content`,
    /// the API will return a 400 error".
    ///
    /// So stripping it would not have removed a loop; it would have broken the call.
    /// The field is kept and round-tripped, exactly as the Responses API's `reasoning`
    /// items are.
    #[test]
    fn a_providers_reasoning_content_is_passed_back() {
        let wire = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "The answer is 4.",
                    "reasoning_content": "Let me check the arithmetic.",
                    "tool_calls": null,
                },
                "finish_reason": "stop",
            }],
            "usage": null,
        });

        let response: ChatResponse = serde_json::from_value(wire).expect("a reply parses");
        let message = response.choices.into_iter().next().unwrap().message;

        assert_eq!(message.content.as_ref().unwrap(), "The answer is 4.");
        assert_eq!(
            message.reasoning_content.as_deref(),
            Some("Let me check the arithmetic.")
        );

        // And it goes back out, under the name the provider used.
        let outgoing = message.to_llm_value();
        assert_eq!(
            outgoing["reasoning_content"], "Let me check the arithmetic.",
            "the provider requires this back: {outgoing}"
        );
    }

    /// The same, for a turn that called a tool: the reasoning and the call both go
    /// back, which is the pair DeepSeek's own tool-calling example preserves.
    #[test]
    fn a_tool_calling_turn_keeps_the_call_and_the_reasoning() {
        let wire = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "I should run the tests.",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "shell", "arguments": "{\"cmd\":\"ls\"}"},
                    }],
                },
                "finish_reason": "tool_calls",
            }],
            "usage": null,
        });

        let response: ChatResponse = serde_json::from_value(wire).expect("a reply parses");
        let message = response.choices.into_iter().next().unwrap().message;

        let calls = message.tool_calls.as_ref().expect("the call is kept");
        assert_eq!(calls[0].function.name, "shell");

        let outgoing = message.to_llm_value();
        assert_eq!(outgoing["reasoning_content"], "I should run the tests.");
        assert_eq!(outgoing["tool_calls"][0]["id"], "call_1");
    }

    /// A provider that sends no reasoning must not receive one.
    ///
    /// OpenAI and Anthropic never put a `reasoning_content` on a message, so the field
    /// stays absent and nothing is sent to them that they did not ask for. This is what
    /// makes the field safe to keep unconditionally: it round-trips to exactly the
    /// providers that use it.
    #[test]
    fn a_provider_that_sends_no_reasoning_gets_none_back() {
        let wire = json!({
            "choices": [{
                "message": {"role": "assistant", "content": "hi"},
                "finish_reason": "stop",
            }],
            "usage": null,
        });

        let response: ChatResponse = serde_json::from_value(wire).expect("a reply parses");
        let message = response.choices.into_iter().next().unwrap().message;

        assert!(message.reasoning_content.is_none());
        let outgoing = message.to_llm_value();
        assert!(
            outgoing.get("reasoning_content").is_none(),
            "an absent field must stay absent: {outgoing}"
        );
    }

    /// The Responses API's `reasoning` items are kept for the same reason, and the two
    /// mechanisms are separate: one is a string beside `content`, the other is opaque
    /// items in `provider_items`. Both must survive.
    #[test]
    fn the_responses_apis_reasoning_items_are_kept_too() {
        let message = Message {
            provider_items: Some(vec![json!({"type": "reasoning", "id": "rs_1"})]),
            ..Message::assistant(Some("done"), None)
        };
        let rendered = message.to_llm_value().to_string();
        assert!(
            rendered.contains("rs_1"),
            "the Responses API needs its items back: {rendered}"
        );
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;
    use crate::domain::ports::DeltaHandler;
    use serde_json::json;
    use std::collections::HashMap;

    /// The framing, read the way a provider writes it: `data:` lines, a blank line
    /// between events, and `[DONE]` at the end.
    #[test]
    fn sse_data_reads_the_payload_and_ignores_the_rest() {
        assert_eq!(sse_data("data: {\"a\":1}"), Some("{\"a\":1}"));
        assert_eq!(sse_data("data:{\"a\":1}"), Some("{\"a\":1}"));
        assert_eq!(sse_data("event: message"), None);
        assert_eq!(sse_data("id: 42"), None);
        assert_eq!(sse_data(""), None);
        assert_eq!(sse_data(": a comment"), None);
    }

    /// `[DONE]` is OpenAI's end marker, not JSON. Reporting it as a payload would make
    /// every stream fail on its last line.
    #[test]
    fn the_done_marker_is_not_a_payload() {
        assert_eq!(sse_data("data: [DONE]"), None);
        assert_eq!(sse_data("data:[DONE]"), None);
    }

    /// Text arrives a piece at a time and must be gathered back into one message.
    #[test]
    fn text_deltas_accumulate() {
        let mut acc = StreamAccumulator::default();
        assert_eq!(
            acc.absorb(&json!({"choices":[{"delta":{"content":"Hel"}}]})),
            "Hel"
        );
        assert_eq!(
            acc.absorb(&json!({"choices":[{"delta":{"content":"lo"}}]})),
            "lo"
        );
        let response = acc.into_response(None);
        assert_eq!(
            response.choices[0].message.content.as_ref().unwrap(),
            "Hello"
        );
    }

    /// A tool call's arguments arrive in fragments that must be concatenated by
    /// `index` -- the only field the fragments of one call share.
    #[test]
    fn tool_call_fragments_are_joined_by_index() {
        let mut acc = StreamAccumulator::default();
        acc.absorb(&json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"call_1","function":{"name":"shell","arguments":"{\"cmd\":"}}
        ]}}]}));
        acc.absorb(&json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"function":{"arguments":"\"ls\"}"}}
        ]}}]}));
        let response = acc.into_response(None);
        let calls = response.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "shell");
        assert_eq!(calls[0].function.arguments, "{\"cmd\":\"ls\"}");
    }

    /// Two calls in one response, interleaved, must not be merged into one.
    #[test]
    fn two_tool_calls_stay_apart() {
        let mut acc = StreamAccumulator::default();
        acc.absorb(&json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"a","function":{"name":"one","arguments":"{}"}},
            {"index":1,"id":"b","function":{"name":"two","arguments":"{}"}}
        ]}}]}));
        let response = acc.into_response(None);
        let calls = response.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].function.name, "one");
        assert_eq!(calls[1].function.name, "two");
    }

    /// The finish reason and usage arrive in the last chunks, and both must survive.
    #[test]
    fn the_finish_reason_and_usage_are_kept() {
        let mut acc = StreamAccumulator::default();
        acc.absorb(&json!({"choices":[{"delta":{"content":"hi"}}]}));
        acc.absorb(&json!({"choices":[{"delta":{},"finish_reason":"stop"}]}));
        acc.absorb(&json!({"choices":[],"usage":{
            "prompt_tokens":10,"completion_tokens":2,"total_tokens":12
        }}));
        let response = acc.into_response(Some("req_1".to_string()));
        assert_eq!(response.choices[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(response.usage.unwrap().total_tokens, 12);
        assert_eq!(response.request_id.as_deref(), Some("req_1"));
    }

    /// A chunk with no `choices` at all -- the usage-only final chunk -- must not
    /// panic or clear what has been gathered.
    #[test]
    fn a_chunk_without_choices_is_harmless() {
        let mut acc = StreamAccumulator::default();
        acc.absorb(&json!({"choices":[{"delta":{"content":"kept"}}]}));
        assert_eq!(acc.absorb(&json!({"usage":{"prompt_tokens":1}})), "");
        let response = acc.into_response(None);
        assert_eq!(
            response.choices[0].message.content.as_ref().unwrap(),
            "kept"
        );
    }

    /// A streamed `reasoning_content` is gathered and handed back, the same as a
    /// non-streamed one -- DeepSeek requires it on the next request when `tools` is
    /// present, and a streamed reply is no different in that respect.
    #[test]
    fn streamed_reasoning_is_gathered_and_kept() {
        let mut acc = StreamAccumulator::default();
        acc.absorb(&json!({"choices":[{"delta":{"reasoning_content":"Let me "}}]}));
        acc.absorb(&json!({"choices":[{"delta":{"reasoning_content":"check."}}]}));
        acc.absorb(&json!({"choices":[{"delta":{"content":"The answer is 4."}}]}));

        let response = acc.into_response(None);
        let message = &response.choices[0].message;
        assert_eq!(
            message.reasoning_content.as_deref(),
            Some("Let me check."),
            "the chain of thought is gathered"
        );
        assert_eq!(message.content.as_ref().unwrap(), "The answer is 4.");
    }

    /// The reasoning IS handed to the delta handler.
    ///
    /// A reasoning model that has fallen into a loop does so inside its chain of
    /// thought, and the loop is visible there long before it reaches the answer. The
    /// detector's window is wide enough that ordinary deliberation never approaches
    /// the threshold, so watching the reasoning costs nothing and catches the
    /// pathology where it actually happens.
    #[test]
    fn reasoning_is_offered_as_a_delta() {
        let mut acc = StreamAccumulator::default();
        assert_eq!(
            acc.absorb(&json!({"choices":[{"delta":{"reasoning_content":"thinking"}}]})),
            "thinking",
            "reasoning must reach the repetition detector"
        );
        assert_eq!(
            acc.absorb(&json!({"choices":[{"delta":{"content":"answer"}}]})),
            "answer"
        );
    }

    // ── End to end, over a real socket ────────────────────────────────────────

    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serve one SSE response, and count how many requests arrived.
    ///
    /// The body is written in one go rather than chunk by chunk: what is being tested
    /// is the parsing and the abort, not the framing of TCP, and a single write still
    /// exercises the line-buffering because the reader sees it as one chunk.
    async fn spawn_sse_server(body: String) -> (std::net::SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&requests);

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut request = vec![0; 8192];
                let _ = stream.read(&mut request).await;
                counter.fetch_add(1, Ordering::SeqCst);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body,
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        (address, requests)
    }

    fn sse_body(chunks: &[Value]) -> String {
        let mut body = String::new();
        for chunk in chunks {
            body.push_str("data: ");
            body.push_str(&chunk.to_string());
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    async fn stream_from(
        address: std::net::SocketAddr,
        on_delta: &mut DeltaHandler<'_>,
    ) -> Result<LlmResponse> {
        openai_compat_call_streaming(
            &reqwest::Client::new(),
            &format!("http://{address}"),
            "test-key",
            &[],
            "test-model",
            &[Message::user("hello")],
            None,
            &HashMap::new(),
            on_delta,
        )
        .await
    }

    /// The whole path: a streamed reply arrives, is gathered, and comes back as the
    /// same shape the non-streaming call would have returned.
    #[tokio::test]
    async fn a_streamed_reply_is_gathered_into_one_message() {
        let body = sse_body(&[
            json!({"choices":[{"delta":{"role":"assistant","content":"Hel"}}]}),
            json!({"choices":[{"delta":{"content":"lo"}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}}),
        ]);
        let (address, _) = spawn_sse_server(body).await;

        let mut seen = String::new();
        let mut on_delta = |delta: &str| {
            seen.push_str(delta);
            Ok(())
        };
        let response = stream_from(address, &mut on_delta).await.unwrap();

        assert_eq!(seen, "Hello", "every delta reached the handler");
        assert_eq!(
            response.choices[0].message.content.as_ref().unwrap(),
            "Hello"
        );
        assert_eq!(response.choices[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(response.usage.unwrap().total_tokens, 9);
    }

    /// The acceptance criterion: a handler that refuses a delta stops the request,
    /// and the error it refused with is the one the caller sees -- not a transport
    /// error from the dropped connection.
    #[tokio::test]
    async fn a_refused_delta_aborts_the_request() {
        let body = sse_body(&[
            json!({"choices":[{"delta":{"content":"one "}}]}),
            json!({"choices":[{"delta":{"content":"two "}}]}),
            json!({"choices":[{"delta":{"content":"three "}}]}),
        ]);
        let (address, _) = spawn_sse_server(body).await;

        let mut count = 0;
        let mut on_delta = |_delta: &str| {
            count += 1;
            if count == 2 {
                anyhow::bail!("stop right there");
            }
            Ok(())
        };
        let error = stream_from(address, &mut on_delta).await.unwrap_err();

        assert_eq!(count, 2, "the third delta was never handed over");
        assert!(
            format!("{error:#}").contains("stop right there"),
            "the caller's own error must survive the abort: {error:#}"
        );
    }

    /// A provider error delivered mid-stream is surfaced rather than parsed as a
    /// chunk with no choices.
    #[tokio::test]
    async fn a_mid_stream_provider_error_is_reported() {
        let body = sse_body(&[
            json!({"choices":[{"delta":{"content":"start"}}]}),
            json!({"error":{"message":"upstream overloaded"}}),
        ]);
        let (address, _) = spawn_sse_server(body).await;

        let mut on_delta = |_: &str| Ok(());
        let error = stream_from(address, &mut on_delta).await.unwrap_err();

        assert!(
            format!("{error:#}").contains("upstream overloaded"),
            "got: {error:#}"
        );
    }

    /// A stream that stops arriving is handed back as its own case, carrying what did
    /// arrive, rather than as a failed request.
    ///
    /// This is the whole point of the type. Until it existed, the run ended here: the
    /// request had started, so `send_sse_with_retry` refused to retry, and the error
    /// went all the way up as "Run failed". What the caller needs instead is to know
    /// that the turn is still answerable and what the model already said, so it can ask
    /// for the rest.
    ///
    /// The partial text matters as much as the classification: it was billed for, and
    /// a model asked to continue from it does better than one asked to start over.
    #[tokio::test]
    async fn a_stream_that_stops_arriving_reports_what_it_received() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0; 8192];
            let _ = stream.read(&mut request).await;
            // Content-Length promises more than will ever be sent, and the connection
            // closes at the end. The deltas below are delivered before that, so the
            // accumulator is not empty when the read fails -- which is the case a real
            // dropped connection produces.
            let body = "data: {\"choices\":[{\"delta\":{\"content\":\"half an \"}}]}\n\n\
                        data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\n";
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len() + 100,
            );
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(body.as_bytes()).await;
            let _ = stream.shutdown().await;
        });

        let mut on_delta = |_: &str| Ok(());
        let error = stream_from(address, &mut on_delta)
            .await
            .expect_err("the stream never finished");

        let interrupted = error
            .downcast_ref::<StreamInterrupted>()
            .unwrap_or_else(|| panic!("expected an interrupted stream, got: {error:#}"));
        assert_eq!(
            interrupted.received, "half an answer",
            "what arrived must be carried out, not dropped"
        );
    }

    /// The abort path is not an interruption, and must not be mistaken for one.
    ///
    /// A repetition refusal and a stopped stream both end the request early, so
    /// "the request did not finish" is true of both — and re-asking a *loop* would be
    /// the opposite of what the repetition breaker decided. The two are told apart by
    /// type, which is why the caller looks for this one before it does anything else.
    #[tokio::test]
    async fn a_refused_delta_is_not_reported_as_an_interrupted_stream() {
        let body = sse_body(&[json!({"choices":[{"delta":{"content":"one"}}]})]);
        let (address, _) = spawn_sse_server(body).await;

        let mut on_delta = |_: &str| anyhow::bail!("repetition");
        let error = stream_from(address, &mut on_delta).await.unwrap_err();

        assert!(
            error.downcast_ref::<StreamInterrupted>().is_none(),
            "an abort is a decision, not a dropped connection: {error:#}"
        );
    }

    /// A delta split across two TCP writes must not be parsed as two malformed
    /// payloads. This is the line-buffering, and without it a healthy stream fails.
    #[tokio::test]
    async fn a_payload_split_across_writes_is_reassembled() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0; 8192];
            let _ = stream.read(&mut request).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            // The first half of one `data:` line, then the rest of it.
            let _ = stream
                .write_all(b"data: {\"choices\":[{\"delta\":{\"cont")
                .await;
            let _ = stream
                .write_all(b"ent\":\"split\"}}]}\n\ndata: [DONE]\n\n")
                .await;
            let _ = stream.shutdown().await;
        });

        let mut on_delta = |_: &str| Ok(());
        let response = stream_from(address, &mut on_delta).await.unwrap();
        assert_eq!(
            response.choices[0].message.content.as_ref().unwrap(),
            "split"
        );
    }
}
