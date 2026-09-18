pub mod anthropic;
pub mod copilot;
pub mod openai;
pub mod openai_responses;
pub(crate) mod shared;

use anyhow::{Context, Result};
use async_trait::async_trait;
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
/// The rule lives in `infra::timeouts` now, because three other places read a timeout
/// from the environment and only this one trimmed its input or refused zero.
fn resolve_timeout_secs(raw: Option<&str>) -> u64 {
    crate::infra::timeouts::seconds_from(raw, DEFAULT_LLM_TIMEOUT_SECS)
}

/// Everything Atoma knows about one provider.
///
/// One interface, one instance per provider, the instances in `PROVIDERS`. A provider
/// is added by adding a line there and removed by deleting one; nothing else in this
/// file names any provider in particular.
///
/// Two arrangements preceded this. Four constructors that each read their own
/// environment variable with their own default — which is how `github-copilot` came to
/// have no configurable endpoint with nothing saying whether that was deliberate, and
/// how one router's attribution headers came to be sent to every provider. Then a data
/// table with an enum that `build_llm_client` matched on, which put "which client does
/// this one build" back into shared control flow.
///
/// The defaults below are the whole of what is common: an endpoint is a default plus
/// the variable that moves it, and a credential is a name looked up with an error that
/// says which provider wanted it. Anything a provider does differently, it does in its
/// own implementation.
#[async_trait]
pub trait Provider: Sync + std::fmt::Debug {
    /// The name in `ATOMA_PROVIDER`, and in an agent definition's `provider:`.
    fn name(&self) -> &'static str;

    /// The credential this provider authenticates with.
    ///
    /// One provider, one name, which is what makes auto-detection an answer rather
    /// than a guess. `OPENAI_API_KEY` used to authenticate anything speaking OpenAI's
    /// dialect, OpenRouter included, so the name said nothing about where the key
    /// would be sent.
    fn credential(&self) -> &'static str;

    /// Where its requests go, unless an operator says otherwise.
    fn default_base_url(&self) -> &'static str;

    /// What says otherwise. Every provider has one; none is a special case.
    fn base_url_var(&self) -> &'static str;

    /// Headers this provider wants that no other should receive.
    ///
    /// On the trait rather than on one dialect's struct. It was a field on
    /// `ChatCompletions` alone, so `openrouter` was attributed and
    /// `openrouter-responses` was not -- picking the newer dialect dropped the run's
    /// identity, OpenRouter's dashboard stopped naming the app, and nothing failed or
    /// said so. Attribution is a property of who is being asked, not of which of two
    /// wire formats the asking uses.
    ///
    /// Empty for most, which is an answer rather than an omission: the generic client
    /// used to attach OpenRouter's headers unconditionally, so real OpenAI received
    /// `X-OpenRouter-Title` too.
    fn headers(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// The wire format, for the log line and for tests.
    ///
    /// An endpoint alone leaves "is this host being asked for `/chat/completions` or
    /// `/responses`" unanswered, which is what an operator pointing a base URL at
    /// their own gateway needs to know.
    fn dialect(&self) -> &'static str;

    /// Build the client. How this provider is reached is entirely in here.
    async fn connect(
        &self,
        http: reqwest::Client,
        credentials: &Credentials,
    ) -> Result<Box<dyn LlmPort + Send + Sync>>;

    /// Every credential name this provider might read, not only the one it advertises.
    ///
    /// The two differ for exactly one reason and it matters: `credential` is what
    /// auto-detection keys on, and this is what has to be kept out of a tool server's
    /// environment. Copilot reads three names and advertises one, so a list built from
    /// `credential` alone would leave the other two inheritable — the failure just fixed
    /// for the router keys, one level down.
    fn credential_names(&self) -> Vec<&'static str> {
        vec![self.credential()]
    }

    /// The endpoint in effect: the default, or what the environment replaced it with.
    fn base_url(&self) -> String {
        std::env::var(self.base_url_var()).unwrap_or_else(|_| self.default_base_url().to_string())
    }

    /// The credential's value, or an error naming what to set and for whom.
    fn api_key(&self, credentials: &Credentials) -> Result<String> {
        credentials
            .get(self.credential())
            .with_context(|| credential_missing(self.credential(), self.name()))
    }
}

/// A provider reached over OpenAI's chat-completions dialect.
///
/// Three providers are this and differ only in a name, a credential, a host and which
/// headers they read — so they are three instances rather than three implementations.
/// One that needed to do something differently would be its own type, the way
/// `Anthropic` and `GitHubCopilot` are.
#[derive(Debug)]
struct ChatCompletions {
    name: &'static str,
    credential: &'static str,
    default_base_url: &'static str,
    base_url_var: &'static str,
    /// Headers this provider wants that no other should receive.
    ///
    /// `no_headers` for most, which is an answer rather than an omission. The generic
    /// client used to attach OpenRouter's attribution headers unconditionally, so real
    /// OpenAI received `X-OpenRouter-Title` too.
    headers: fn() -> Vec<(String, String)>,
}

#[async_trait]
impl Provider for ChatCompletions {
    fn name(&self) -> &'static str {
        self.name
    }
    fn credential(&self) -> &'static str {
        self.credential
    }
    fn default_base_url(&self) -> &'static str {
        self.default_base_url
    }
    fn base_url_var(&self) -> &'static str {
        self.base_url_var
    }
    fn headers(&self) -> Vec<(String, String)> {
        (self.headers)()
    }
    fn dialect(&self) -> &'static str {
        "chat-completions"
    }

    async fn connect(
        &self,
        http: reqwest::Client,
        credentials: &Credentials,
    ) -> Result<Box<dyn LlmPort + Send + Sync>> {
        Ok(Box::new(OpenAIClient::new(
            http,
            self.base_url(),
            self.api_key(credentials)?,
            Provider::headers(self),
        )))
    }
}

/// A provider reached over OpenAI's Responses API.
///
/// Its own type rather than a flag on the one above, because what differs is a URL
/// path, a request shape and a response shape — code, not data.
#[derive(Debug)]
struct Responses {
    name: &'static str,
    credential: &'static str,
    default_base_url: &'static str,
    base_url_var: &'static str,
    /// See `Provider::headers`. Present here for the same reason it is present on
    /// `ChatCompletions`: the router behind this dialect still wants to be told who
    /// is asking.
    headers: fn() -> Vec<(String, String)>,
}

#[async_trait]
impl Provider for Responses {
    fn name(&self) -> &'static str {
        self.name
    }
    fn credential(&self) -> &'static str {
        self.credential
    }
    fn default_base_url(&self) -> &'static str {
        self.default_base_url
    }
    fn base_url_var(&self) -> &'static str {
        self.base_url_var
    }
    fn headers(&self) -> Vec<(String, String)> {
        (self.headers)()
    }
    fn dialect(&self) -> &'static str {
        "responses"
    }

    async fn connect(
        &self,
        http: reqwest::Client,
        credentials: &Credentials,
    ) -> Result<Box<dyn LlmPort + Send + Sync>> {
        Ok(Box::new(OpenAIResponsesClient::new(
            http,
            self.base_url(),
            self.api_key(credentials)?,
            Provider::headers(self),
        )))
    }
}

/// Anthropic, over its own Messages format.
#[derive(Debug)]
struct Anthropic;

#[async_trait]
impl Provider for Anthropic {
    fn name(&self) -> &'static str {
        "anthropic"
    }
    fn credential(&self) -> &'static str {
        "ANTHROPIC_API_KEY"
    }
    fn default_base_url(&self) -> &'static str {
        "https://api.anthropic.com"
    }
    fn base_url_var(&self) -> &'static str {
        "ANTHROPIC_BASE_URL"
    }
    fn dialect(&self) -> &'static str {
        "anthropic-messages"
    }

    async fn connect(
        &self,
        http: reqwest::Client,
        credentials: &Credentials,
    ) -> Result<Box<dyn LlmPort + Send + Sync>> {
        Ok(Box::new(AnthropicClient::new(
            http,
            self.base_url(),
            self.api_key(credentials)?,
        )))
    }
}

/// GitHub Copilot: chat completions, reached with a token exchanged from a GitHub
/// credential.
///
/// Its own type because three of its rules are its own — the exchange, the four
/// headers its endpoint requires, and accepting a GitHub token under three names. All
/// three used to sit inside its client and its constructor, where they read as
/// exceptions to something rather than as this provider's own behaviour.
#[derive(Debug)]
struct GitHubCopilot;

/// What GitHub Copilot is told about the client asking.
///
/// A function rather than a literal inside `connect`, because one of these is a
/// contract and a test is the only thing that can watch it.
fn copilot_headers() -> Vec<(String, String)> {
    let pkg_id = format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    vec![
        // The executable, which is this one. These are a version statement and
        // nothing is gated on them.
        ("Editor-Version".to_string(), pkg_id.clone()),
        ("Editor-Plugin-Version".to_string(), pkg_id),
        // NOT a name for this application: `Copilot-Integration-Id` selects an API
        // contract, and with it which models the account may reach. It was
        // `atoma-cli`, which is not registered with GitHub, and an unregistered id
        // is not a smaller entitlement but an unknown one -- the failure it produces
        // is `The requested model is not supported`, on models that work elsewhere.
        //
        // `copilot-developer-cli` is the documented default for a client that has
        // not registered a branded id, which is this one. Measured by somebody with
        // a real token: 44 models under this id, 39 with `vscode-chat` and 39 with
        // the header absent. So it is also the widest catalogue available without
        // claiming to be a product we are not.
        //
        // Register `atoma` with GitHub and this becomes that instead. Until then,
        // naming ourselves here buys nothing and costs models.
        (
            "Copilot-Integration-Id".to_string(),
            "copilot-developer-cli".to_string(),
        ),
        (
            "Openai-Intent".to_string(),
            "conversation-panel".to_string(),
        ),
    ]
}
#[async_trait]
impl Provider for GitHubCopilot {
    fn name(&self) -> &'static str {
        "github-copilot"
    }

    /// The one name auto-detection may read.
    ///
    /// It accepts two more in `api_key`, and deliberately does not advertise them
    /// here: a run that talks to GitHub holds `GH_TOKEN` anyway, so detecting Copilot
    /// from that would make every such run ambiguous.
    fn credential(&self) -> &'static str {
        "ATOMA_COPILOT_TOKEN"
    }
    fn default_base_url(&self) -> &'static str {
        "https://api.githubcopilot.com"
    }
    fn base_url_var(&self) -> &'static str {
        "COPILOT_BASE_URL"
    }
    fn dialect(&self) -> &'static str {
        "copilot-chat"
    }

    /// All three, so the union that keeps credentials out of tool servers covers them.
    fn credential_names(&self) -> Vec<&'static str> {
        vec![self.credential(), "GITHUB_TOKEN", "GH_TOKEN"]
    }

    fn api_key(&self, credentials: &Credentials) -> Result<String> {
        credentials
            .get(self.credential())
            .or_else(|| credentials.get("GITHUB_TOKEN"))
            .or_else(|| credentials.get("GH_TOKEN"))
            .context(
                "ATOMA_COPILOT_TOKEN, GITHUB_TOKEN, or GH_TOKEN is required for the \
                 github-copilot provider",
            )
    }

    async fn connect(
        &self,
        http: reqwest::Client,
        credentials: &Credentials,
    ) -> Result<Box<dyn LlmPort + Send + Sync>> {
        let headers = copilot_headers();
        Ok(Box::new(
            CopilotClient::connect(http, self.base_url(), headers, self.api_key(credentials)?)
                .await?,
        ))
    }
}

/// A provider that asks for nothing beyond the credential.
fn no_headers() -> Vec<(String, String)> {
    Vec::new()
}

/// What this application calls itself, for a router that attributes requests.
///
/// `ATOMA_APP_*`, not `OPENAI_APP_*` as before: the value identifies this application,
/// and naming it after one vendor is what made it look like OpenAI's business.
fn app_identity() -> (String, String) {
    let name =
        std::env::var("ATOMA_APP_NAME").unwrap_or_else(|_| env!("CARGO_PKG_NAME").to_string());
    let url =
        std::env::var("ATOMA_APP_URL").unwrap_or_else(|_| env!("CARGO_PKG_REPOSITORY").to_string());
    (name, url)
}

/// The two headers a router reads to attribute a request to an application.
///
/// Both routers in this table read exactly these, and they are the whole of what a
/// router needs: a name to show and a URL to link. Anything beyond them is one
/// vendor's own, and belongs to that vendor's function rather than to this one --
/// which is the shape the table already wanted, since sending a vendor's header to
/// somebody else is the mistake it was built to stop.
fn router_attribution() -> Vec<(String, String)> {
    let (name, url) = app_identity();
    vec![
        ("X-Title".to_string(), name),
        ("HTTP-Referer".to_string(), url),
    ]
}

/// OpenRouter's: the two above, and one of its own.
fn openrouter_attribution() -> Vec<(String, String)> {
    let (name, _) = app_identity();
    let mut headers = router_attribution();
    headers.push(("X-OpenRouter-Title".to_string(), name));
    headers
}

/// Every provider Atoma speaks to. One line each.
///
/// `openai` means OpenAI. It used to default to `https://openrouter.ai/api/v1`, so the
/// name pointed somewhere other than it said, and the Responses client carried a
/// comment explaining that it had to keep the same wrong default or an OpenRouter key
/// would reach OpenAI and come back `401 invalid_api_key`. The routers have their own
/// names now, and each dialect they serve has one, so a run's log states where the
/// request went rather than leaving it to be inferred from a variable.
///
/// The `openai` pair is also how to reach a provider with no line of its own: point
/// `OPENAI_BASE_URL` at anything speaking either dialect.
static PROVIDERS: &[&dyn Provider] = &[
    &ChatCompletions {
        name: "openai",
        credential: "OPENAI_API_KEY",
        default_base_url: "https://api.openai.com/v1",
        base_url_var: "OPENAI_BASE_URL",
        headers: no_headers,
    },
    &Responses {
        name: "openai-responses",
        credential: "OPENAI_API_KEY",
        default_base_url: "https://api.openai.com/v1",
        base_url_var: "OPENAI_BASE_URL",
        headers: no_headers,
    },
    &ChatCompletions {
        name: "openrouter",
        credential: "OPENROUTER_API_KEY",
        default_base_url: "https://openrouter.ai/api/v1",
        base_url_var: "OPENROUTER_BASE_URL",
        headers: openrouter_attribution,
    },
    &Responses {
        name: "openrouter-responses",
        credential: "OPENROUTER_API_KEY",
        default_base_url: "https://openrouter.ai/api/v1",
        base_url_var: "OPENROUTER_BASE_URL",
        headers: openrouter_attribution,
    },
    &ChatCompletions {
        name: "orcarouter",
        credential: "ORCAROUTER_API_KEY",
        default_base_url: "https://api.orcarouter.ai/v1",
        base_url_var: "ORCAROUTER_BASE_URL",
        headers: router_attribution,
    },
    &Responses {
        name: "orcarouter-responses",
        credential: "ORCAROUTER_API_KEY",
        default_base_url: "https://api.orcarouter.ai/v1",
        base_url_var: "ORCAROUTER_BASE_URL",
        headers: router_attribution,
    },
    &Anthropic,
    &GitHubCopilot,
];

/// The providers, as the CLI's help text describes them.
///
/// Rendered from the list rather than written a second time. `cli.rs` used to carry
/// its own copy of every provider's name, credential and endpoint variable — the same
/// facts, in the same crate, with nothing keeping them in step. A provider added here
/// silently went undocumented, and one renamed left the help naming something that no
/// longer existed.
pub fn describe_providers() -> String {
    let mut out = String::new();
    for provider in PROVIDERS {
        out.push_str(&format!(
            "  {:<22} {} ({}), at {} unless {} says otherwise\n",
            provider.credential(),
            provider.name(),
            provider.dialect(),
            provider.default_base_url(),
            provider.base_url_var(),
        ));
    }
    out
}

/// Every name a provider authenticates with.
///
/// Published because something else has to know it: the names a tool server must not
/// inherit are these plus the GitHub tokens, and that union used to be a second
/// hand-written list. It was already wrong -- `OPENROUTER_API_KEY` and
/// `ORCAROUTER_API_KEY` were added here and not there, so in environment mode a tool
/// server inherited them and could read a provider key straight out of its own
/// environment.
/// Whether a name is a provider this build knows.
///
/// For `atoma validate`, and deliberately nothing more than the name. A definition
/// naming a provider that does not exist and a credential that is not set are
/// different facts: only the first is a defect in the definition, and a check that
/// conflated them would fail every validation run, which have no credentials.
///
/// Goes through `by_name` rather than comparing against a list of its own, so the
/// error a pull request sees is the error the run would have produced -- including
/// the candidates, which is the part that makes it actionable.
pub fn check_provider_name(name: &str) -> Result<()> {
    by_name(PROVIDERS, name).map(|_| ())
}

pub fn provider_credential_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = PROVIDERS
        .iter()
        .flat_map(|p| p.credential_names())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// The provider names, for a message that has to list them.
fn provider_names() -> String {
    PROVIDERS
        .iter()
        .map(|p| p.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What to say when the provider a run resolved to has no credential.
///
/// One place, because two callers need the same sentence: `api_key`, reached once a
/// run has already started, and `check_credentials`, whose whole purpose is to say it
/// before one does. Two copies would drift, and the copy a person reads is whichever
/// one they happened to hit.
fn credential_missing(credential: &str, provider: &str) -> String {
    format!(
        "{credential} is not set, and the {provider} provider authenticates with it. Set it, \
         or choose another provider with ATOMA_PROVIDER (one of: {}).",
        provider_names(),
    )
}

/// Whether the credential this run needs is among the ones that are set.
///
/// Names, never values. `resolve_provider` already takes presence as a function
/// because that is all the decision needs, and this is the same decision asked one
/// step earlier -- before a runner has checked out, installed and started tool
/// servers, rather than after. So a caller can answer it holding no secret: a
/// workflow step can test whether a secret is empty without materialising it, and a
/// check that demanded the values would put them in the one step with no use for them.
///
/// Deliberately not part of `validate`. That command asks whether a definition is
/// correct and runs where no credential exists -- a pull request checking a file --
/// and `a_known_provider_passes_without_its_credential` fixes that judgement. This is
/// a different question, asked by whoever is about to run the definition.
pub fn check_credentials(provider_hint: Option<&str>, present: &[String]) -> Result<String> {
    let provider = credentials_checked(PROVIDERS, provider_hint, present)?;
    Ok(format!(
        "{} is set, and the {} provider authenticates with it",
        provider.credential(),
        provider.name()
    ))
}

/// The list is supplied rather than reached for, as everywhere else in this file.
fn credentials_checked<'a>(
    providers: &'a [&'a dyn Provider],
    provider_hint: Option<&str>,
    present: &[String],
) -> Result<&'a dyn Provider> {
    let provider = resolve_provider(providers, provider_hint, |name| {
        present.iter().any(|set| set.as_str() == name)
    })?;
    if present.iter().any(|set| set.as_str() == provider.credential()) {
        return Ok(provider);
    }
    anyhow::bail!("{}", credential_missing(provider.credential(), provider.name()))
}

/// Which provider this run is for.
///
/// Takes the list, so a caller supplies it rather than this function reaching for a
/// global — and takes "is this credential present" as a function, because that is all
/// the decision needs.
///
/// Priority: the agent definition's `provider:`, then `ATOMA_PROVIDER`, then the
/// credential that is present.
fn resolve_provider<'a>(
    providers: &'a [&'a dyn Provider],
    provider_hint: Option<&str>,
    present: impl Fn(&str) -> bool,
) -> Result<&'a dyn Provider> {
    if let Some(name) = provider_hint {
        return by_name(providers, name);
    }
    if let Ok(name) = std::env::var("ATOMA_PROVIDER") {
        let name = name.trim();
        if !name.is_empty() {
            return by_name(providers, name);
        }
    }
    detect(providers, present)
}

fn by_name<'a>(providers: &'a [&'a dyn Provider], name: &str) -> Result<&'a dyn Provider> {
    providers
        .iter()
        .copied()
        .find(|p| p.name() == name)
        .with_context(|| {
            format!(
                "Unknown provider '{name}'. Valid values: {}",
                providers
                    .iter()
                    .map(|p| p.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// The provider whose credential is present.
///
/// Ambiguity is an error rather than a precedence, deliberately. The cascade this
/// replaces answered "openai" whenever `OPENAI_API_KEY` was set, no matter what else
/// was, so a repository that added a second provider's key got the first one silently
/// and found out from a 401 or a bill.
///
/// Two providers can share a credential — one vendor reached by two dialects — and
/// then the earlier line wins: the Responses one has to be asked for by name.
fn detect<'a>(
    providers: &'a [&'a dyn Provider],
    present: impl Fn(&str) -> bool,
) -> Result<&'a dyn Provider> {
    let mut found: Vec<&'a dyn Provider> = Vec::new();
    for provider in providers.iter().copied() {
        if present(provider.credential())
            && !found
                .iter()
                .any(|p| p.credential() == provider.credential())
        {
            found.push(provider);
        }
    }

    match found.as_slice() {
        [] => anyhow::bail!(
            "No provider credential is set. Set one of {}, or name a provider with ATOMA_PROVIDER \
             (one of: {}).",
            providers
                .iter()
                .map(|p| p.credential())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", "),
            providers
                .iter()
                .map(|p| p.name())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        [only] => Ok(*only),
        several => anyhow::bail!(
            "More than one provider credential is set ({}), so which one to use is not decided by \
             the credentials. Name the provider with ATOMA_PROVIDER or the agent definition's \
             `provider:` field, or remove the credentials this run should not use.",
            several
                .iter()
                .map(|p| p.credential())
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
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

    let provider = resolve_provider(PROVIDERS, provider_hint, |name| credentials.has(name))?;

    // All three together, because any one of them alone leaves the question the old
    // default made unanswerable. The name without the host is what hid an `openai`
    // that meant OpenRouter. The host without the dialect leaves "does this endpoint
    // serve /responses" to be guessed at.
    tracing::info!(
        "LLM provider: {} ({}) at {}",
        provider.name(),
        provider.dialect(),
        provider.base_url()
    );

    provider.connect(http, credentials).await
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

    /// A line per provider is only an improvement if the lines cannot contradict each
    /// other, so what every provider must answer the same way is checked here.
    #[test]
    fn every_provider_answers_the_same_questions() {
        for provider in PROVIDERS {
            assert!(!provider.name().is_empty());
            assert!(!provider.credential().is_empty());
            assert!(
                provider.default_base_url().starts_with("https://"),
                "{} points at {}",
                provider.name(),
                provider.default_base_url()
            );
            assert!(
                provider.base_url_var().ends_with("_BASE_URL"),
                "{} moves its endpoint with {}",
                provider.name(),
                provider.base_url_var()
            );
            assert!(!provider.dialect().is_empty());
        }

        let names: std::collections::BTreeSet<_> = PROVIDERS.iter().map(|p| p.name()).collect();
        assert_eq!(names.len(), PROVIDERS.len(), "two lines share a name");
    }

    /// One vendor reached two ways has to be reached at one place. When the two
    /// clients' defaults drifted apart, an OpenRouter key went to OpenAI and came back
    /// `401 invalid_api_key` — which reads like a bad secret and was a bad default.
    #[test]
    fn providers_sharing_a_credential_agree_on_where_it_is_sent() {
        for a in PROVIDERS {
            for b in PROVIDERS {
                if a.credential() == b.credential() {
                    assert_eq!(
                        a.default_base_url(),
                        b.default_base_url(),
                        "{} and {} share {} but not an endpoint",
                        a.name(),
                        b.name(),
                        a.credential()
                    );
                    assert_eq!(
                        a.base_url_var(),
                        b.base_url_var(),
                        "{} and {} share {} but not the variable that moves it",
                        a.name(),
                        b.name(),
                        a.credential()
                    );
                }
            }
        }
    }

    /// The bug this whole arrangement is about: `openai` pointed at OpenRouter.
    #[test]
    fn openai_means_openai_and_the_routers_have_their_own_names() {
        assert_eq!(
            by_name(PROVIDERS, "openai").unwrap().default_base_url(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            by_name(PROVIDERS, "openrouter").unwrap().default_base_url(),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(
            by_name(PROVIDERS, "orcarouter").unwrap().default_base_url(),
            "https://api.orcarouter.ai/v1"
        );
    }

    /// Both routers serve both dialects, and each combination is its own line rather
    /// than a base URL somebody has to know about.
    #[test]
    fn each_router_offers_both_dialects_under_its_own_name() {
        for (chat, responses) in [
            ("openrouter", "openrouter-responses"),
            ("orcarouter", "orcarouter-responses"),
        ] {
            let a = by_name(PROVIDERS, chat).unwrap();
            let b = by_name(PROVIDERS, responses).unwrap();
            assert_eq!(a.dialect(), "chat-completions");
            assert_eq!(b.dialect(), "responses");
            assert_eq!(a.credential(), b.credential());
            assert_eq!(a.default_base_url(), b.default_base_url());
        }
    }

    #[test]
    fn an_unknown_name_lists_the_known_ones() {
        let error = by_name(PROVIDERS, "openai-compatible")
            .unwrap_err()
            .to_string();
        assert!(error.contains("openrouter"), "{error}");
        assert!(error.contains("orcarouter"), "{error}");
    }

    #[test]
    fn one_credential_decides() {
        let provider = detect(PROVIDERS, |name| name == "ORCAROUTER_API_KEY").unwrap();
        assert_eq!(provider.name(), "orcarouter");
    }

    /// Two providers share `OPENAI_API_KEY`; the chat dialect is the one you get
    /// without asking, and `openai-responses` has to be named.
    #[test]
    fn the_shared_credential_resolves_to_the_chat_dialect() {
        let provider = detect(PROVIDERS, |name| name == "OPENAI_API_KEY").unwrap();
        assert_eq!(provider.name(), "openai");
        assert_eq!(provider.dialect(), "chat-completions");
    }

    #[test]
    fn two_credentials_are_an_error_rather_than_a_precedence() {
        let error = detect(PROVIDERS, |name| {
            matches!(name, "OPENAI_API_KEY" | "ANTHROPIC_API_KEY")
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("OPENAI_API_KEY"), "{error}");
        assert!(error.contains("ANTHROPIC_API_KEY"), "{error}");
        assert!(error.contains("ATOMA_PROVIDER"), "{error}");
    }

    #[test]
    fn no_credential_says_what_to_set() {
        let error = detect(PROVIDERS, |_| false).unwrap_err().to_string();
        assert!(error.contains("OPENROUTER_API_KEY"), "{error}");
        assert!(error.contains("ANTHROPIC_API_KEY"), "{error}");
    }

    /// A hint beats both the environment and the credentials, because it comes from
    /// the agent definition: an agent that names its provider means it.
    #[test]
    fn a_hint_wins() {
        let provider = resolve_provider(PROVIDERS, Some("anthropic"), |_| true).unwrap();
        assert_eq!(provider.name(), "anthropic");
    }

    /// The list is a parameter, so a caller can supply its own. Nothing in the
    /// resolution logic knows which providers exist.
    #[test]
    fn the_provider_list_is_supplied_not_reached_for() {
        static ONLY_ONE: &[&dyn Provider] = &[&Anthropic];

        assert_eq!(
            detect(ONLY_ONE, |name| name == "ANTHROPIC_API_KEY")
                .unwrap()
                .name(),
            "anthropic"
        );
        let error = by_name(ONLY_ONE, "openrouter").unwrap_err().to_string();
        assert!(error.contains("Valid values: anthropic"), "{error}");
    }

    /// The help text is generated, so this checks the rendering rather than a second
    /// copy of the facts: every provider has to appear in it, with the two names an
    /// operator has to type.
    #[test]
    fn the_help_text_describes_every_provider() {
        let help = describe_providers();
        for provider in PROVIDERS {
            assert!(
                help.contains(provider.name()),
                "{} is missing from the help",
                provider.name()
            );
            assert!(help.contains(provider.credential()), "{}", provider.name());
            assert!(
                help.contains(provider.base_url_var()),
                "{}",
                provider.name()
            );
        }
    }

    /// The union that keeps provider keys out of tool servers is built from this, so
    /// a provider added above is covered without anyone remembering a second list.
    /// The check a runner makes before it installs anything.
    ///
    /// Names go in and a provider comes out; no value is passed and none is returned.
    /// The caller is a workflow step that can say whether a secret is empty without
    /// materialising it, and it would have no other use for the value.
    #[test]
    fn a_present_credential_passes_the_early_check() {
        let present = vec!["ORCAROUTER_API_KEY".to_string()];
        let provider = credentials_checked(PROVIDERS, Some("orcarouter-responses"), &present)
            .expect("the credential it names is present");
        assert_eq!(provider.name(), "orcarouter-responses");
        assert_eq!(provider.credential(), "ORCAROUTER_API_KEY");
    }

    /// The whole point: the name to add, before a runner is spent finding out.
    ///
    /// Asserted against `credential_missing` rather than against a quoted sentence,
    /// because the property that matters is that this is the SAME sentence a run
    /// produces. A person who has seen one has to recognise the other, and a copy here
    /// would pass this test while drifting from what the run says.
    #[test]
    fn a_missing_credential_names_the_secret_to_add() {
        let present = vec!["OPENAI_API_KEY".to_string()];
        let error = credentials_checked(PROVIDERS, Some("orcarouter-responses"), &present)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            credential_missing("ORCAROUTER_API_KEY", "orcarouter-responses")
        );
        assert!(error.contains("ORCAROUTER_API_KEY"), "{error}");
    }

    /// Detection is not re-tested here: the early check reaches it through
    /// `resolve_provider`, which is the run's own function, and the tests above cover
    /// what it answers for none and for several. Re-testing it through this door would
    /// read `ATOMA_PROVIDER` from the environment, which is what those tests avoid by
    /// calling `detect` directly.
    #[test]
    fn the_early_check_carries_no_credential_value_in_its_answer() {
        let present = vec!["ANTHROPIC_API_KEY".to_string()];
        let message = check_credentials(Some("anthropic"), &present).expect("present");
        assert!(message.contains("ANTHROPIC_API_KEY"), "{message}");
        // The name, and nothing that could be a value: this string is printed.
        assert!(!message.contains("sk-"), "{message}");
    }
    #[test]
    fn every_provider_credential_is_published() {
        let names = provider_credential_names();
        for provider in PROVIDERS {
            assert!(
                names.contains(&provider.credential()),
                "{} authenticates with {}, which nothing would strip",
                provider.name(),
                provider.credential()
            );
        }
        assert!(names.contains(&"OPENROUTER_API_KEY"), "{names:?}");
        assert!(names.contains(&"ORCAROUTER_API_KEY"), "{names:?}");
    }

    /// A provider that READS a name it does not advertise has to publish it anyway, or
    /// the union that strips credentials from tool servers leaves it inheritable.
    /// Copilot is the one that does this, and it did.
    #[test]
    fn a_credential_a_provider_only_falls_back_to_is_still_published() {
        let names = provider_credential_names();
        for name in ["ATOMA_COPILOT_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"] {
            assert!(names.contains(&name), "{name} is missing from {names:?}");
        }
    }

    /// Extra headers belong to the provider that reads them, not to the dialect it
    /// shares with everyone else. The generic client used to send
    /// `X-OpenRouter-Title` to real OpenAI for exactly that reason.
    #[test]
    fn attribution_headers_reach_only_the_providers_that_asked() {
        assert!(no_headers().is_empty());

        for name in ["openai", "openai-responses", "anthropic", "github-copilot"] {
            let provider = by_name(PROVIDERS, name).expect(name);
            assert!(provider.headers().is_empty(), "{name}");
        }

        let names: Vec<String> = openrouter_attribution()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(
            names.contains(&"X-OpenRouter-Title".to_string()),
            "{names:?}"
        );
        assert!(names.contains(&"HTTP-Referer".to_string()), "{names:?}");
    }

    /// The Copilot integration id is a contract, and a test is the only thing watching it.
    ///
    /// Nothing fails when this value is wrong. The account reaches fewer models, or one,
    /// with `The requested model is not supported` on the rest -- an error that reads as
    /// the model being unavailable rather than as the request being routed somewhere
    /// narrower. It was `atoma-cli`, which nothing had registered, for however long.
    ///
    /// The assertion is the reasoning rather than the string: this client has no
    /// registered branded id, so it sends the documented default for clients that have
    /// none. Registering one makes this test the place that says so.
    #[test]
    fn copilot_sends_the_documented_default_integration_id() {
        let headers = copilot_headers();
        let integration = headers
            .iter()
            .find(|(name, _)| name == "Copilot-Integration-Id")
            .expect("Copilot-Integration-Id is sent");
        assert_eq!(integration.1, "copilot-developer-cli");

        // The version headers state the executable and are not the contract.
        for name in ["Editor-Version", "Editor-Plugin-Version"] {
            let value = headers
                .iter()
                .find(|(header, _)| header == name)
                .map(|(_, value)| value.as_str())
                .unwrap_or_default();
            assert!(value.starts_with(env!("CARGO_PKG_NAME")), "{name}: {value}");
        }
    }
    /// What every router reads, and what only one of them does.
    ///
    /// `X-Title` and `HTTP-Referer` are the convention; `X-OpenRouter-Title` is
    /// OpenRouter's own. Giving the second router all three would repeat, one table row
    /// over, the mistake of sending a vendor's header to somebody who never asked.
    #[test]
    fn a_vendors_own_header_goes_only_to_that_vendor() {
        let shared: Vec<String> = router_attribution()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(shared, vec!["X-Title", "HTTP-Referer"]);

        for name in ["orcarouter", "orcarouter-responses"] {
            let sent: Vec<String> = by_name(PROVIDERS, name)
                .expect(name)
                .headers()
                .into_iter()
                .map(|(header, _)| header)
                .collect();
            assert_eq!(sent, shared, "{name}");
        }
    }

    /// The two dialects of one provider are attributed the same.
    ///
    /// `headers` was a field on the chat-completions struct alone, so `openrouter`
    /// was attributed and `openrouter-responses` was not. Choosing the newer dialect
    /// dropped the run's identity -- no error, no log line, just an empty App column
    /// on the router's dashboard, which is the kind of thing found weeks later by
    /// somebody wondering where their requests went.
    #[test]
    fn a_dialect_pair_is_attributed_the_same() {
        for (chat, responses) in [("openrouter", "openrouter-responses")] {
            let one = by_name(PROVIDERS, chat).expect(chat);
            let other = by_name(PROVIDERS, responses).expect(responses);
            assert_eq!(one.headers(), other.headers(), "{chat} vs {responses}");
            assert!(!one.headers().is_empty(), "{chat}");
        }
    }
}
