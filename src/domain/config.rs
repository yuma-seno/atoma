/// Configuration types for orchestration, profiles, and shell guard rules.
///
/// These types are shared across the atoma CLI and the atoma-github binary.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── atoma.toml ───────────────────────────────────────────────────────────────

/// Top-level atoma.toml configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomaConfig {
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Default CLI argument values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default)]
    pub agent_def: Option<String>,
    #[serde(default)]
    pub tools_file: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_output_format")]
    pub output: OutputFormat,
}

fn default_max_iterations() -> u32 {
    50
}

fn default_output_format() -> OutputFormat {
    OutputFormat::Text
}

/// Output format for the `atoma run` command.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    JsonPretty,
}

/// Named profile — overrides defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    #[serde(default)]
    pub agent_def: Option<String>,
    #[serde(default)]
    pub tools_file: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub output: Option<OutputFormat>,
    /// Extra environment variables for this profile.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

// ── Orchestration Config ─────────────────────────────────────────────────────

/// Runtime orchestration configuration (from orchestration.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationConfig {
    #[serde(default)]
    pub version: Option<u64>,
    #[serde(default)]
    pub dispatch: DispatchConfig,
    #[serde(default)]
    pub agents: HashMap<String, AgentContextConfig>,
    #[serde(default)]
    pub scripts: ScriptsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchConfig {
    #[serde(default = "default_workflow")]
    pub workflow: String,
}

fn default_workflow() -> String {
    "atoma-runner.yml".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContextConfig {
    #[serde(default)]
    pub shared_context: SharedContextPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedContextPolicy {
    /// If set, only these event types are included.
    #[serde(default)]
    pub include_event_types: Vec<String>,
    /// If set, these event types are excluded.
    #[serde(default)]
    pub exclude_event_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptsConfig {
    #[serde(default)]
    pub create_pr: ScriptDispatchConfig,
    #[serde(default)]
    pub push_commits: ScriptDispatchConfig,
    #[serde(default)]
    pub create_sub_issue: SubIssueScriptConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptDispatchConfig {
    /// Agent to dispatch after this script completes.
    #[serde(default = "default_dispatch_agent")]
    pub dispatch_agent: String,
}

fn default_dispatch_agent() -> String {
    "reviewer".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubIssueScriptConfig {
    #[serde(default = "default_notify_agent")]
    pub notify_agent: String,
    #[serde(default = "default_trigger_agent")]
    pub trigger_agent: String,
}

fn default_notify_agent() -> String {
    "orchestrator".to_string()
}

fn default_trigger_agent() -> String {
    "engineer".to_string()
}

// ── Shell Guard Config ────────────────────────────────────────────────────────

/// Patterns for blocking dangerous shell commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellGuardConfig {
    /// Substring patterns for commands/pipelines that should be blocked.
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
        BlockedPattern {
            name: "git-network".into(),
            pattern: "git push".into(),
            reason: "Use create_pr or push_commits tools instead of raw git network operations".into(),
        },
        BlockedPattern {
            name: "git-fetch".into(),
            pattern: "git fetch".into(),
            reason: "Use create_pr or push_commits tools instead of raw git network operations".into(),
        },
        BlockedPattern {
            name: "git-pull".into(),
            pattern: "git pull".into(),
            reason: "Use create_pr or push_commits tools instead of raw git network operations".into(),
        },
        BlockedPattern {
            name: "git-clone".into(),
            pattern: "git clone".into(),
            reason: "Use create_pr or push_commits tools instead of raw git network operations".into(),
        },
        BlockedPattern {
            name: "git-remote".into(),
            pattern: "git remote".into(),
            reason: "Use create_pr or push_commits tools instead of raw git network operations".into(),
        },
        BlockedPattern {
            name: "gh-cli".into(),
            pattern: "gh ".into(),
            reason: "Use atoma-github tools instead of gh CLI".into(),
        },
        BlockedPattern {
            name: "curl-wget".into(),
            pattern: "curl ".into(),
            reason: "Network downloads are not permitted".into(),
        },
        BlockedPattern {
            name: "wget".into(),
            pattern: "wget ".into(),
            reason: "Network downloads are not permitted".into(),
        },
        BlockedPattern {
            name: "ssh-scp".into(),
            pattern: "ssh ".into(),
            reason: "Remote access is not permitted".into(),
        },
        BlockedPattern {
            name: "scp".into(),
            pattern: "scp ".into(),
            reason: "Remote access is not permitted".into(),
        },
        BlockedPattern {
            name: "rsync".into(),
            pattern: "rsync ".into(),
            reason: "Remote access is not permitted".into(),
        },
        BlockedPattern {
            name: "eval-exec".into(),
            pattern: "eval ".into(),
            reason: "Dynamic code execution is not permitted".into(),
        },
        BlockedPattern {
            name: "exec".into(),
            pattern: "exec ".into(),
            reason: "Dynamic code execution is not permitted".into(),
        },
        BlockedPattern {
            name: "source-dot".into(),
            pattern: "source ".into(),
            reason: "Dynamic code execution is not permitted".into(),
        },
        BlockedPattern {
            name: "base64-decode".into(),
            pattern: "base64 -d".into(),
            reason: "Base64 decoding from shell is not permitted".into(),
        },
    ]
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

// ── Default implementations ──────────────────────────────────────────────────

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            workflow: default_workflow(),
        }
    }
}

impl Default for ScriptsConfig {
    fn default() -> Self {
        Self {
            create_pr: ScriptDispatchConfig::default(),
            push_commits: ScriptDispatchConfig::default(),
            create_sub_issue: SubIssueScriptConfig::default(),
        }
    }
}

impl Default for ScriptDispatchConfig {
    fn default() -> Self {
        Self {
            dispatch_agent: default_dispatch_agent(),
        }
    }
}

impl Default for SubIssueScriptConfig {
    fn default() -> Self {
        Self {
            notify_agent: default_notify_agent(),
            trigger_agent: default_trigger_agent(),
        }
    }
}

impl Default for ShellGuardConfig {
    fn default() -> Self {
        Self {
            blocked_patterns: default_blocked_patterns(),
        }
    }
}

impl Default for SharedContextPolicy {
    fn default() -> Self {
        Self {
            include_event_types: Vec::new(),
            exclude_event_types: Vec::new(),
        }
    }
}