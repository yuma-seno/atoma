/// Load and resolve Atoma configuration from atoma.toml and CLI args.
///
/// Run setting priority (highest to lowest):
///   1. CLI argument
///   2. atoma.toml profile value
///   3. atoma.toml defaults value
///   4. Hard-coded default
///
/// Environment variables use process environment, profile, then global config priority.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level atoma.toml configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomaConfig {
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default, rename = "profile")]
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
    #[serde(default)]
    pub output: OutputFormat,
}

fn default_max_iterations() -> u32 {
    50
}

/// Output format for the `atoma run` command.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

/// Named profile overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Resolved configuration for a single `atoma run` invocation.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub agent_def: PathBuf,
    pub tools_file: Option<PathBuf>,
    pub template: Option<PathBuf>,
    pub max_iterations: u32,
    pub output: OutputFormat,
    pub env: HashMap<String, String>,
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
    pub template: Option<PathBuf>,
    pub max_iterations: Option<u32>,
    pub output: Option<OutputFormat>,
}

pub fn resolve_run_config(
    overrides: CliOverrides,
    profile_name: Option<&str>,
    config: Option<&AtomaConfig>,
) -> Result<ResolvedConfig> {
    let (defaults, profile) =
        match config {
            Some(cfg) => {
                let prof =
                    match profile_name {
                        Some(name) => Some(cfg.profiles.get(name).cloned().with_context(|| {
                            format!("Profile '{}' not found in atoma.toml", name)
                        })?),
                        None => None,
                    };
                (Some(cfg.defaults.clone()), prof)
            }
            None if profile_name.is_some() => {
                anyhow::bail!("--profile requires an atoma.toml configuration file")
            }
            None => (None, None),
        };

    let agent_def = overrides
        .agent_def
        .clone()
        .or_else(|| {
            profile
                .as_ref()
                .and_then(|p| p.agent_def.clone())
                .map(PathBuf::from)
        })
        .or_else(|| {
            defaults
                .as_ref()
                .and_then(|d| d.agent_def.clone())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("agent.md"));

    let tools_file = overrides
        .tools_file
        .clone()
        .or_else(|| {
            profile
                .as_ref()
                .and_then(|p| p.tools_file.clone())
                .map(PathBuf::from)
        })
        .or_else(|| {
            defaults
                .as_ref()
                .and_then(|d| d.tools_file.clone())
                .map(PathBuf::from)
        });

    let template = overrides
        .template
        .or_else(|| {
            profile
                .as_ref()
                .and_then(|p| p.template.clone())
                .map(PathBuf::from)
        })
        .or_else(|| {
            defaults
                .as_ref()
                .and_then(|d| d.template.clone())
                .map(PathBuf::from)
        });

    let max_iterations = overrides
        .max_iterations
        .or_else(|| profile.as_ref().and_then(|p| p.max_iterations))
        .or_else(|| defaults.as_ref().map(|d| d.max_iterations))
        .unwrap_or(50);

    let output = overrides
        .output
        .or_else(|| profile.as_ref().and_then(|p| p.output.clone()))
        .or_else(|| defaults.as_ref().map(|d| d.output.clone()))
        .unwrap_or(OutputFormat::Text);

    let mut env = config.map(|cfg| cfg.env.clone()).unwrap_or_default();
    if let Some(profile) = &profile {
        env.extend(profile.env.clone());
    }

    Ok(ResolvedConfig {
        agent_def,
        tools_file,
        template,
        max_iterations,
        output,
        env,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_template_using_cli_profile_defaults_priority() {
        let config: AtomaConfig = toml::from_str(
            r#"
[defaults]
template = "default.md"

[profile.review]
template = "review.md"
output = "json"
"#,
        )
        .unwrap();

        let from_profile = resolve_run_config(
            CliOverrides {
                agent_def: None,
                tools_file: None,
                template: None,
                max_iterations: None,
                output: None,
            },
            Some("review"),
            Some(&config),
        )
        .unwrap();
        assert_eq!(from_profile.template, Some(PathBuf::from("review.md")));
        assert_eq!(from_profile.output, OutputFormat::Json);

        let from_cli = resolve_run_config(
            CliOverrides {
                agent_def: None,
                tools_file: None,
                template: Some(PathBuf::from("cli.md")),
                max_iterations: None,
                output: None,
            },
            Some("review"),
            Some(&config),
        )
        .unwrap();
        assert_eq!(from_cli.template, Some(PathBuf::from("cli.md")));
    }

    #[test]
    fn profile_environment_overrides_global_environment() {
        let config: AtomaConfig = toml::from_str(
            r#"
[env]
SHARED = "global"
GLOBAL_ONLY = "yes"

[profile.review.env]
SHARED = "profile"
PROFILE_ONLY = "yes"
"#,
        )
        .unwrap();

        let resolved = resolve_run_config(
            CliOverrides {
                agent_def: None,
                tools_file: None,
                template: None,
                max_iterations: None,
                output: None,
            },
            Some("review"),
            Some(&config),
        )
        .unwrap();

        assert_eq!(
            resolved.env.get("SHARED").map(String::as_str),
            Some("profile")
        );
        assert_eq!(
            resolved.env.get("GLOBAL_ONLY").map(String::as_str),
            Some("yes")
        );
        assert_eq!(
            resolved.env.get("PROFILE_ONLY").map(String::as_str),
            Some("yes")
        );
    }

    #[test]
    fn rejects_unknown_profile() {
        let config: AtomaConfig = toml::from_str("[profile.review]\nmax_iterations = 10").unwrap();
        let result = resolve_run_config(
            CliOverrides {
                agent_def: None,
                tools_file: None,
                template: None,
                max_iterations: None,
                output: None,
            },
            Some("typo"),
            Some(&config),
        );

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Profile 'typo' not found"));
    }
}
