use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::domain::tool::{Hooks, ToolDef};
use crate::infra::credentials::{expand_from_environment, Credentials};

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
    /// Seconds one `tools/list` or `tools/call` on this server may take. Absent
    /// means the client's default, which is what nearly every server wants.
    #[serde(default)]
    pub request_timeout_secs: Option<u64>,
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
///
/// shell:
///   command: bun
///   args: ["run", "./scripts/shell.ts"]
///   # A build is not a stall. Only set this for a server whose work genuinely
///   # takes minutes -- it is also the only thing that notices a hung server.
///   request_timeout_secs: 3600
/// ```
pub fn load(path: &Path, credentials: &Credentials) -> Result<HashMap<String, ToolDef>> {
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
                // `${NAME}` here resolves against the ENVIRONMENT, not the
                // credentials. These are program paths: the delivery runner uses
                // one to point a tool server at a checkout of the default branch
                // rather than at the pull request under review, so a path that
                // only resolved when a credential of that name existed would be a
                // trap. `${NAME:-default}` keeps a tools file working where the
                // variable is unset, such as a hand-run `atoma`.
                args: cfg
                    .args
                    .into_iter()
                    .map(|arg| expand_from_environment(&arg))
                    .collect(),
                // `${NAME}` resolved here, against the run's credentials rather
                // than the environment. This is what routes a credential to one
                // server and not the others: a value reaches a tool only by being
                // named in that tool's `env`.
                //
                // Values were literal before this, so a tools file written for an
                // older atoma is unaffected -- there is nothing to expand in it.
                env: cfg
                    .env
                    .into_iter()
                    .map(|(key, value)| (key, credentials.expand(&value)))
                    .collect(),
                hooks,
                // Zero means the default, the same as absent. `infra::timeouts`
                // made that the rule for every timeout read from the environment
                // after three of the four call sites took `0` literally and turned
                // a stall detector into an immediate failure. A tools file is a
                // different source, but the reader is the same person, and a rule
                // that holds in one place and not the other is worse than either.
                request_timeout_secs: cfg.request_timeout_secs.filter(|secs| *secs > 0),
            };
            Ok((name, def))
        })
        .collect()
}

// ── Port adapter ──────────────────────────────────────────────────────────────

/// File-system adapter implementing `ToolDefPort`.
///
/// Holds the run's credentials rather than taking them per call, because the
/// port's `load` is what the runner sees and adding a parameter there would push
/// credentials through every caller that has no business with them.
pub struct FileToolDefAdapter {
    credentials: Credentials,
}

impl FileToolDefAdapter {
    pub fn new(credentials: Credentials) -> Self {
        Self { credentials }
    }
}

impl Default for FileToolDefAdapter {
    /// For `atoma validate` and for tests, which check a tools file's shape and
    /// have no run to draw credentials from. Reading the environment is what
    /// happened before credentials existed, and expanding a reference that is not
    /// set yields empty, which validation does not care about.
    fn default() -> Self {
        Self::new(Credentials::from_environment())
    }
}

impl crate::domain::ports::ToolDefPort for FileToolDefAdapter {
    fn load(
        &self,
        path: &std::path::Path,
    ) -> anyhow::Result<std::collections::HashMap<String, crate::domain::tool::ToolDef>> {
        load(path, &self.credentials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn load_yaml(body: &str) -> HashMap<String, ToolDef> {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(body.as_bytes()).expect("write");
        load(file.path(), &Credentials::from_environment()).expect("load")
    }

    #[test]
    fn a_server_that_says_nothing_gets_the_client_default() {
        let tools = load_yaml("github:\n  command: bun\n  args: [\"run\", \"github.ts\"]\n");
        assert_eq!(tools["github"].request_timeout_secs, None);
    }

    /// The value `shell` needs: `shell_execute` advertises `timeout_seconds` up to
    /// 3600, and every value above the client's 60-second default was unreachable.
    #[test]
    fn a_declared_timeout_is_carried_through() {
        let tools = load_yaml("shell:\n  command: bun\n  args: []\n  request_timeout_secs: 3600\n");
        assert_eq!(tools["shell"].request_timeout_secs, Some(3600));
    }

    /// Same rule as `infra::timeouts`: zero means the default, not "fail every call
    /// immediately". One rule for every timeout in this codebase, because the
    /// person reading them is the same person.
    #[test]
    fn zero_means_the_default_the_same_as_absent() {
        let tools = load_yaml("web:\n  command: bun\n  args: []\n  request_timeout_secs: 0\n");
        assert_eq!(tools["web"].request_timeout_secs, None);
    }
}
