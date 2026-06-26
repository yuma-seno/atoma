/// Load and resolve Atoma configuration from atoma.toml, environment variables, and CLI args.
///
/// Priority (highest to lowest):
///   1. CLI argument
///   2. Environment variable
///   3. atoma.toml profile value
///   4. atoma.toml defaults value
///   5. Hard-coded default

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::domain::config::{AtomaConfig, OutputFormat};

/// Resolved configuration for a single `atoma run` invocation.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub agent_def: PathBuf,
    pub tools_file: Option<PathBuf>,
    pub max_iterations: u32,
    pub output: OutputFormat,
}

/// Attempt to discover and load atoma.toml from the current directory or ancestors.
pub fn discover_and_load() -> Result<(Option<PathBuf>, Option<AtomaConfig>)> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    for dir in cwd.ancestors() {
        let candidate = dir.join("atoma.toml");
        if candidate.exists() {
            let content = std::fs::read_to_string(&candidate)
                .with_context(|| format!("Failed to read config: {:?}", candidate))?;
            let config: AtomaConfig = toml::from_str(&content)
                .with_context(|| format!("Failed to parse config: {:?}", candidate))?;
            tracing::info!("Loaded config from {:?}", candidate);
            return Ok((Some(candidate), Some(config)));
        }
    }
    Ok((None, None))
}

/// Resolve CLI arguments against config file.
pub struct CliOverrides {
    pub agent_def: Option<PathBuf>,
    pub tools_file: Option<PathBuf>,
    pub max_iterations: Option<u32>,
    pub output: Option<OutputFormat>,
}

pub fn resolve_run_config(
    overrides: CliOverrides,
    profile_name: Option<&str>,
    config: Option<&AtomaConfig>,
) -> Result<ResolvedConfig> {
    let (defaults, profile) = match config {
        Some(cfg) => {
            let prof = profile_name
                .and_then(|name| cfg.profiles.get(name))
                .cloned();
            (Some(cfg.defaults.clone()), prof)
        }
        None => (None, None),
    };

    let agent_def = overrides
        .agent_def
        .clone()
        .or_else(|| profile.as_ref().and_then(|p| p.agent_def.clone()).map(PathBuf::from))
        .or_else(|| defaults.as_ref().and_then(|d| d.agent_def.clone()).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("agent.md"));

    let tools_file = overrides
        .tools_file
        .clone()
        .or_else(|| profile.as_ref().and_then(|p| p.tools_file.clone()).map(PathBuf::from))
        .or_else(|| defaults.as_ref().and_then(|d| d.tools_file.clone()).map(PathBuf::from));

    let max_iterations = overrides
        .max_iterations
        .or_else(|| profile.as_ref().and_then(|p| p.max_iterations))
        .or_else(|| defaults.as_ref().map(|d| d.max_iterations))
        .unwrap_or(50);

    let output = overrides
        .output
        .or_else(|| defaults.as_ref().map(|d| d.output.clone()))
        .unwrap_or(OutputFormat::Text);

    Ok(ResolvedConfig {
        agent_def,
        tools_file,
        max_iterations,
        output,
    })
}

/// Generate a default atoma.toml file.
pub fn generate_default_config() -> String {
    r#"# Atoma configuration
# See https://github.com/yuma-seno/atoma for documentation.

[defaults]
# agent_def = "agents/default.md"
# tools_file = "tools.yaml"
# template = "templates/custom.md"
max_iterations = 50
# output = "text"   # "text" or "json"

# Profile: overrides defaults when --profile is used
# [profile.review]
# agent_def = "agents/reviewer.md"
# output = "json"

# Environment variables to set when running under this config
# [env]
# OPENAI_BASE_URL = "https://openrouter.ai/api/v1"
"#
    .to_string()
}