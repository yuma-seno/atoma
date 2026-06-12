use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::domain::session::Session;

/// Load a session from a JSON file.
///
/// Returns an empty session if the file does not exist yet.
pub fn load(path: &Path) -> Result<Session> {
    if path.exists() {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read session file: {:?}", path))?;
        let session: Session = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse session JSON: {:?}", path))?;
        Ok(session)
    } else {
        Ok(Session::default())
    }
}

/// Persist a session to a JSON file atomically (write-then-rename).
pub fn save(session: &Session, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {:?}", parent))?;
    }
    let content = serde_json::to_string_pretty(session).context("Failed to serialize session")?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &content).with_context(|| format!("Failed to write session to: {:?}", tmp))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("Failed to atomically rename session file: {:?}", path))?;
    Ok(())
}

// ── Port adapter ──────────────────────────────────────────────────────────────

/// File-system adapter implementing `SessionPort`.
pub struct FileSessionAdapter;

impl crate::domain::ports::SessionPort for FileSessionAdapter {
    fn load(&self, path: &Path) -> Result<crate::domain::session::Session> {
        load(path)
    }

    fn save(&self, session: &crate::domain::session::Session, path: &Path) -> Result<()> {
        save(session, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session::{Message, Session};

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let session = load(&path).unwrap();
        assert!(session.messages.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");

        let mut session = Session::default();
        session.messages.push(Message::user("Hello"));
        session
            .messages
            .push(Message::assistant(Some("Hi there"), None));

        save(&session, &path).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].role, "user");
        assert_eq!(loaded.messages[1].role, "assistant");
    }

    #[test]
    fn test_save_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/dir/session.json");

        let session = Session::default();
        save(&session, &path).unwrap();
        assert!(path.exists());
    }
}
