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
    /// Optional now, because a server that is already running has no command.
    /// Which of `command` and `url` are present is checked in [`transport_of`],
    /// where the error can say what the four combinations mean.
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Where the server is, for one that is reached over Streamable HTTP.
    #[serde(default)]
    pub url: Option<String>,
    /// Headers for every request to `url`.
    #[serde(default)]
    pub headers: HashMap<String, String>,
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

/// Which of the four combinations of `command` and `url` this entry is, or an
/// error naming what is missing.
///
/// A function so the reasoning sits next to the rule. Three combinations are
/// meaningful and one is not:
///
///   - `command`          -- a child, over stdio
///   - `url`              -- something already running, over HTTP
///   - `command` + `url`  -- atoma starts it, then speaks HTTP to it
///   - neither            -- nothing to talk to
///
/// The last used to be impossible to write, because `command` was required. Now it
/// is possible and is refused here, before anything starts: a tools file naming a
/// server with no way to reach it would otherwise fail at connection time with an
/// error about an empty program name.
fn transport_of(name: &str, cfg: &ToolConfig) -> Result<()> {
    let has_command = !cfg.command.trim().is_empty();
    let url = cfg.url.as_deref().map(str::trim).unwrap_or("");
    let has_url = !url.is_empty();

    if !has_command && !has_url {
        anyhow::bail!(
            "Tool server '{}' names neither 'command' nor 'url', so there is nothing to \
             connect to. Give it a 'command' to start a server over stdio, a 'url' to \
             reach one that is already running, or both to start one and reach it over HTTP.",
            name,
        );
    }

    if has_url && !url.starts_with("http://") && !url.starts_with("https://") {
        anyhow::bail!(
            "Tool server '{}' has url '{}', which is not an http:// or https:// address. \
             Streamable HTTP is the only transport a url names.",
            name,
            url,
        );
    }

    // Refused rather than ignored, and this is the case worth being strict about.
    // `env` is how a credential reaches a server, and it reaches it by being placed
    // in a child process's environment. There is no child here, so the value goes
    // nowhere -- and a credential someone believes they routed and did not is worse
    // than either a failure or an honest absence. A remote endpoint is
    // authenticated by `headers`.
    if has_url && !has_command && !cfg.env.is_empty() {
        anyhow::bail!(
            "Tool server '{}' declares 'env' but no 'command', so nothing is started and \
             those values reach nothing. A server at a url is authenticated with 'headers'.",
            name,
        );
    }

    // The mirror of the above, for the same reason: a header on a server nobody
    // sends a request to is a token that was never sent.
    if !has_url && !cfg.headers.is_empty() {
        anyhow::bail!(
            "Tool server '{}' declares 'headers' but no 'url'. A server spoken to over stdio \
             has no requests to put them on; a credential reaches it through 'env'.",
            name,
        );
    }

    Ok(())
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
///
/// # Already running somewhere else. No process, so no `env`: the token is a
/// # header, because that is the only thing that reaches an endpoint atoma did
/// # not start.
/// warehouse:
///   url: https://mcp.internal.example.com/mcp
///   headers:
///     Authorization: "Bearer ${WAREHOUSE_TOKEN}"
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
            transport_of(&name, &cfg)?;
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
                // The environment, not the credentials -- a url is an address and
                // the delivery runner already points server paths at a checkout
                // the same way. Same reasoning as `args` above.
                url: cfg
                    .url
                    .map(|url| expand_from_environment(url.trim()))
                    .filter(|url| !url.is_empty()),
                // The credentials, like `env` -- these carry the token. Same
                // routing rule: a value reaches a server only by being named in
                // that server's own block.
                headers: cfg
                    .headers
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

    fn load_err(body: &str) -> String {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(body.as_bytes()).expect("write");
        load(file.path(), &Credentials::from_environment())
            .expect_err("expected this tools file to be refused")
            .to_string()
    }

    #[test]
    fn a_command_alone_is_stdio() {
        let tools = load_yaml("shell:\n  command: bun\n  args: []\n");
        assert_eq!(tools["shell"].url, None);
        assert!(tools["shell"].headers.is_empty());
    }

    #[test]
    fn a_url_alone_needs_no_command() {
        let tools = load_yaml("warehouse:\n  url: https://mcp.example.com/mcp\n");
        assert_eq!(
            tools["warehouse"].url.as_deref(),
            Some("https://mcp.example.com/mcp"),
        );
        assert_eq!(tools["warehouse"].command, "");
    }

    /// Both, which is the arrangement that keeps a server's stderr -- and so its
    /// health reports -- while talking to it over HTTP.
    #[test]
    fn a_command_and_a_url_together_are_allowed() {
        let tools = load_yaml(
            r#"local:
  command: bun
  args: ["run", "s.ts"]
  url: http://127.0.0.1:9000/mcp
"#,
        );
        assert_eq!(tools["local"].command, "bun");
        assert!(tools["local"].url.is_some());
    }

    /// `command` used to be required, so this shape could not be written. It can
    /// now, and connecting would fail with an error about an empty program name.
    #[test]
    fn neither_a_command_nor_a_url_is_refused_at_load() {
        let message = load_err("nowhere:\n  args: []\n");
        assert!(message.contains("neither"), "{message}");
        assert!(message.contains("nowhere"), "{message}");
    }

    #[test]
    fn a_url_that_is_not_http_is_refused() {
        let message = load_err("odd:\n  url: ws://example.com/mcp\n");
        assert!(message.contains("Streamable HTTP"), "{message}");
    }

    /// The case worth being strict about: `env` puts a value in a child's
    /// environment, and there is no child. Ignoring it silently would mean a
    /// credential someone believes they routed and did not.
    #[test]
    fn env_on_a_server_with_no_process_is_refused() {
        let message = load_err(
            r#"remote:
  url: https://x.example/mcp
  env:
    GH_TOKEN: "${GH_TOKEN}"
"#,
        );
        assert!(message.contains("reach nothing"), "{message}");
        assert!(message.contains("headers"), "{message}");
    }

    /// And the mirror: a header on a server nobody sends a request to.
    #[test]
    fn headers_on_a_stdio_server_are_refused() {
        let message = load_err(
            r#"piped:
  command: bun
  headers:
    Authorization: "Bearer x"
"#,
        );
        assert!(message.contains("no 'url'"), "{message}");
        assert!(message.contains("env"), "{message}");
    }

    /// `env` is allowed the moment there is a process to put it in, even when the
    /// conversation happens over HTTP.
    #[test]
    fn env_is_allowed_when_a_command_starts_the_server() {
        let tools = load_yaml(
            r#"local:
  command: bun
  url: http://127.0.0.1:9000/mcp
  env:
    GH_TOKEN: "${GH_TOKEN}"
"#,
        );
        assert!(tools["local"].env.contains_key("GH_TOKEN"));
    }
}
