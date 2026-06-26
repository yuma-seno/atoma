//! Orchestration configuration types (from orchestration.json).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    #[serde(default)]
    pub include_event_types: Vec<String>,
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

impl Default for SharedContextPolicy {
    fn default() -> Self {
        Self {
            include_event_types: Vec::new(),
            exclude_event_types: Vec::new(),
        }
    }
}