//! Where credential values come from, and the one place that decides.
//!
//! # Two sources, never both
//!
//! A file, or the environment. If `--credentials-file` is given, that file is the
//! only source and the environment is not consulted for credentials at all; if it
//! is not, values are read from the environment exactly as they always were.
//!
//! Two sources rather than one because the contexts genuinely differ. A file
//! delivered by CI is ephemeral and can be deleted the moment it has been read; a
//! developer's is a config file that belongs to them and must not be. Collapsing
//! them into one concept would mean a flag deciding whether to delete, and would
//! push a hand-run `atoma` into keeping credentials in plaintext on disk. The
//! environment is the better answer there.
//!
//! # Why a file at all
//!
//! Because a value in an environment block cannot be taken back. `/proc/<pid>/
//! environ` reflects what was placed on the stack at `execve`, and glibc's
//! `setenv`/`unsetenv` do not rewrite it — so anything that was ever in a
//! process's environment stays readable there, by any process of the same user,
//! for that process's lifetime. Measured, not assumed.
//!
//! A file has the one property the environment lacks: its exposure can be ended.
//! So `from_file` reads it and immediately deletes it, and the caller does that
//! before any tool server exists. The file and the servers never coexist.
//!
//! This is the arrangement Kubernetes settled on for the same reasons — secrets
//! mounted as files rather than injected as environment variables — so it is the
//! ordinary shape rather than a workaround.
//!
//! # What this does NOT do
//!
//! It does not put anything into this process's own environment. That would put
//! it back in `/proc` and undo the point. Values live in this map, and reach a
//! child only by being named in that child's `env` in the tools file — see
//! `expand`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// The environment variables this program treats as credentials.
///
/// Exactly the names atoma's own code reads a secret from — the provider keys in
/// `infra/llm/*` and the GitHub tokens the tool servers authenticate with. Not an
/// attempt to enumerate every secret a variable could hold; a list like that is
/// wrong the moment someone invents a name nobody here thought of.
///
/// Used to keep them out of the environment of the tool servers this process
/// spawns. A server that legitimately needs one names it in its own `env` in the
/// tools file, which is applied after the removal and so puts it back — for that
/// one server and no other.
///
/// In file mode this is mostly moot, because none of these are in this process's
/// environment to begin with. It earns its place in environment mode, which is
/// how a developer runs atoma by hand: there the provider key really is inherited
/// by every server, and `shell` could read it out of its own environment without
/// going anywhere near `/proc`.
/// The GitHub tokens. The provider keys come from the provider list itself, via
/// [`credential_env_names`] below.
///
/// Half of this list used to be provider keys, written out a second time. They drifted
/// the day two providers were added: `OPENROUTER_API_KEY` and `ORCAROUTER_API_KEY`
/// were declared in `infra::llm` and missing here, so in environment mode a tool
/// server inherited them -- and `shell` could read a provider key out of its own
/// environment without going near `/proc`.
const GITHUB_ENV_NAMES: &[&str] = &["GH_TOKEN", "GITHUB_PERSONAL_ACCESS_TOKEN", "GITHUB_TOKEN"];

/// Every name a tool server must not inherit.
///
/// A union rather than a list: the provider half is whatever `infra::llm` declares, so
/// adding a provider covers it here with nothing to remember.
pub fn credential_env_names() -> Vec<&'static str> {
    let mut names = crate::infra::llm::provider_credential_names();
    names.extend_from_slice(GITHUB_ENV_NAMES);
    names.sort_unstable();
    names.dedup();
    names
}

/// The credential values available to this run.
pub struct Credentials {
    /// `Some` when a file was supplied, and then the only source. `None` means
    /// read from the environment.
    values: Option<HashMap<String, String>>,
}

impl Credentials {
    /// Read a JSON object of `{"NAME": "value"}` and delete the file.
    ///
    /// Deleting here rather than leaving it to the caller is deliberate: the
    /// guarantee is that no tool server ever coexists with the file, and the only
    /// way to be sure is for the read and the delete to be the same act. A
    /// workflow that removed it afterwards would leave it readable for the whole
    /// run.
    ///
    /// A file that cannot be deleted is a warning rather than a failure. The
    /// values are already in memory, the run can proceed, and saying so is more
    /// use than refusing to start.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read credentials file: {:?}", path))?;

        let values: HashMap<String, String> =
            serde_json::from_str(&content).with_context(|| {
                format!(
                    "Credentials file is not a JSON object of name/value pairs: {:?}",
                    path
                )
            })?;

        if let Err(error) = fs::remove_file(path) {
            tracing::warn!(
                ?path,
                %error,
                "could not delete the credentials file after reading it; it stays readable to anything running as this user for the rest of the run"
            );
        }

        tracing::debug!(
            count = values.len(),
            "credentials read from file and the file removed"
        );
        Ok(Self {
            values: Some(values),
        })
    }

    /// Read credentials from the environment, as before this existed.
    pub fn from_environment() -> Self {
        Self { values: None }
    }

    /// The value for `name`, from whichever source this was built with.
    pub fn get(&self, name: &str) -> Option<String> {
        match &self.values {
            Some(values) => values.get(name).cloned(),
            None => std::env::var(name).ok(),
        }
    }

    /// Whether `name` has a non-empty value.
    ///
    /// Used to decide which provider a run is for, so an empty string has to
    /// count as absent: a workflow that exports a secret which is not set passes
    /// one, and auto-detection reading that as "OpenAI is configured" is how a
    /// run ends up failing with the wrong provider's error message.
    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some_and(|value| !value.is_empty())
    }

    /// Replace `${NAME}` in `template` with the credential of that name.
    ///
    /// This is how a value reaches one tool server and not the others: the tools
    /// file names it in that server's `env`, and nothing else sees it. An unknown
    /// name expands to empty with a warning rather than failing the run — a
    /// project that has declared a credential it has not added yet should get a
    /// server that cannot authenticate, and a log line saying why, not a run that
    /// refuses to start.
    pub fn expand(&self, template: &str) -> String {
        expand_with(template, |name| self.get(name), "credential")
    }
}

/// Substitute `${NAME}` and `${NAME:-default}` from whatever `lookup` returns.
///
/// Shared by two callers that must NOT share a source. A credential belongs only
/// in the environment of the server that declared it, so `env:` values resolve
/// against the credentials; a program path is not a secret and must work when no
/// credentials exist at all, so `args` resolve against the process environment.
/// Keeping the substitution common and the sources separate is what stops a path
/// from becoming a way to read a credential.
///
/// An unknown name expands to its default, or to empty with a warning when it has
/// none. Failing the run instead would turn a project declaring a credential it
/// has not added yet into a repository whose agents cannot start.
fn expand_with(template: &str, lookup: impl Fn(&str) -> Option<String>, what: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            // No closing brace: not a reference, so it is literal text.
            out.push_str(&rest[start..]);
            return out;
        };

        let (name, fallback) = match after[..end].split_once(":-") {
            Some((name, fallback)) => (name, Some(fallback)),
            None => (&after[..end], None),
        };

        match lookup(name) {
            Some(value) => out.push_str(&value),
            None => match fallback {
                Some(fallback) => out.push_str(fallback),
                None => tracing::warn!(
                    name,
                    what,
                    "a tools file references something that is not available; it will be empty"
                ),
            },
        }
        rest = &after[end + 1..];
    }

    out.push_str(rest);
    out
}

/// Substitute `${NAME}` in a tools file's `args` from the process environment.
///
/// Deliberately not the credentials. These are program paths -- which is how the
/// delivery runner points a tool server at a checkout of the default branch
/// rather than at the pull request under review -- and a path that only resolved
/// when a credential of the same name existed would be a trap.
pub fn expand_from_environment(template: &str) -> String {
    expand_with(
        template,
        |name| std::env::var(name).ok(),
        "environment variable",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn from_pairs(pairs: &[(&str, &str)]) -> Credentials {
        Credentials {
            values: Some(
                pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect(),
            ),
        }
    }

    #[test]
    fn a_file_is_read_and_then_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("creds.json");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(br#"{"OPENAI_API_KEY":"sk-test","SLACK_TOKEN":"xoxb-test"}"#)
            .unwrap();
        drop(file);

        let credentials = Credentials::from_file(&path).unwrap();

        assert_eq!(
            credentials.get("OPENAI_API_KEY"),
            Some("sk-test".to_string())
        );
        assert!(
            !path.exists(),
            "the file must be gone before any tool server can read it"
        );
    }

    /// The ordering the guarantee rests on, pinned against a refactor.
    ///
    /// `from_file` deletes as it reads, so "the file is gone before any tool
    /// server starts" holds only while the read happens before the command runs.
    /// Move it after `match cli.command` and the file would sit on disk for the
    /// whole run, readable by every server the agent spawns -- and nothing would
    /// fail. The guarantee would simply be gone.
    ///
    /// Checking the source is crude, and it is the only thing that catches this:
    /// a behavioural test would need a full agent run, and the failure it guards
    /// against is invisible at runtime.
    #[test]
    fn credentials_are_read_before_the_command_runs() {
        let main_rs = include_str!("../main.rs");
        let read_at = main_rs
            .find("Credentials::from_file")
            .expect("main.rs must build credentials from a file");
        let dispatch_at = main_rs
            .find("match cli.command")
            .expect("main.rs must dispatch on the subcommand");
        assert!(
            read_at < dispatch_at,
            "credentials must be read (and the file deleted) before any command runs, or the file outlives the tool servers"
        );
    }

    #[test]
    fn a_malformed_file_fails_rather_than_yielding_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("creds.json");
        fs::write(&path, "not json").unwrap();
        assert!(Credentials::from_file(&path).is_err());
    }

    /// File mode means the file is the ONLY source. Falling through to the
    /// environment would reintroduce exactly what the file exists to avoid.
    #[test]
    fn file_mode_does_not_fall_back_to_the_environment() {
        // SAFETY: single-threaded test, and the name is unique to it.
        unsafe { std::env::set_var("ATOMA_TEST_ONLY_IN_ENV", "from-env") };
        let credentials = from_pairs(&[("OTHER", "x")]);
        assert_eq!(credentials.get("ATOMA_TEST_ONLY_IN_ENV"), None);
        unsafe { std::env::remove_var("ATOMA_TEST_ONLY_IN_ENV") };
    }

    #[test]
    fn environment_mode_reads_the_environment() {
        // SAFETY: as above.
        unsafe { std::env::set_var("ATOMA_TEST_ENV_MODE", "yes") };
        let credentials = Credentials::from_environment();
        assert_eq!(
            credentials.get("ATOMA_TEST_ENV_MODE"),
            Some("yes".to_string())
        );
        unsafe { std::env::remove_var("ATOMA_TEST_ENV_MODE") };
    }

    /// An exported-but-unset secret arrives as an empty string. Reading that as
    /// "configured" is how a run picks the wrong provider and then fails with a
    /// message about the one it did not want.
    #[test]
    fn an_empty_value_counts_as_absent() {
        let credentials = from_pairs(&[("OPENAI_API_KEY", "")]);
        assert!(!credentials.has("OPENAI_API_KEY"));
        assert!(from_pairs(&[("OPENAI_API_KEY", "sk-x")]).has("OPENAI_API_KEY"));
    }

    #[test]
    fn expand_substitutes_a_reference() {
        let credentials = from_pairs(&[("SLACK_TOKEN", "xoxb-1")]);
        assert_eq!(credentials.expand("${SLACK_TOKEN}"), "xoxb-1");
        assert_eq!(
            credentials.expand("Bearer ${SLACK_TOKEN} end"),
            "Bearer xoxb-1 end"
        );
    }

    #[test]
    fn expand_leaves_text_without_a_reference_alone() {
        let credentials = from_pairs(&[("A", "1")]);
        assert_eq!(credentials.expand("plain value"), "plain value");
        assert_eq!(credentials.expand(""), "");
        // Unterminated: literal, not a reference.
        assert_eq!(credentials.expand("${UNCLOSED"), "${UNCLOSED");
    }

    #[test]
    fn expand_yields_empty_for_a_name_it_does_not_have() {
        let credentials = from_pairs(&[("A", "1")]);
        assert_eq!(credentials.expand("x${MISSING}y"), "xy");
    }

    /// A tools file has to keep working where the variable is unset -- a hand-run
    /// `atoma` sets no machinery root, and a program path of `/…` would not exist.
    #[test]
    fn a_default_covers_an_absent_name() {
        let credentials = from_pairs(&[("A", "1")]);
        assert_eq!(credentials.expand("${MISSING:-fallback}"), "fallback");
        assert_eq!(credentials.expand("${A:-fallback}"), "1");
        assert_eq!(credentials.expand("${MISSING:-}/x"), "/x");
    }

    /// The split that keeps a program path from becoming a way to read a secret.
    #[test]
    fn args_expansion_reads_the_environment_and_not_the_credentials() {
        // SAFETY: single-threaded test, and the names are unique to it.
        unsafe { std::env::set_var("ATOMA_TEST_ARG_ROOT", "machinery") };
        assert_eq!(
            expand_from_environment("${ATOMA_TEST_ARG_ROOT}/x.ts"),
            "machinery/x.ts"
        );
        assert_eq!(
            expand_from_environment("${ATOMA_TEST_ARG_ROOT_UNSET:-.}/x.ts"),
            "./x.ts"
        );
        unsafe { std::env::remove_var("ATOMA_TEST_ARG_ROOT") };
    }

    #[test]
    fn expand_handles_several_references() {
        let credentials = from_pairs(&[("A", "1"), ("B", "2")]);
        assert_eq!(credentials.expand("${A}-${B}-${A}"), "1-2-1");
    }
}
