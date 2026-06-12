use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::domain::tool::{Hooks, ToolDef};

/// YAML deserialization view — private to this module.
#[derive(Deserialize)]
struct ToolConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Deserialize, Default)]
struct HooksConfig {
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub tool_denylist: Vec<String>,
    #[serde(default)]
    pub before_tool: Option<String>,
    #[serde(default)]
    pub after_tool: Option<String>,
}

/// Load a tools YAML file and return a map of server-name → `ToolDef`.
///
/// Hook script paths are resolved relative to the directory of the YAML file,
/// so `./scripts/guard.py` in `./tools/tools.yaml` resolves to
/// `./tools/scripts/guard.py` regardless of the working directory.
///
/// # Example YAML
/// ```yaml
/// filesystem:
///   command: npx
///   args: ["-y", "@modelcontextprotocol/server-filesystem", "."]
///   hooks:
///     tool_allowlist: ["filesystem__*"]
///     before_tool: ./scripts/fs_guard.py
/// ```
pub fn load(path: &Path) -> Result<HashMap<String, ToolDef>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read tools file: {:?}", path))?;
    let configs: HashMap<String, ToolConfig> = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse tools YAML: {:?}", path))?;

    let base_dir = path.parent().unwrap_or(Path::new("."));

    let resolve = |s: Option<String>| -> Result<Option<String>> {
        let Some(script) = s else { return Ok(None) };
        let p = Path::new(&script);
        let resolved = if p.is_absolute() {
            script.clone()
        } else {
            base_dir.join(p).to_string_lossy().into_owned()
        };
        if !Path::new(&resolved).exists() {
            anyhow::bail!(
                "Hook script not found: '{}' (resolved from '{}')",
                resolved,
                script
            );
        }
        Ok(Some(resolved))
    };

    configs
        .into_iter()
        .map(|(name, cfg)| {
            let hooks = Hooks {
                tool_allowlist: cfg.hooks.tool_allowlist,
                tool_denylist: cfg.hooks.tool_denylist,
                before_tool: resolve(cfg.hooks.before_tool)?,
                after_tool: resolve(cfg.hooks.after_tool)?,
            };
            let def = ToolDef {
                name: name.clone(),
                command: cfg.command,
                args: cfg.args,
                env: cfg.env,
                hooks,
            };
            Ok((name, def))
        })
        .collect()
}

// ── Port adapter ──────────────────────────────────────────────────────────────

/// File-system adapter implementing `ToolDefPort`.
pub struct FileToolDefAdapter;

impl crate::domain::ports::ToolDefPort for FileToolDefAdapter {
    fn load(
        &self,
        path: &std::path::Path,
    ) -> anyhow::Result<std::collections::HashMap<String, crate::domain::tool::ToolDef>> {
        load(path)
    }
}
