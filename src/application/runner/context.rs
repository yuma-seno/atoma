//! Context session helpers — transient context injection and filtering.

use crate::domain::session::{Message, Session};
use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::domain::ports::SessionPort;

const TRANSIENT_CONTEXT_FLAG: &str = "transient_context";
const TRANSIENT_CONTEXT_LAYER: &str = "context-session";

pub fn load_transient_context_messages(
    context_sessions: &[PathBuf],
    session_port: &dyn SessionPort,
) -> Result<Vec<Message>> {
    let mut messages = Vec::new();

    for path in context_sessions {
        let context_session = session_port
            .load(path)
            .with_context(|| format!("Failed to load context session: {:?}", path))?;

        let mut loaded: Vec<Message> = context_session
            .messages
            .into_iter()
            .filter(|message| message.role != "system")
            .map(mark_transient_context_message)
            .collect();

        tracing::info!(
            "Loaded {} transient context message(s) from {:?}",
            loaded.len(),
            path
        );

        messages.append(&mut loaded);
    }

    Ok(messages)
}

fn mark_transient_context_message(mut message: Message) -> Message {
    let mut metadata = match message.atoma_metadata.take() {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };

    metadata.insert(
        TRANSIENT_CONTEXT_FLAG.to_string(),
        serde_json::Value::Bool(true),
    );
    metadata
        .entry("layer".to_string())
        .or_insert_with(|| serde_json::Value::String(TRANSIENT_CONTEXT_LAYER.to_string()));

    message.atoma_metadata = Some(serde_json::Value::Object(metadata));
    message
}

pub fn is_transient_context_message(message: &Message) -> bool {
    message
        .atoma_metadata
        .as_ref()
        .and_then(|value| value.get(TRANSIENT_CONTEXT_FLAG))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

pub fn session_for_persistence(session: &Session) -> Session {
    let mut persisted = session.clone();
    persisted
        .messages
        .retain(|message| !is_transient_context_message(message));
    persisted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session::Message;

    #[test]
    fn test_mark_transient_context_message_preserves_metadata() {
        let message = Message::user_with_metadata("ctx", serde_json::json!({ "id": 1 }));
        let marked = mark_transient_context_message(message);
        let metadata = marked
            .atoma_metadata
            .as_ref()
            .and_then(|value| value.as_object())
            .unwrap();

        assert_eq!(metadata.get("id").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(
            metadata
                .get(TRANSIENT_CONTEXT_FLAG)
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            metadata.get("layer").and_then(|v| v.as_str()),
            Some(TRANSIENT_CONTEXT_LAYER)
        );
    }

    #[test]
    fn test_session_for_persistence_removes_transient_context_messages() {
        let mut session = Session::default();
        session.messages.push(Message::system("sys"));
        session.messages.push(Message::user("persistent"));
        session
            .messages
            .push(mark_transient_context_message(Message::user("transient")));

        let cleaned = session_for_persistence(&session);
        assert_eq!(cleaned.messages.len(), 2);
        assert_eq!(
            cleaned.messages[0]
                .content
                .as_ref()
                .and_then(|c| c.as_str()),
            Some("sys")
        );
        assert_eq!(
            cleaned.messages[1]
                .content
                .as_ref()
                .and_then(|c| c.as_str()),
            Some("persistent")
        );
    }
}
