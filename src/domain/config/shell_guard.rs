//! Shell guard configuration — patterns for blocking dangerous shell commands.

use serde::{Deserialize, Serialize};

/// Patterns for blocking dangerous shell commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellGuardConfig {
    #[serde(default = "default_blocked_patterns")]
    pub blocked_patterns: Vec<BlockedPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedPattern {
    pub name: String,
    pub pattern: String,
    pub reason: String,
}

fn default_blocked_patterns() -> Vec<BlockedPattern> {
    vec![
        blocked("git-push", "git push", "Git network operations should use create_pr/push_commits"),
        blocked("git-fetch", "git fetch", "Git network operations should use create_pr/push_commits"),
        blocked("git-pull", "git pull", "Git network operations should use create_pr/push_commits"),
        blocked("git-clone", "git clone", "Git network operations should use create_pr/push_commits"),
        blocked("git-remote", "git remote", "Git network operations should use create_pr/push_commits"),
        blocked("gh-cli", "gh ", "Use atoma-github tools instead of gh CLI"),
        blocked("curl", "curl ", "Network downloads are not permitted"),
        blocked("wget", "wget ", "Network downloads are not permitted"),
        blocked("ssh", "ssh ", "Remote access is not permitted"),
        blocked("scp", "scp ", "Remote access is not permitted"),
        blocked("rsync", "rsync ", "Remote access is not permitted"),
        blocked("eval", "eval ", "Dynamic code execution is not permitted"),
        blocked("exec", "exec ", "Dynamic code execution is not permitted"),
        blocked("source", "source ", "Dynamic code execution is not permitted"),
        blocked("base64-decode", "base64 -d", "Base64 decoding from shell is not permitted"),
    ]
}

fn blocked(name: &str, pattern: &str, reason: &str) -> BlockedPattern {
    BlockedPattern {
        name: name.to_string(),
        pattern: pattern.to_string(),
        reason: reason.to_string(),
    }
}

impl ShellGuardConfig {
    /// Check whether `command` is blocked. Returns `Some(reason)` if blocked.
    pub fn check(&self, command: &str) -> Option<&str> {
        let cmd_lower = command.to_lowercase();
        for pattern in &self.blocked_patterns {
            if cmd_lower.contains(&pattern.pattern.to_lowercase()) {
                return Some(&pattern.reason);
            }
        }
        None
    }
}

impl Default for ShellGuardConfig {
    fn default() -> Self {
        Self {
            blocked_patterns: default_blocked_patterns(),
        }
    }
}