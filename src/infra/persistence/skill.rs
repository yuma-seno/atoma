use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::skill::{Skill, SkillCatalog, SkillMetadata};

/// The one file a skill directory must hold.
///
/// The directory is the skill, the way https://agentskills.io/specification defines it:
/// everything else in it -- `scripts/`, `references/`, `assets/` -- belongs to the
/// instructions and is never a definition of its own. A bare `.md` per skill carried
/// prose and nothing beside it, so a procedure with a script behind it had to spell that
/// script's path as prose and hope the two stayed in step.
const SKILL_FILE: &str = "SKILL.md";

/// The spec's ceiling on `name`, and on `description`.
const MAX_NAME_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 1024;

pub fn load(root: &Path) -> Result<SkillCatalog> {
    let root = root
        .canonicalize()
        .with_context(|| format!("Failed to resolve skills directory: {:?}", root))?;
    if !root.is_dir() {
        anyhow::bail!("Skills path is not a directory: {:?}", root);
    }

    let mut paths = Vec::new();
    collect_skill_files(&root, &mut paths)?;
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

/// Find every `SKILL.md` under `directory`, descending only where there is none.
///
/// A directory holding a `SKILL.md` is a skill, so its children are its resources and
/// are not searched for further skills -- one skill cannot nest inside another. Every
/// directory above one of them is a category, which the layout uses to group skills and
/// the spec says nothing about.
fn collect_skill_files(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("Failed to read skills directory: {:?}", directory))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_symlink() {
            anyhow::bail!("Skill catalog does not allow symbolic links: {:?}", path);
        }
        if !file_type.is_dir() {
            // Loose files are ignored rather than refused. A skill directory is a
            // package, and a stray note or script beside the categories is not a claim
            // to be one.
            continue;
        }
        let definition = path.join(SKILL_FILE);
        match fs::symlink_metadata(&definition) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                anyhow::bail!(
                    "Skill catalog does not allow symbolic links: {:?}",
                    definition
                );
            }
            Ok(metadata) if metadata.is_file() => paths.push(definition),
            _ => collect_skill_files(&path, paths)?,
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
    validate_name(path, &metadata.name)?;
    validate_description(path, &metadata.description)?;

    let instructions = content[3 + end + delimiter_len..].trim().to_string();
    if instructions.is_empty() {
        anyhow::bail!("Skill instructions must not be empty: {:?}", path);
    }

    Ok(Skill {
        metadata,
        instructions,
    })
}

/// The spec's rules for a skill name, each refused with the file and the rule it broke.
///
/// A name is what an agent types into `load_skill` and what a directory is called, so
/// the two agree by construction: the directory name is the name, and a `name` field
/// that says otherwise would be a skill reachable under a word nothing on disk holds.
fn validate_name(path: &Path, name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Skill name must not be empty: {:?}", path);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        anyhow::bail!(
            "Skill name '{}' is {} characters; the limit is {MAX_NAME_CHARS}: {:?}",
            name,
            name.chars().count(),
            path
        );
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        anyhow::bail!(
            "Skill name '{}' may hold only lowercase letters, digits and hyphens: {:?}",
            name,
            path
        );
    }
    if name.starts_with('-') || name.ends_with('-') {
        anyhow::bail!(
            "Skill name '{}' must not start or end with a hyphen: {:?}",
            name,
            path
        );
    }
    if name.contains("--") {
        anyhow::bail!(
            "Skill name '{}' must not hold two hyphens in a row: {:?}",
            name,
            path
        );
    }
    let directory = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|d| d.to_str());
    if directory != Some(name) {
        anyhow::bail!(
            "Skill name '{}' must be the directory it lives in, which is '{}': {:?}",
            name,
            directory.unwrap_or_default(),
            path
        );
    }
    Ok(())
}

fn validate_description(path: &Path, description: &str) -> Result<()> {
    if description.trim().is_empty() {
        anyhow::bail!("Skill description must not be empty: {:?}", path);
    }
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        anyhow::bail!(
            "Skill description is {} characters; the limit is {MAX_DESCRIPTION_CHARS}: {:?}",
            description.chars().count(),
            path
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(directory: &Path, name: &str, description: &str, body: &str) {
        let skill_dir = directory.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join(SKILL_FILE),
            format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
        )
        .unwrap();
    }

    #[test]
    fn loads_nested_skills_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(dir.path(), "tdd", "Test first.", "Use red-green-refactor.");
        // `tdd` above sits directly in the root, this one inside a category, and both are
        // the same kind of thing.
        fs::create_dir_all(dir.path().join("review")).unwrap();
        write_skill(
            &dir.path().join("review"),
            "correctness",
            "Review behavior.",
            "Check invariants.",
        );

        let catalog = load(dir.path()).unwrap();
        let names: Vec<_> = catalog
            .metadata()
            .into_iter()
            .map(|metadata| metadata.name)
            .collect();
        assert_eq!(names, vec!["correctness", "tdd"]);
        assert_eq!(
            catalog.get("tdd").unwrap().instructions,
            "Use red-green-refactor."
        );
    }

    /// A skill directory is a package. Loose files beside it, and scripts inside it, are
    /// the instructions' resources -- not definitions, and not a reason to fail.
    #[test]
    fn files_that_are_not_a_skill_definition_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(dir.path(), "tdd", "Test first.", "Use red-green-refactor.");
        fs::write(dir.path().join("README.md"), "Category notes.\n").unwrap();
        fs::create_dir_all(dir.path().join("tdd/scripts")).unwrap();
        fs::write(dir.path().join("tdd/scripts/run.sh"), "#!/bin/sh\n").unwrap();

        let catalog = load(dir.path()).unwrap();
        let names: Vec<_> = catalog
            .metadata()
            .into_iter()
            .map(|metadata| metadata.name)
            .collect();
        assert_eq!(names, vec!["tdd"]);
    }

    #[test]
    fn rejects_duplicate_names() {
        let dir = tempfile::tempdir().unwrap();
        for category in ["one", "two"] {
            fs::create_dir_all(dir.path().join(category)).unwrap();
            write_skill(
                &dir.path().join(category),
                "duplicate",
                "Duplicate.",
                "Instructions.",
            );
        }

        let error = load(dir.path()).unwrap_err();
        assert!(error
            .to_string()
            .contains("Duplicate skill name 'duplicate'"));
    }

    #[test]
    fn rejects_missing_instructions() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("empty")).unwrap();
        fs::write(
            dir.path().join("empty/SKILL.md"),
            "---\nname: empty\ndescription: Empty.\n---\n",
        )
        .unwrap();

        let error = load(dir.path()).unwrap_err();
        assert!(error.to_string().contains("instructions must not be empty"));
    }

    /// Every rule the spec puts on a name, each refused with the rule named.
    #[test]
    fn a_name_outside_the_specs_rules_is_refused() {
        for (name, rule) in [
            ("Engineering", "only lowercase letters"),
            ("engineering_skills", "only lowercase letters"),
            ("-leading", "must not start or end with a hyphen"),
            ("trailing-", "must not start or end with a hyphen"),
            ("two--hyphens", "two hyphens in a row"),
            (
                "a-name-far-too-long-for-the-spec-and-then-some-more-characters-beside-it",
                "the limit is 64",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            // Written where the directory agrees with the name, so the rule under test is
            // the one that fires rather than the directory check standing in for it.
            write_skill(dir.path(), name, "A skill.", "Instructions.");

            let error = load(dir.path()).unwrap_err().to_string();
            assert!(error.contains(rule), "{name}: {error}");
            assert!(error.contains("SKILL.md"), "{name}: {error}");
        }
    }

    #[test]
    fn a_name_the_directory_disagrees_with_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("delivery")).unwrap();
        fs::write(
            dir.path().join("delivery/SKILL.md"),
            "---\nname: pipeline-setup\ndescription: Set up checks.\n---\n\nInstructions.\n",
        )
        .unwrap();

        let error = load(dir.path()).unwrap_err().to_string();
        assert!(
            error.contains("must be the directory it lives in, which is 'delivery'"),
            "{error}"
        );
        assert!(error.contains("SKILL.md"), "{error}");
    }

    #[test]
    fn a_description_outside_the_specs_rules_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let long = "x".repeat(MAX_DESCRIPTION_CHARS + 1);
        write_skill(dir.path(), "long", &long, "Instructions.");

        let error = load(dir.path()).unwrap_err().to_string();
        assert!(error.contains("the limit is 1024"), "{error}");
        assert!(error.contains("SKILL.md"), "{error}");
    }
}
