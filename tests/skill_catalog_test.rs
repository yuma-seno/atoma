//! The skills this repository ships, loaded the way a run loads them.
//!
//! The catalog is what the system prompt lists and what `load_skill` accepts, so a skill
//! that stops being found here is one no run can load. It is asserted against the real
//! tree rather than a fixture: a fixture cannot be wrong in the way a moved file or a
//! `name:` left as a path is wrong, and this is the one place in the repository where the
//! layout `<category>/<skill-name>/SKILL.md` (https://agentskills.io/specification) is
//! checked as a whole.

use std::path::Path;

#[test]
fn the_repositorys_skills_load_under_their_leaf_names() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/atomaton/skills");
    let catalog = atoma::infra::persistence::skill::load(&root).expect("the shipped skills load");

    let names: Vec<String> = catalog
        .metadata()
        .into_iter()
        .map(|metadata| metadata.name)
        .collect();
    assert_eq!(
        names,
        vec![
            "agent-prompt-surface",
            "customer-doc",
            "environment",
            "gh-actions-surface",
            "japanese",
            "on-demand-audit",
            "pipeline-setup",
            "secrets-in-a-check",
            "web-search",
        ]
    );
}
