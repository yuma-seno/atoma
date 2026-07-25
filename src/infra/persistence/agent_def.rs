use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::domain::agent::{AgentDef, ParsedAgentDef};

/// Parse a markdown file with YAML frontmatter into a `ParsedAgentDef`.
///
/// The file must begin with a `---` frontmatter block followed by YAML.
/// Any markdown body after the second `---` delimiter is returned as
/// `ParsedAgentDef::body` and used as the agent's role prompt template.
pub fn parse(path: &Path) -> Result<ParsedAgentDef> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read agent definition: {:?}", path))?;
    let content = raw.trim_start();

    if !content.starts_with("---") {
        anyhow::bail!("Agent definition must start with `---` frontmatter delimiter");
    }

    let rest = &content[3..];

    let (end, delimiter_len) = if let Some(pos) = rest.find("\r\n---") {
        (pos, 5)
    } else if let Some(pos) = rest.find("\n---") {
        (pos, 4)
    } else {
        anyhow::bail!(
            "Could not find closing `---` for frontmatter in: {:?}",
            path
        )
    };

    let yaml_str = &rest[..end];
    let body_raw = content[3 + end + delimiter_len..].trim().to_string();

    let frontmatter: AgentDef = serde_yaml::from_str(yaml_str)
        .with_context(|| format!("Failed to parse YAML frontmatter in: {:?}", path))?;

    Ok(ParsedAgentDef {
        frontmatter,
        body: if body_raw.is_empty() {
            None
        } else {
            Some(body_raw)
        },
    })
}

// ── Port adapter ──────────────────────────────────────────────────────────────

/// File-system adapter implementing `AgentDefPort`.
pub struct FileAgentDefAdapter;

impl crate::domain::ports::AgentDefPort for FileAgentDefAdapter {
    fn parse(
        &self,
        path: &std::path::Path,
    ) -> anyhow::Result<crate::domain::agent::ParsedAgentDef> {
        parse(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-agent.md");
        let markdown = "---\nname: TestAgent\ndescription: A test agent\nmodel: claude-3-5-sonnet\nmcp_servers:\n  - filesystem\n---\n\nThis is the custom system prompt body.\n";
        fs::write(&path, markdown).unwrap();
        let parsed = parse(&path).unwrap();
        assert_eq!(parsed.frontmatter.name, "TestAgent");
        assert_eq!(parsed.frontmatter.mcp_servers, vec!["filesystem"]);
        assert_eq!(
            parsed.body.unwrap(),
            "This is the custom system prompt body."
        );
    }

    #[test]
    fn test_parse_no_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-agent.md");
        let markdown =
            "---\nname: TestAgent2\ndescription: Another test\nmodel: claude-3-5-sonnet\nmetadata:\n  legacy: value\n---\n";
        fs::write(&path, markdown).unwrap();
        let parsed = parse(&path).unwrap();
        assert_eq!(parsed.frontmatter.name, "TestAgent2");
        assert!(parsed.body.is_none());
    }
}
