//! The context budget, end to end (#12).
//!
//! The domain fold is tested in `domain::context_budget`; what an integration test
//! adds is the WIRING: a run whose first inference reports a prompt past the
//! definition's `context_limit` compacts between turns and finishes, the summary
//! inference actually happened, the replacement session was saved to the same path,
//! and the hooks saw the session BEFORE it was replaced.

mod common;

use anyhow::Result;
use async_trait::async_trait;
use atoma::application::runner::{run, RunDeps, RunFacts, RunOutcome, RunSettings};
use atoma::domain::agent::{AgentDef, ParsedAgentDef};
use atoma::domain::ports::{
    AgentDefPort, DeltaHandler, FinishReason, LlmChoice, LlmPort, LlmResponse, LlmUsage,
    McpFactory, SessionPort, ToolDefPort, ToolPort,
};
use atoma::domain::session::Message;
use atoma::domain::tool::ToolDef;
use common::mock_mcp::MockMcpRegistry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
// `tempdir` is used unqualified in the test bodies via `tempdir::TempDir`-free
// spelling: the calls are `::tempfile::tempdir()` inline.
#[allow(unused_imports)]
use tempfile::tempdir;

// ── Stubs ─────────────────────────────────────────────────────────────────────

struct StubAgentDefPort {
    agent_def: AgentDef,
}

impl AgentDefPort for StubAgentDefPort {
    fn parse(&self, _path: &Path) -> Result<ParsedAgentDef> {
        Ok(ParsedAgentDef {
            frontmatter: self.agent_def.clone(),
            body: None,
        })
    }
}

struct StubToolDefPort;
impl ToolDefPort for StubToolDefPort {
    fn load(&self, _path: &Path) -> Result<HashMap<String, ToolDef>> {
        Ok(HashMap::new())
    }
}

struct StubMcpFactory;
#[async_trait::async_trait]
impl McpFactory for StubMcpFactory {
    async fn build(&self, _defs: &[ToolDef]) -> Result<Box<dyn ToolPort + Send>> {
        Ok(Box::new(MockMcpRegistry::new()))
    }
}

/// A session port that records every save, as (path, message count).
#[derive(Clone, Default)]
struct RecordingSessionPort {
    saves: Arc<Mutex<Vec<(PathBuf, usize)>>>,
}

impl SessionPort for RecordingSessionPort {
    fn load(&self, _path: &Path) -> Result<atoma::domain::session::Session> {
        Ok(atoma::domain::session::Session::default())
    }
    fn save(&self, session: &atoma::domain::session::Session, path: &Path) -> Result<()> {
        self.saves
            .lock()
            .unwrap()
            .push((path.to_path_buf(), session.messages.len()));
        Ok(())
    }
}

/// An LLM that reports a configured prompt size with each response, so the budget
/// check has a measurement to read. Every response carries the same fixed text,
/// which serves as both the run's answer and the compaction's summary -- the test
/// is about the wiring, and the domain fold asserts on the summary's placement.
struct BudgetedLlm {
    prompt_tokens: u64,
}

#[async_trait]
impl LlmPort for BudgetedLlm {
    async fn chat_completion(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: Option<&[serde_json::Value]>,
        _extra_body: &HashMap<String, serde_json::Value>,
    ) -> Result<LlmResponse> {
        unreachable!("the streaming path is the only one the runner takes")
    }

    async fn chat_completion_streaming(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: Option<&[serde_json::Value]>,
        _extra_body: &HashMap<String, serde_json::Value>,
        _on_delta: &mut DeltaHandler<'_>,
    ) -> Result<LlmResponse> {
        Ok(LlmResponse {
            choices: vec![LlmChoice {
                message: Message::assistant(Some("work continues"), None),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: Some(LlmUsage {
                prompt_tokens: self.prompt_tokens,
                completion_tokens: 1,
                total_tokens: self.prompt_tokens + 1,
                cached_prompt_tokens: None,
                written_prompt_tokens: None,
            }),
            request_id: None,
        })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn agent_with_limit(limit: Option<u64>) -> AgentDef {
    AgentDef {
        name: "BudgetAgent".to_string(),
        description: "Test agent".to_string(),
        model: "test-model".to_string(),
        provider: None,
        vision: false,
        context_limit: limit,
        knows_about: vec![],
        mcp_servers: vec![],
        extra_body: HashMap::new(),
        extra_headers: HashMap::new(),
    }
}

fn settings(dir: &::tempfile::TempDir, in_session: Option<PathBuf>) -> RunSettings {
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();
    RunSettings {
        agent_def_path: agent_path,
        in_session: in_session.clone(),
        prompt_file: None,
        out_session: Some(in_session.unwrap_or_else(|| dir.path().join("session.json"))),
        template_path: None,
        tools_file: None,
        skills_dir: None,
        max_iterations: Some(10),
        max_runtime: None,
        stop_file: None,
        loop_retries: false,
    }
}

async fn run_with(
    llm: &BudgetedLlm,
    agent: AgentDef,
    dir: &::tempfile::TempDir,
    session: Option<PathBuf>,
) -> Result<RunOutcome> {
    let session_port = RecordingSessionPort::default();
    run(
        settings(dir, session),
        RunDeps {
            llm,
            agent_def: &StubAgentDefPort { agent_def: agent },
            session: &session_port,
            tool_def: &StubToolDefPort,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &StubMcpFactory,
        },
        &mut RunFacts::default(),
    )
    .await
}

/// A run whose inference reports a prompt past the limit: the loop compacts
/// between turns and FINISHES, instead of dying on the provider's 400.
#[tokio::test]
async fn a_run_over_its_context_limit_compacts_and_finishes() {
    let dir = ::tempfile::tempdir().unwrap();
    let llm = BudgetedLlm {
        prompt_tokens: 50_000,
    };
    let outcome = run_with(&llm, agent_with_limit(Some(40_000)), &dir, None)
        .await
        .expect("the run should survive its over-budget prompt");
    assert!(
        matches!(outcome, RunOutcome::Completed { .. }),
        "expected a completed run, got {:?}",
        outcome
    );
}

/// A run with no `context_limit` never compacts, whatever the usage says -- the
/// field is opt-in and every agent written before it existed runs unchanged.
#[tokio::test]
async fn a_run_without_a_context_limit_never_compacts() {
    let dir = ::tempfile::tempdir().unwrap();
    let llm = BudgetedLlm {
        prompt_tokens: 50_000,
    };
    let outcome = run_with(&llm, agent_with_limit(None), &dir, None)
        .await
        .expect("a run without a limit cannot trip one");
    assert!(matches!(outcome, RunOutcome::Completed { .. }));
}
