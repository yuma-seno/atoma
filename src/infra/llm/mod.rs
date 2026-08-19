pub mod anthropic;
pub mod copilot;
pub mod openai;
pub mod openai_responses;
pub(crate) mod shared;

use anyhow::{Context, Result};
use std::time::Duration;

use crate::domain::ports::LlmPort;
use crate::infra::credentials::Credentials;

pub use anthropic::AnthropicClient;
pub use copilot::CopilotClient;
pub use openai::OpenAIClient;
pub use openai_responses::OpenAIResponsesClient;

/// Ceiling on a single completion request, end to end.
///
/// This is a stall detector, not a generation budget: a provider that has not
/// finished in this long has almost always stopped responding altogether rather
/// than fallen behind. Raising it delays detection of exactly that case, so
/// prefer tuning `ATOMA_LLM_TIMEOUT` per model over changing this default.
const DEFAULT_LLM_TIMEOUT_SECS: u64 = 300;

/// Resolve the request timeout from a raw `ATOMA_LLM_TIMEOUT` value.
///
/// Absent, unparseable, and zero values all fall back to the default; a timeout
/// of zero would mean "no timeout", which is never what an operator wants here.
fn resolve_timeout_secs(raw: Option<&str>) -> u64 {
    raw.and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_LLM_TIMEOUT_SECS)
}

/// The wire format a provider's endpoint speaks.
///
/// A separate axis from the provider itself, because the two vary independently:
/// three providers speak `OpenAiChat`, and one host can offer two dialects. All
/// this decides is which client to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `POST {base}/chat/completions`.
    OpenAiChat,
    /// `POST {base}/responses`.
    OpenAiResponses,
    /// `POST {base}/v1/messages`, Anthropic's own format.
    AnthropicMessages,
    /// `OpenAiChat`, reached with a token exchanged from a GitHub credential.
    CopilotChat,
}

/// A provider, as data rather than as a `match` arm.
///
/// Four facts used to live inside four constructors: which credential to read,
/// where to send the request, what overrides that, and which extra headers to
/// attach. Every client read its own environment variable with its own default,
/// inline — which is why `github-copilot` had no configurable endpoint at all.
/// Nothing said each provider should have one, so the one that did not was
/// indistinguishable from a deliberate omission.
///
/// Written out here, adding a provider is a row, and "what does this one do
/// differently" has one place to be answered.
#[derive(Debug)]
pub struct Provider {
    /// The name in `ATOMA_PROVIDER`, and in an agent definition's `provider:`.
    pub name: &'static str,
    /// The credential it authenticates with.
    ///
    /// One provider, one name. That is what makes auto-detection an answer rather
    /// than a guess: `OPENAI_API_KEY` used to select a client whose default
    /// endpoint was OpenRouter, so the key's name said nothing about where it was
    /// going to be sent.
    pub credential: &'static str,
    /// Where it is, unless an operator says otherwise.
    pub default_base_url: &'static str,
    /// What says otherwise. Every provider has one; none is a special case.
    pub base_url_var: &'static str,
    /// Which wire format its endpoint speaks.
    pub dialect: Dialect,
    /// Whether it reads the attribution headers below.
    pub attribution: bool,
}

/// Every provider Atoma speaks to.
///
/// `openai` means OpenAI. It used to default to `https://openrouter.ai/api/v1`, so
/// the name pointed somewhere other than it said, and `openai-responses` carried a
/// comment explaining that it had to keep the same wrong default or the two would
/// disagree. The routers have their own names now, which puts a run's provider in
/// the log as a fact rather than as "openai, and read `OPENAI_BASE_URL` to find out
/// where that went".
///
/// `openai` and `openai-responses` still share one credential and one endpoint
/// variable, because they are one vendor reached two ways — and that pair is also
/// the way to reach a provider Atoma has no row for: point `OPENAI_BASE_URL` at
/// anything that speaks either dialect.
const PROVIDERS: &[Provider] = &[
    Provider {
        name: "openai",
        credential: "OPENAI_API_KEY",
        default_base_url: "https://api.openai.com/v1",
        base_url_var: "OPENAI_BASE_URL",
        dialect: Dialect::OpenAiChat,
        attribution: false,
    },
    Provider {
        name: "openai-responses",
        credential: "OPENAI_API_KEY",
        default_base_url: "https://api.openai.com/v1",
        base_url_var: "OPENAI_BASE_URL",
        dialect: Dialect::OpenAiResponses,
        attribution: false,
    },
    Provider {
        name: "openrouter",
        credential: "OPENROUTER_API_KEY",
        default_base_url: "https://openrouter.ai/api/v1",
        base_url_var: "OPENROUTER_BASE_URL",
        dialect: Dialect::OpenAiChat,
        attribution: true,
    },
    Provider {
        name: "orcarouter",
        credential: "ORCAROUTER_API_KEY",
        default_base_url: "https://api.orcarouter.ai/v1",
        base_url_var: "ORCAROUTER_BASE_URL",
        dialect: Dialect::OpenAiChat,
        attribution: false,
    },
    Provider {
        name: "anthropic",
        credential: "ANTHROPIC_API_KEY",
        default_base_url: "https://api.anthropic.com",
        base_url_var: "ANTHROPIC_BASE_URL",
        dialect: Dialect::AnthropicMessages,
        attribution: false,
    },
    Provider {
        name: "github-copilot",
        credential: "ATOMA_COPILOT_TOKEN",
        default_base_url: "https://api.githubcopilot.com",
        base_url_var: "COPILOT_BASE_URL",
        dialect: Dialect::CopilotChat,
        attribution: false,
    },
];

/// The provider names, for a message that has to list them.
fn provider_names() -> String {
    PROVIDERS
        .iter()
        .map(|p| p.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The headers OpenRouter reads to attribute a request to an application.
///
/// Only the providers that read them get them. They used to be built inside the
/// generic OpenAI-compatible client, which sent `X-OpenRouter-Title` to everybody
/// — real OpenAI included — because the headers lived with the dialect instead of
/// with the provider that asked for them.
///
/// `ATOMA_APP_*`, not `OPENAI_APP_*` as before: the value identifies this
/// application, and naming it after one vendor is what made it look like OpenAI's
/// business.
fn attribution_headers() -> Vec<(String, String)> {
    let name =
        std::env::var("ATOMA_APP_NAME").unwrap_or_else(|_| env!("CARGO_PKG_NAME").to_string());
    let url =
        std::env::var("ATOMA_APP_URL").unwrap_or_else(|_| env!("CARGO_PKG_REPOSITORY").to_string());
    vec![
        ("X-Title".to_string(), name.clone()),
        ("X-OpenRouter-Title".to_string(), name),
        ("HTTP-Referer".to_string(), url),
    ]
}

/// Which provider this run is for.
///
/// Takes "is this credential present" as a function rather than the credentials
/// themselves, because that is all the decision needs — and it makes the decision
/// testable without a credential source.
///
/// Priority: the agent definition's `provider:`, then `ATOMA_PROVIDER`, then the
/// credential that is present.
fn resolve_provider(
    provider_hint: Option<&str>,
    present: impl Fn(&str) -> bool,
) -> Result<&'static Provider> {
    if let Some(name) = provider_hint {
        return by_name(name);
    }
    if let Ok(name) = std::env::var("ATOMA_PROVIDER") {
        let name = name.trim();
        if !name.is_empty() {
            return by_name(name);
        }
    }
    detect(present)
}

fn by_name(name: &str) -> Result<&'static Provider> {
    PROVIDERS.iter().find(|p| p.name == name).with_context(|| {
        format!(
            "Unknown provider '{name}'. Valid values: {}",
            provider_names()
        )
    })
}

/// The provider whose credential is present.
///
/// Ambiguity is an error rather than a precedence, and that is deliberate. The
/// cascade this replaces answered "openai" whenever `OPENAI_API_KEY` was set, no
/// matter what else was, so a repository that had added a second provider's key got
/// the first one silently and found out from a bill or a 401. Naming both is worse
/// than nothing to guess from.
///
/// Two rows share `OPENAI_API_KEY` — the chat and Responses dialects of one vendor
/// — and the earlier row wins: Responses has to be asked for by name.
fn detect(present: impl Fn(&str) -> bool) -> Result<&'static Provider> {
    let mut found: Vec<&'static Provider> = Vec::new();
    for provider in PROVIDERS {
        if present(provider.credential)
            && !found.iter().any(|p| p.credential == provider.credential)
        {
            found.push(provider);
        }
    }

    match found.as_slice() {
        [] => anyhow::bail!(
            "No provider credential is set. Set one of {}, or name a provider with ATOMA_PROVIDER (one of: {}).",
            PROVIDERS
                .iter()
                .map(|p| p.credential)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", "),
            provider_names(),
        ),
        [only] => Ok(only),
        several => anyhow::bail!(
            "More than one provider credential is set ({}), so which one to use is not decided by \
             the credentials. Name the provider with ATOMA_PROVIDER or the agent definition's \
             `provider:` field, or remove the credentials this run should not use.",
            several
                .iter()
                .map(|p| p.credential)
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
}

/// The credential a provider needs, or an error naming it.
fn credential_for(provider: &Provider, credentials: &Credentials) -> Result<String> {
    credentials.get(provider.credential).with_context(|| {
        format!(
            "{} is not set, and the {} provider authenticates with it. Set it, or choose another \
             provider with ATOMA_PROVIDER (one of: {}).",
            provider.credential,
            provider.name,
            provider_names(),
        )
    })
}

/// Build an `LlmPort` implementation for this run.
pub async fn build_llm_client(
    provider_hint: Option<&str>,
    credentials: &Credentials,
) -> Result<Box<dyn LlmPort + Send + Sync>> {
    let raw_timeout = std::env::var("ATOMA_LLM_TIMEOUT").ok();
    let timeout_secs = resolve_timeout_secs(raw_timeout.as_deref());
    tracing::debug!("LLM request timeout: {}s", timeout_secs);

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent(format!(
            "{}/{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("Failed to create HTTP client")?;

    let provider = resolve_provider(provider_hint, |name| credentials.has(name))?;
    let base_url = std::env::var(provider.base_url_var)
        .unwrap_or_else(|_| provider.default_base_url.to_string());
    let headers = if provider.attribution {
        attribution_headers()
    } else {
        Vec::new()
    };

    // Logged together, because "which provider" without "reached where" is what
    // made the old default hard to see: every run said `openai` and none said
    // openrouter.ai.
    tracing::info!("LLM provider: {} at {}", provider.name, base_url);

    match provider.dialect {
        Dialect::OpenAiChat => Ok(Box::new(OpenAIClient::new(
            http,
            base_url,
            credential_for(provider, credentials)?,
            headers,
        ))),
        Dialect::OpenAiResponses => Ok(Box::new(OpenAIResponsesClient::new(
            http,
            base_url,
            credential_for(provider, credentials)?,
        ))),
        Dialect::AnthropicMessages => Ok(Box::new(AnthropicClient::new(
            http,
            base_url,
            credential_for(provider, credentials)?,
        ))),
        Dialect::CopilotChat => Ok(Box::new(
            CopilotClient::connect(http, base_url, credentials).await?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_defaults_when_unset() {
        assert_eq!(resolve_timeout_secs(None), DEFAULT_LLM_TIMEOUT_SECS);
    }

    #[test]
    fn timeout_reads_an_explicit_value() {
        assert_eq!(resolve_timeout_secs(Some("900")), 900);
        assert_eq!(resolve_timeout_secs(Some("  120 ")), 120);
    }

    #[test]
    fn timeout_rejects_zero_and_garbage() {
        for raw in ["0", "", "abc", "-5", "12.5"] {
            assert_eq!(
                resolve_timeout_secs(Some(raw)),
                DEFAULT_LLM_TIMEOUT_SECS,
                "{raw:?} should fall back to the default"
            );
        }
    }

    /// A row per provider is only an improvement if the rows cannot contradict each
    /// other, so the table's own invariants are checked rather than assumed.
    #[test]
    fn the_table_holds_together() {
        for provider in PROVIDERS {
            assert!(!provider.name.is_empty());
            assert!(!provider.credential.is_empty());
            assert!(
                provider.default_base_url.starts_with("https://"),
                "{} points at {}",
                provider.name,
                provider.default_base_url
            );
            assert!(
                provider.base_url_var.ends_with("_BASE_URL"),
                "{} overrides its endpoint with {}",
                provider.name,
                provider.base_url_var
            );
        }

        let names: std::collections::BTreeSet<_> = PROVIDERS.iter().map(|p| p.name).collect();
        assert_eq!(names.len(), PROVIDERS.len(), "two rows share a name");
    }

    /// One vendor reached two ways has to be reached at one place. When the two
    /// clients' defaults drifted apart, an OpenRouter key went to OpenAI and came
    /// back `401 invalid_api_key` — which reads like a bad secret and was a bad
    /// default. Rows, not constants in two files, but the same invariant.
    #[test]
    fn rows_sharing_a_credential_agree_on_where_it_is_sent() {
        for a in PROVIDERS {
            for b in PROVIDERS {
                if a.credential == b.credential {
                    assert_eq!(
                        a.default_base_url, b.default_base_url,
                        "{} and {} share {} but not an endpoint",
                        a.name, b.name, a.credential
                    );
                    assert_eq!(
                        a.base_url_var, b.base_url_var,
                        "{} and {} share {} but not the variable that moves it",
                        a.name, b.name, a.credential
                    );
                }
            }
        }
    }

    /// The bug this whole change is about: `openai` pointed at OpenRouter.
    #[test]
    fn openai_means_openai_and_the_routers_have_their_own_names() {
        assert_eq!(
            by_name("openai").unwrap().default_base_url,
            "https://api.openai.com/v1"
        );
        assert_eq!(
            by_name("openrouter").unwrap().default_base_url,
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(
            by_name("orcarouter").unwrap().default_base_url,
            "https://api.orcarouter.ai/v1"
        );
    }

    /// The attribution headers belong to the providers that read them, not to the
    /// dialect they happen to share with everyone else.
    #[test]
    fn only_openrouter_asks_for_attribution() {
        for provider in PROVIDERS {
            assert_eq!(
                provider.attribution,
                provider.name == "openrouter",
                "{} disagrees about attribution headers",
                provider.name
            );
        }
    }

    #[test]
    fn an_unknown_name_lists_the_known_ones() {
        let error = by_name("openai-compatible").unwrap_err().to_string();
        assert!(error.contains("openrouter"), "{error}");
        assert!(error.contains("orcarouter"), "{error}");
    }

    #[test]
    fn one_credential_decides() {
        let provider = detect(|name| name == "ORCAROUTER_API_KEY").unwrap();
        assert_eq!(provider.name, "orcarouter");
    }

    /// Two rows share `OPENAI_API_KEY`; the chat dialect is the one you get without
    /// asking, and `openai-responses` has to be named.
    #[test]
    fn the_shared_credential_resolves_to_the_chat_dialect() {
        let provider = detect(|name| name == "OPENAI_API_KEY").unwrap();
        assert_eq!(provider.name, "openai");
        assert_eq!(provider.dialect, Dialect::OpenAiChat);
    }

    #[test]
    fn two_credentials_are_an_error_rather_than_a_precedence() {
        let error = detect(|name| matches!(name, "OPENAI_API_KEY" | "ANTHROPIC_API_KEY"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("OPENAI_API_KEY"), "{error}");
        assert!(error.contains("ANTHROPIC_API_KEY"), "{error}");
        assert!(error.contains("ATOMA_PROVIDER"), "{error}");
    }

    #[test]
    fn no_credential_says_what_to_set() {
        let error = detect(|_| false).unwrap_err().to_string();
        assert!(error.contains("OPENROUTER_API_KEY"), "{error}");
        assert!(error.contains("ANTHROPIC_API_KEY"), "{error}");
    }

    /// A hint beats both the environment and the credentials, because it comes from
    /// the agent definition: an agent that names its provider means it.
    #[test]
    fn a_hint_wins() {
        let provider = resolve_provider(Some("anthropic"), |_| true).unwrap();
        assert_eq!(provider.name, "anthropic");
    }
}
