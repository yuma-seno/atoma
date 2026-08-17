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

        let values: HashMap<String, String> = serde_json::from_str(&content).with_context(|| {
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

    /// Every name this holds, for the strip set. Empty in environment mode,
    /// which is correct: there is no declared set to derive from there.
    pub fn names(&self) -> Vec<String> {
        match &self.values {
            Some(values) => values.keys().cloned().collect(),
            None => Vec::new(),
        }
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
            let name = &after[..end];
            match self.get(name) {
                Some(value) => out.push_str(&value),
                None => {
                    tracing::warn!(
                        name,
                        "a tools file references a credential that is not available; the server will get an empty value"
                    );
                }
            }
            rest = &after[end + 1..];
        }

        out.push_str(rest);
        out
    }
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

    #[test]
    fn expand_handles_several_references() {
        let credentials = from_pairs(&[("A", "1"), ("B", "2")]);
        assert_eq!(credentials.expand("${A}-${B}-${A}"), "1-2-1");
    }

    #[test]
    fn names_are_empty_in_environment_mode() {
        assert!(Credentials::from_environment().names().is_empty());
        let mut names = from_pairs(&[("A", "1"), ("B", "2")]).names();
        names.sort();
        assert_eq!(names, vec!["A".to_string(), "B".to_string()]);
    }
}
