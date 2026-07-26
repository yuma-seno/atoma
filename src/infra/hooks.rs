use anyhow::{Context, Result};
use serde_json::Value;
use std::time::Duration;

use crate::domain::tool::Hooks;

fn hook_timeout() -> Duration {
    std::env::var("ATOMA_HOOK_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(30))
}

/// Returns `true` if `pattern` matches `value`.
///
/// Supports a single trailing `*` wildcard (e.g. `"filesystem__*"`).
pub fn glob_matches(pattern: &str, value: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

/// Validate whether `tool_name` is permitted by the hook configuration.
///
/// Order of checks:
/// 1. Denylist — any match → blocked immediately.
/// 2. Allowlist — if non-empty and no match → blocked.
pub fn check_access(hooks: &Hooks, tool_name: &str) -> Result<()> {
    if let Some(message) = access_denial_reason(hooks, tool_name) {
        tracing::warn!("{}", message);
        anyhow::bail!(message);
    }

    Ok(())
}

/// Return the static access-control reason for hiding or rejecting a tool.
/// Dynamic `before_tool` hooks are intentionally not evaluated here.
pub fn access_denial_reason(hooks: &Hooks, tool_name: &str) -> Option<String> {
    if let Some(pattern) = hooks
        .tool_denylist
        .iter()
        .find(|p| glob_matches(p, tool_name))
    {
        return Some(format!(
            "Tool '{}' is blocked by denylist pattern '{}'",
            tool_name, pattern
        ));
    }

    if !hooks.tool_allowlist.is_empty()
        && !hooks
            .tool_allowlist
            .iter()
            .any(|p| glob_matches(p, tool_name))
    {
        return Some(format!(
            "Tool '{}' is not permitted by the allowlist",
            tool_name
        ));
    }

    None
}

/// Validate hook configuration. Returns an error if both allowlist and denylist
/// are non-empty, as the interaction between the two is ambiguous.
/// Should be called during tool registration, not at call time.
pub fn validate_hooks(hooks: &Hooks) -> Result<()> {
    if !hooks.tool_allowlist.is_empty() && !hooks.tool_denylist.is_empty() {
        anyhow::bail!(
            "Ambiguous hook configuration: both tool_allowlist and tool_denylist are set. \
             Use one or the other, not both."
        );
    }
    Ok(())
}

/// Invoke the `before_tool` hook script.
///
/// The script receives JSON on stdin and must respond with
/// `{"allow": true}` or `{"allow": false, "reason": "..."}`.
/// Non-zero exit or invalid JSON is treated as a deny (fail-closed).
pub async fn run_before_hook(script: &str, payload: Value) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let input = serde_json::to_vec(&payload)?;

    let mut child = tokio::process::Command::new(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to spawn before_tool hook: {}", script))?;

    let mut stdin = child
        .stdin
        .take()
        .context("Failed to get before_tool hook stdin")?;
    stdin.write_all(&input).await?;
    drop(stdin);

    let timeout = hook_timeout();
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .with_context(|| {
            format!(
                "before_tool hook '{}' timed out after {}s",
                script,
                timeout.as_secs()
            )
        })?
        .with_context(|| format!("Failed to wait for before_tool hook: {}", script))?;

    if !output.status.success() {
        let msg = format!(
            "before_tool hook '{}' exited with status {}",
            script, output.status
        );
        tracing::warn!("{}", msg);
        anyhow::bail!("{}", msg);
    }

    let response: Value = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("before_tool hook '{}' returned invalid JSON", script))?;

    let allowed = response
        .get("allow")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !allowed {
        let reason = response
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("hook denied the request");
        let msg = format!("Tool blocked by hook '{}': {}", script, reason);
        tracing::warn!("{}", msg);
        anyhow::bail!("{}", msg);
    }

    tracing::debug!("before_tool hook '{}' allowed the call", script);
    Ok(())
}

/// Invoke the `after_tool` hook (best-effort; failures are logged, not propagated).
pub async fn run_after_hook(script: &str, payload: Value) {
    if let Err(e) = run_after_hook_inner(script, payload).await {
        tracing::warn!("after_tool hook '{}' error: {}", script, e);
    }
}

async fn run_after_hook_inner(script: &str, payload: Value) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let input = serde_json::to_vec(&payload)?;

    let mut child = tokio::process::Command::new(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to spawn after_tool hook: {}", script))?;

    let mut stdin = child
        .stdin
        .take()
        .context("Failed to get after_tool hook stdin")?;
    stdin.write_all(&input).await?;
    drop(stdin);

    let timeout = hook_timeout();
    let status = tokio::time::timeout(timeout, child.wait())
        .await
        .with_context(|| {
            format!(
                "after_tool hook '{}' timed out after {}s",
                script,
                timeout.as_secs()
            )
        })?
        .with_context(|| format!("Failed to wait for after_tool hook: {}", script))?;

    if !status.success() {
        tracing::warn!("after_tool hook '{}' exited with status {}", script, status);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tool::Hooks;

    #[test]
    fn test_glob_matches_exact() {
        assert!(glob_matches("read_file", "read_file"));
    }

    #[test]
    fn test_glob_matches_wildcard_suffix() {
        assert!(glob_matches("read_*", "read_file"));
        assert!(glob_matches("read_*", "read_anything"));
        assert!(!glob_matches("read_*", "write_file"));
    }

    #[test]
    fn test_glob_matches_wildcard_all() {
        assert!(glob_matches("*", "any_tool"));
    }

    #[test]
    fn test_glob_no_match() {
        assert!(!glob_matches("write_file", "read_file"));
    }

    #[test]
    fn test_check_access_empty_hooks_allows_all() {
        let hooks = Hooks {
            tool_allowlist: vec![],
            tool_denylist: vec![],
            before_tool: None,
            after_tool: None,
        };
        assert!(check_access(&hooks, "any_tool").is_ok());
    }

    #[test]
    fn test_check_access_denylist_blocks() {
        let hooks = Hooks {
            tool_allowlist: vec![],
            tool_denylist: vec!["dangerous_*".to_string()],
            before_tool: None,
            after_tool: None,
        };
        assert!(check_access(&hooks, "dangerous_rm").is_err());
        assert!(check_access(&hooks, "safe_read").is_ok());
    }

    #[test]
    fn test_check_access_allowlist_gates() {
        let hooks = Hooks {
            tool_allowlist: vec!["read_*".to_string()],
            tool_denylist: vec![],
            before_tool: None,
            after_tool: None,
        };
        assert!(check_access(&hooks, "read_file").is_ok());
        assert!(check_access(&hooks, "write_file").is_err());
    }

    #[test]
    fn test_check_access_denylist_takes_precedence_over_allowlist() {
        let hooks = Hooks {
            tool_allowlist: vec!["*".to_string()],
            tool_denylist: vec!["rm_*".to_string()],
            before_tool: None,
            after_tool: None,
        };
        assert!(check_access(&hooks, "rm_all").is_err());
        assert!(check_access(&hooks, "read_file").is_ok());
    }
}
