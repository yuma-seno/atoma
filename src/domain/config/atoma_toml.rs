//! atoma.toml configuration types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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