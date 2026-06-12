use anyhow::{bail, Result};
use std::path::PathBuf;

use crate::domain::ports::{AgentDefPort, ToolDefPort};

/// Validate an agent definition file and optional tools file.
///
/// Checks:
///   1. Agent definition parses without error (YAML, required fields).
///   2. Each `knows_about` entry has a corresponding `<name>.md` in the same directory.
///   3. `extra_body` does not override the reserved keys `model` or `messages`.
///   4. If a tools file is provided:
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

        for name in &agent.knows_about {
            let candidate = agent_def_dir.join(format!("{}.md", name));
            if candidate.exists() {
                println!("  ✓ knows_about '{}' → {:?}", name, candidate);
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
