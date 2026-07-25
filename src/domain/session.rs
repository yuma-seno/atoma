use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolCallFunction,
}

/// A single message in a conversation.
///
/// `atoma_metadata` is an internal field stripped before sending to the LLM API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Atoma-internal metadata (e.g. GitHub comment ID).
    /// Stored in session.json but stripped before sending to the LLM API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atoma_metadata: Option<Value>,
}

impl Message {
    pub fn system(content: &str) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(Value::String(content.to_string())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            atoma_metadata: None,
        }
    }

    pub fn user(content: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(Value::String(content.to_string())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            atoma_metadata: None,
        }
    }

    pub fn user_with_metadata(content: &str, metadata: Value) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(Value::String(content.to_string())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            atoma_metadata: Some(metadata),
        }
    }

    pub fn assistant(content: Option<&str>, tool_calls: Option<Vec<ToolCall>>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.map(|c| Value::String(c.to_string())),
            tool_calls,
            tool_call_id: None,
            name: None,
            atoma_metadata: None,
        }
    }

    pub fn tool(tool_call_id: &str, content: &str) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(Value::String(content.to_string())),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
            name: None,
            atoma_metadata: None,
        }
    }

    /// Metadata key set on messages loaded from a `--context-session` file
    /// (see `application::runner::context`): these are appended to the
    /// conversation for exactly one inference call and stripped again
    /// before the session is persisted. Exposed here (not just in the
    /// `runner` layer) so lower layers -- e.g. `infra::llm::anthropic`'s
    /// prompt-cache breakpoint placement -- can recognize the same
    /// boundary without duplicating the key string.
    pub const TRANSIENT_CONTEXT_FLAG: &'static str = "transient_context";

    /// True if this message was injected from a `--context-session` file
    /// for the current run only (see `TRANSIENT_CONTEXT_FLAG`).
    pub fn is_transient_context(&self) -> bool {
        self.atoma_metadata
            .as_ref()
            .and_then(|v| v.get(Self::TRANSIENT_CONTEXT_FLAG))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Return a `serde_json::Value` with `atoma_metadata` stripped, suitable
    /// for sending to the LLM API.
    pub fn to_llm_value(&self) -> Value {
        let mut val = serde_json::to_value(self).expect("Message serialization is infallible");
        if let Some(obj) = val.as_object_mut() {
            obj.remove("atoma_metadata");
        }
        val
    }
}

/// An in-memory conversation session.
///
/// Persistence (loading from / saving to disk) is handled by
/// `crate::infra::persistence::session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_system() {
        let m = Message::system("You are helpful.");
        assert_eq!(m.role, "system");
        assert_eq!(m.content, Some(Value::String("You are helpful.".into())));
        assert!(m.tool_calls.is_none());
        assert!(m.atoma_metadata.is_none());
    }

    #[test]
    fn test_message_user() {
        let m = Message::user("Hello!");
        assert_eq!(m.role, "user");
        assert_eq!(m.content, Some(Value::String("Hello!".into())));
    }

    #[test]
    fn test_message_user_with_metadata() {
        let meta = serde_json::json!({ "comment_id": 42 });
        let m = Message::user_with_metadata("Hello!", meta.clone());
        assert_eq!(m.role, "user");
        assert_eq!(m.atoma_metadata, Some(meta));
    }

    #[test]
    fn test_message_assistant_text() {
        let m = Message::assistant(Some("I'm here."), None);
        assert_eq!(m.role, "assistant");
        assert_eq!(m.content, Some(Value::String("I'm here.".into())));
        assert!(m.tool_calls.is_none());
    }

    #[test]
    fn test_message_tool() {
        let m = Message::tool("call-1", "result_value");
        assert_eq!(m.role, "tool");
        assert_eq!(m.tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(m.content, Some(Value::String("result_value".into())));
    }

    #[test]
    fn test_to_llm_value_strips_metadata() {
        let meta = serde_json::json!({ "comment_id": 99 });
        let m = Message::user_with_metadata("Hi", meta);
        let val = m.to_llm_value();
        let obj = val.as_object().unwrap();
        assert!(
            !obj.contains_key("atoma_metadata"),
            "atoma_metadata should be stripped"
        );
        assert_eq!(obj["role"].as_str(), Some("user"));
    }

    #[test]
    fn test_to_llm_value_preserves_content() {
        let m = Message::system("sys");
        let val = m.to_llm_value();
        assert_eq!(val["content"].as_str(), Some("sys"));
    }
}
