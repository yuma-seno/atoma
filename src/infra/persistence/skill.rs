use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::skill::{Skill, SkillCatalog, SkillMetadata};

pub fn load(root: &Path) -> Result<SkillCatalog> {
    let root = root
        .canonicalize()
        .with_context(|| format!("Failed to resolve skills directory: {:?}", root))?;
    if !root.is_dir() {
        anyhow::bail!("Skills path is not a directory: {:?}", root);
    }

    let mut paths = Vec::new();
    collect_markdown_files(&root, &mut paths)?;
    paths.sort();

    let mut skills = Vec::new();
    for path in paths {
        let skill = parse_skill(&path)?;
        skills.push(skill);
    }
    SkillCatalog::new(skills)
}

pub struct FileSkillAdapter;

impl crate::domain::ports::SkillPort for FileSkillAdapter {
    fn load(&self, root: &Path) -> Result<SkillCatalog> {
        load(root)
    }
}
fn collect_markdown_files(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("Failed to read skills directory: {:?}", directory))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_symlink() {
            anyhow::bail!("Skill catalog does not allow symbolic links: {:?}", path);
        }
        if file_type.is_dir() {
            collect_markdown_files(&path, paths)?;
        } else if file_type.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn parse_skill(path: &Path) -> Result<Skill> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read skill definition: {:?}", path))?;
    let content = raw.trim_start();
    if !content.starts_with("---") {
        anyhow::bail!("Skill definition must start with `---`: {:?}", path);
    }

    let rest = &content[3..];
    let (end, delimiter_len) = if let Some(pos) = rest.find("\r\n---") {
        (pos, 5)
    } else if let Some(pos) = rest.find("\n---") {
        (pos, 4)
    } else {
        anyhow::bail!("Could not find closing `---` in skill: {:?}", path);
    };

    let metadata: SkillMetadata = serde_yaml::from_str(&rest[..end])
        .with_context(|| format!("Failed to parse skill frontmatter: {:?}", path))?;
    if metadata.name.trim().is_empty() || metadata.description.trim().is_empty() {
        anyhow::bail!("Skill name and description must not be empty: {:?}", path);
    }

    let instructions = content[3 + end + delimiter_len..].trim().to_string();
    if instructions.is_empty() {
        anyhow::bail!("Skill instructions must not be empty: {:?}", path);
    }

    Ok(Skill {
        metadata,
        instructions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_nested_skills_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("engineering")).unwrap();
        fs::write(
            dir.path().join("engineering/tdd.md"),
            "---\nname: engineering/tdd\ndescription: Test first.\n---\n\nUse red-green-refactor.\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("review.md"),
            "---\nname: review/correctness\ndescription: Review behavior.\n---\n\nCheck invariants.\n",
        )
        .unwrap();

        let catalog = load(dir.path()).unwrap();
        let names: Vec<_> = catalog
            .metadata()
            .into_iter()
            .map(|metadata| metadata.name)
            .collect();
        assert_eq!(names, vec!["engineering/tdd", "review/correctness"]);
        assert_eq!(
            catalog.get("engineering/tdd").unwrap().instructions,
            "Use red-green-refactor."
        );
    }

    #[test]
    fn rejects_duplicate_names() {
        let dir = tempfile::tempdir().unwrap();
        let skill = "---\nname: duplicate\ndescription: Duplicate.\n---\n\nInstructions.\n";
        fs::write(dir.path().join("one.md"), skill).unwrap();
        fs::write(dir.path().join("two.md"), skill).unwrap();

        let error = load(dir.path()).unwrap_err();
        assert!(error
            .to_string()
            .contains("Duplicate skill name 'duplicate'"));
    }

    #[test]
    fn rejects_missing_instructions() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("empty.md"),
            "---\nname: empty\ndescription: Empty.\n---\n",
        )
        .unwrap();

        let error = load(dir.path()).unwrap_err();
        assert!(error.to_string().contains("instructions must not be empty"));
    }
}
