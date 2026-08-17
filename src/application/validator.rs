use anyhow::{bail, Result};
use std::path::PathBuf;

use crate::domain::ports::{AgentDefPort, ToolDefPort};

const VALID_CALLABLE_BY: &[&str] = &["user", "agent"];

/// Validate an agent definition file and optional tools file.
///
/// Checks:
///   1. Agent definition parses without error (YAML, required fields).
///   2. Each `knows_about` entry has a corresponding `<name>.md` in the same directory.
///   3. Each `knows_about` target's own `callable_by` includes `"agent"` — otherwise
///      it does not accept agent-to-agent delegation and the reference is dead.
///   4. `callable_by` only contains recognized values (`"user"`, `"agent"`).
///   5. `extra_body` does not override the reserved keys `model` or `messages`.
///   6. If a tools file is provided:
///      a. The tools file parses without error.
///      b. Each `mcp_servers` entry is present in the tools file.
pub fn validate(
    agent_def_path: PathBuf,
    tools_file: Option<PathBuf>,
    agent_def_port: &dyn AgentDefPort,
    tool_def_port: &dyn ToolDefPort,
) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    let parsed_agent = match agent_def_port.parse(&agent_def_path) {
        Ok(a) => {
            println!("✓ Agent definition parsed: {}", a.frontmatter.name);
            Some(a)
        }
        Err(e) => {
            errors.push(format!("Agent definition parse error: {}", e));
            None
        }
    };

    if let Some(ref parsed) = parsed_agent {
        let agent = &parsed.frontmatter;
        let agent_def_dir = agent_def_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));

        for c in &agent.callable_by {
            if !VALID_CALLABLE_BY.contains(&c.as_str()) {
                errors.push(format!(
                    "callable_by contains unknown value '{}' (expected one of: {})",
                    c,
                    VALID_CALLABLE_BY.join(", ")
                ));
            }
        }

        for name in &agent.knows_about {
            let candidate = agent_def_dir.join(format!("{}.md", name));
            if candidate.exists() {
                println!("  ✓ knows_about '{}' → {:?}", name, candidate);
                match agent_def_port.parse(&candidate) {
                    Ok(target) => {
                        if !target.frontmatter.callable_by.iter().any(|c| c == "agent") {
                            errors.push(format!(
                                "knows_about '{}': target agent's callable_by does not include \"agent\", so delegation from '{}' is not a supported invocation path",
                                name, agent.name
                            ));
                        }
                    }
                    Err(e) => {
                        errors.push(format!(
                            "knows_about '{}': failed to parse target definition: {}",
                            name, e
                        ));
                    }
                }
            } else {
                errors.push(format!(
                    "knows_about '{}': definition file not found at {:?}",
                    name, candidate
                ));
            }
        }

        const RESERVED: &[&str] = &["model", "messages"];
        for key in RESERVED {
            if agent.extra_body.contains_key(*key) {
                errors.push(format!(
                    "extra_body contains reserved key '{}' which would be silently ignored",
                    key
                ));
            }
        }

        // `extra_body.tools` is appended to the runtime tool definitions rather
        // than replacing them, which is only possible for an array.
        if let Some(tools) = agent.extra_body.get("tools") {
            if !tools.is_array() {
                errors.push(
                    "extra_body 'tools' must be an array so it can be merged with the \
                     runtime tool definitions"
                        .to_string(),
                );
            }
        }

        if let Some(ref tools_path) = tools_file {
            match tool_def_port.load(tools_path) {
                Ok(tools_map) => {
                    println!("✓ Tools file parsed: {} server(s) defined", tools_map.len());
                    for server in &agent.mcp_servers {
                        if tools_map.contains_key(server.as_str()) {
                            println!("  ✓ mcp_servers '{}' found in tools file", server);
                        } else {
                            errors.push(format!(
                                "mcp_servers '{}': not found in tools file {:?}",
                                server, tools_path
                            ));
                        }
                    }
                }
                Err(e) => {
                    errors.push(format!("Tools file parse error: {}", e));
                }
            }
        } else if !agent.mcp_servers.is_empty() {
            println!(
                "  ⚠ mcp_servers is non-empty but --tools-file was not provided; skipping server check"
            );
        }
    }

    if errors.is_empty() {
        println!("\nValidation passed.");
        Ok(())
    } else {
        eprintln!("\nValidation failed with {} error(s):", errors.len());
        for e in &errors {
            eprintln!("  ✗ {}", e);
        }
        bail!("Validation failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::persistence::agent_def::FileAgentDefAdapter;
    use crate::infra::persistence::tool_def::FileToolDefAdapter;
    use std::fs;

    fn write_agent(dir: &std::path::Path, name: &str, extra_frontmatter: &str) -> PathBuf {
        let path = dir.join(format!("{}.md", name));
        fs::write(
            &path,
            format!(
                "---\nname: {name}\ndescription: test\nmodel: test-model\n{extra}\n---\n",
                name = name,
                extra = extra_frontmatter
            ),
        )
        .unwrap();
        path
    }

    #[test]
    fn callable_by_rejects_unknown_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "callable_by:\n  - human\n");
        let result = validate(path, None, &FileAgentDefAdapter, &FileToolDefAdapter::default());
        assert!(result.is_err());
    }

    #[test]
    fn callable_by_accepts_known_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "callable_by:\n  - user\n  - agent\n");
        let result = validate(path, None, &FileAgentDefAdapter, &FileToolDefAdapter::default());
        assert!(result.is_ok());
    }

    // Individual reasons are printed to stderr rather than returned (the error
    // is a flat "Validation failed"), so these two assert as a pair: the
    // fixtures differ only in the shape of `extra_body.tools`, which is what
    // isolates the check being exercised.
    #[test]
    fn extra_body_tools_must_be_an_array() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "extra_body:\n  tools: web_search\n");
        assert!(validate(path, None, &FileAgentDefAdapter, &FileToolDefAdapter::default()).is_err());
    }

    #[test]
    fn extra_body_tools_as_an_array_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(
            dir.path(),
            "solo",
            "extra_body:\n  tools:\n    - type: openrouter:web_search\n",
        );
        assert!(validate(path, None, &FileAgentDefAdapter, &FileToolDefAdapter::default()).is_ok());
    }

    #[test]
    fn knows_about_target_must_accept_agent_delegation() {
        let dir = tempfile::tempdir().unwrap();
        // "helper" does not list "agent" in callable_by, so it cannot be delegated to.
        write_agent(dir.path(), "helper", "callable_by:\n  - user\n");
        let caller = write_agent(
            dir.path(),
            "caller",
            "callable_by:\n  - user\nknows_about:\n  - helper\n",
        );
        let result = validate(caller, None, &FileAgentDefAdapter, &FileToolDefAdapter::default());
        assert!(result.is_err());
    }

    #[test]
    fn knows_about_target_accepting_agent_delegation_passes() {
        let dir = tempfile::tempdir().unwrap();
        write_agent(dir.path(), "helper", "callable_by:\n  - agent\n");
        let caller = write_agent(
            dir.path(),
            "caller",
            "callable_by:\n  - user\nknows_about:\n  - helper\n",
        );
        let result = validate(caller, None, &FileAgentDefAdapter, &FileToolDefAdapter::default());
        assert!(result.is_ok());
    }
}
