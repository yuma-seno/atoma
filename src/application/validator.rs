use anyhow::{bail, Result};
use std::path::PathBuf;

use crate::domain::ports::{AgentDefPort, ToolDefPort};
use crate::infra::llm::check_provider_name;
use crate::infra::template::unknown_placeholders;

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
///   7. `provider`, when named, is one this build has.
///   8. If a template is provided: every `{{...}}` in it is one that gets
///      substituted.
///
/// The last two are here rather than in a caller because they are facts only this
/// crate holds. `atoma-autonomous-delivery` runs this against every agent
/// definition a pull request would merge; checking them there would mean keeping a
/// copy of the provider list and of the template vocabulary in another repository,
/// in another language.
pub fn validate(
    agent_def_path: PathBuf,
    tools_file: Option<PathBuf>,
    template_file: Option<PathBuf>,
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

        // The adapters' own list, not a copy of it. This was `["model", "messages"]`
        // written out here, so `validate` passed an `extra_body.input` that the Responses
        // adapter drops and an `extra_body.system` that Anthropic's does -- reporting a
        // configuration as sound while two of the three dialects quietly ignored part of
        // it.
        for key in crate::infra::llm::shared::RESERVED_KEYS {
            if agent.extra_body.contains_key(key) {
                errors.push(format!(
                    "extra_body contains reserved key '{}' which would be silently ignored",
                    key
                ));
            }
        }

        // The name only. A provider that does not exist is a defect in the definition;
        // a credential that is not set is not, and a validation run has none -- so
        // conflating them would fail every run of this command.
        if let Some(ref provider) = agent.provider {
            let name = provider.trim();
            if name.is_empty() {
                errors.push("provider is present but empty; remove it or name one".to_string());
            } else {
                match check_provider_name(name) {
                    Ok(()) => println!("  ✓ provider '{}' is known", name),
                    Err(e) => errors.push(format!("{}", e)),
                }
            }
        }

        // No `tools` shape check here. A non-array is not fatal -- `reconcile_tools` keeps
        // the runtime definitions and warns at run time -- and this function reports only
        // errors, so its only way to speak was to call a tolerated configuration invalid.
        // Being stricter than the code it describes is its own kind of wrong answer.

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

    // Independent of the agent definition, so it runs even when that failed to parse:
    // a template is wrong or right on its own terms, and reporting both problems at
    // once beats reporting them one run apart.
    if let Some(ref template_path) = template_file {
        match std::fs::read_to_string(template_path) {
            Ok(template) => {
                let unknown = unknown_placeholders(&template);
                if unknown.is_empty() {
                    println!("✓ Template checked: every placeholder is one atoma substitutes");
                } else {
                    // Not a warning. An unsubstituted placeholder renders literally into
                    // the system prompt, where a model reads it as text it was given on
                    // purpose -- which is worse than an obviously missing section.
                    errors.push(format!(
                        "template {:?} uses {} placeholder(s) nothing substitutes: {}",
                        template_path,
                        unknown.len(),
                        unknown.join(", "),
                    ));
                }
            }
            Err(e) => errors.push(format!(
                "Failed to read template {:?}: {}",
                template_path, e
            )),
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

    /// The error the run would have produced, produced earlier. A definition naming
    /// a provider that does not exist used to fail at `build_llm_client` -- after the
    /// tool servers had started and the prompt had been assembled.
    #[test]
    fn an_unknown_provider_is_a_validation_error_that_names_the_alternatives() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "provider: openai-responsez\n");
        let result = validate(
            path,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default(),
        );
        assert!(result.is_err());
    }

    /// The distinction the check rests on: the name being unknown and the credential
    /// being absent are different facts, and only the first is a defect in the
    /// definition. No credentials are set here, which is the state every validation
    /// run is in.
    #[test]
    fn a_known_provider_passes_without_its_credential() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "provider: openai\n");
        let result = validate(
            path,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default(),
        );
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[test]
    fn a_definition_that_names_no_provider_is_not_checked_for_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "");
        assert!(validate(
            path,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default()
        )
        .is_ok());
    }

    #[test]
    fn a_template_placeholder_nothing_substitutes_fails_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "");
        let template = dir.path().join("prompt.md");
        fs::write(&template, "You are {{AGENT_NAME}}. Use {{AVAILABLE_TOOL}}.").unwrap();
        let result = validate(
            path,
            None,
            Some(template),
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn a_template_that_only_uses_known_placeholders_passes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "");
        let template = dir.path().join("prompt.md");
        fs::write(
            &template,
            "You are {{AGENT_NAME}} in {{WORKING_DIRECTORY}}.",
        )
        .unwrap();
        let result = validate(
            path,
            None,
            Some(template),
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default(),
        );
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[test]
    fn callable_by_rejects_unknown_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "callable_by:\n  - human\n");
        let result = validate(
            path,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn callable_by_accepts_known_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "callable_by:\n  - user\n  - agent\n");
        let result = validate(
            path,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default(),
        );
        assert!(result.is_ok());
    }

    // A non-array `tools` is TOLERATED: `reconcile_tools` keeps the runtime definitions
    // and warns. This used to assert the opposite, pinning a validator that was stricter
    // than the code it described.
    #[test]
    fn a_non_array_tools_is_not_a_validation_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(dir.path(), "solo", "extra_body:\n  tools: web_search\n");
        assert!(validate(
            path,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default()
        )
        .is_ok());
    }

    /// Every key any adapter assembles itself. `input` and `system` are the two that
    /// used to pass validation and then be silently dropped by the Responses and
    /// Anthropic adapters respectively.
    #[test]
    fn a_reserved_key_fails_validation_including_the_ones_only_one_dialect_owns() {
        for key in ["model", "messages", "input", "system", "store"] {
            let dir = tempfile::tempdir().unwrap();
            let path = write_agent(
                dir.path(),
                "solo",
                &format!("extra_body:\n  {key}: something\n"),
            );
            assert!(
                validate(
                    path,
                    None,
                    None,
                    &FileAgentDefAdapter,
                    &FileToolDefAdapter::default()
                )
                .is_err(),
                "{key} should be refused"
            );
        }
    }

    #[test]
    fn extra_body_tools_as_an_array_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_agent(
            dir.path(),
            "solo",
            "extra_body:\n  tools:\n    - type: openrouter:web_search\n",
        );
        assert!(validate(
            path,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default()
        )
        .is_ok());
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
        let result = validate(
            caller,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default(),
        );
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
        let result = validate(
            caller,
            None,
            None,
            &FileAgentDefAdapter,
            &FileToolDefAdapter::default(),
        );
        assert!(result.is_ok());
    }
}
