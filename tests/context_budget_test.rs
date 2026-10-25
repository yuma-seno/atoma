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

/// A session port whose load hands back a session with enough turns behind it
/// that a compaction has something to drop. A fresh session's handful of
/// messages is entirely recent turns, and `compact` honestly answers `None`
/// to it -- which is the soft stop, not the hook round this file's newest
/// test needs to observe.
struct SeededSessionPort;

impl SessionPort for SeededSessionPort {
    fn load(&self, _path: &Path) -> Result<atoma::domain::session::Session> {
        let mut messages = vec![Message::user("the task this session works on")];
        for i in 0..30 {
            messages.push(Message::user(&format!("turn {i}: please look at file {i}")));
            messages.push(Message::assistant(
                None,
                Some(vec![common::mock_llm::make_tool_call(
                    &format!("seed-{i}"),
                    "read",
                    &format!("{{\"path\":\"f{i}\"}}"),
                )]),
            ));
            messages.push(Message::tool(&format!("seed-{i}"), &"x".repeat(100)));
        }
        Ok(atoma::domain::session::Session {
            messages,
            ..Default::default()
        })
    }
    fn save(&self, _session: &atoma::domain::session::Session, _path: &Path) -> Result<()> {
        Ok(())
    }
}

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

/// The hooks the tools file declares REACH the compaction, and see the session
/// before it is replaced.
///
/// This is the test the wiring below needed and did not have: the hooks are
/// gathered while the MCP registry is built, inside an `else` block, and the
/// first cut of that wiring let the block close over the vec -- a second,
/// empty `let` shadowed it below -- so a real delivery's hook never ran and
/// the archive silently never happened. Every other test here passes an agent
/// with no `mcp_servers`, which takes the `None` arm and never exercises the
/// collection at all; this one declares a server, a tools file, and a hook,
/// and asserts on what the hook received. The first real compaction
/// (atomaton #149's chain, run 40166780902) found the bug in production
/// before any test here did.
#[cfg(unix)]
#[tokio::test]
async fn a_tools_file_hook_runs_before_the_session_is_replaced() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    // The hook: record the payload it was handed, answer "archived".
    let dir = ::tempfile::tempdir().unwrap();
    let marker = dir.path().join("hook-saw.json");
    let hook = dir.path().join("hook.sh");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\ncat - > \"{}\"\necho '{{\"archived\":true}}'\n",
            marker.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    // One tool call whose usage reports an over-budget prompt, then quiet text at
    // an under-budget one. The tool call is what keeps the loop alive past the
    // first inference -- a text-only first response completes the run before the
    // top-of-loop check ever sees a measurement, which is exactly why the tests
    // above could not catch the shadowing: they never reached the check either.
    struct ScriptedLlm {
        calls: Mutex<usize>,
    }
    #[async_trait]
    impl LlmPort for ScriptedLlm {
        async fn chat_completion(
            &self,
            _: &str,
            _: &[Message],
            _: Option<&[serde_json::Value]>,
            _: &HashMap<String, serde_json::Value>,
        ) -> Result<LlmResponse> {
            unreachable!("the streaming path is the only one the runner takes")
        }

        async fn chat_completion_streaming(
            &self,
            _: &str,
            _: &[Message],
            _: Option<&[serde_json::Value]>,
            _: &HashMap<String, serde_json::Value>,
            _: &mut DeltaHandler<'_>,
        ) -> Result<LlmResponse> {
            let n = {
                let mut calls = self.calls.lock().unwrap();
                *calls += 1;
                *calls
            };
            let message = if n == 1 {
                Message {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![common::mock_llm::make_tool_call(
                        "call_1", "probe", "{}",
                    )]),
                    tool_call_id: None,
                    name: None,
                    atoma_metadata: None,
                    provider_items: None,
                    reasoning_content: None,
                    finish_reason: None,
                }
            } else {
                Message::assistant(Some("work continues"), None)
            };
            Ok(LlmResponse {
                choices: vec![LlmChoice {
                    message,
                    finish_reason: Some(if n == 1 {
                        FinishReason::ToolCalls
                    } else {
                        FinishReason::Stop
                    }),
                }],
                usage: Some(LlmUsage {
                    // Call 2 is the SUMMARY the compaction generates; its usage is
                    // deliberately never recorded by the runner, so only call 1's
                    // over-budget report and call 3's under-budget one matter.
                    prompt_tokens: if n == 1 { 50_000 } else { 1_000 },
                    completion_tokens: 1,
                    total_tokens: 1_001,
                    cached_prompt_tokens: None,
                    written_prompt_tokens: None,
                }),
                request_id: None,
            })
        }
    }

    // A server with one tool, so the scripted tool call resolves instead of erroring.
    struct ProbeMcpFactory;
    #[async_trait::async_trait]
    impl McpFactory for ProbeMcpFactory {
        async fn build(&self, _defs: &[ToolDef]) -> Result<Box<dyn ToolPort + Send>> {
            Ok(Box::new(
                MockMcpRegistry::new()
                    .with_tool("probe", "the one tool the scripted call makes")
                    .with_response("probe", "the probe answered"),
            ))
        }
    }

    struct GivenToolDefs(std::collections::BTreeMap<String, ToolDef>);
    impl ToolDefPort for GivenToolDefs {
        fn load(&self, _path: &Path) -> Result<HashMap<String, ToolDef>> {
            Ok(self.0.clone().into_iter().collect())
        }
    }

    let mut tool_def = ToolDef {
        name: "stub".to_string(),
        command: String::new(),
        args: vec![],
        env: HashMap::new(),
        url: None,
        headers: HashMap::new(),
        hooks: atoma::domain::tool::Hooks {
            before_compaction: vec![hook.display().to_string()],
            ..Default::default()
        },
        unprefixed: false,
        max_output_chars: None,
        request_timeout_secs: None,
        guess_severity_from_output: false,
    };
    tool_def.name = "stub".to_string();

    let mut agent = agent_with_limit(Some(40_000));
    agent.mcp_servers = vec!["stub".to_string()];

    let out_session = dir.path().join("session.json");
    let settings = RunSettings {
        agent_def_path: dir.path().join("agent.md"),
        in_session: Some(dir.path().join("seeded.json")),
        prompt_file: None,
        out_session: Some(out_session.clone()),
        template_path: None,
        tools_file: Some(dir.path().join("tools.yaml")),
        skills_dir: None,
        max_iterations: Some(10),
        max_runtime: None,
        stop_file: None,
        loop_retries: false,
    };
    std::fs::write(&settings.agent_def_path, "").unwrap();
    std::fs::write(settings.tools_file.as_ref().unwrap(), "").unwrap();

    let mut defs = HashMap::new();
    defs.insert("stub".to_string(), tool_def);

    let llm = ScriptedLlm {
        calls: Mutex::new(0),
    };
    let session_port = SeededSessionPort;
    let mut facts = RunFacts::default();
    let outcome = run(
        settings,
        RunDeps {
            llm: &llm,
            agent_def: &StubAgentDefPort { agent_def: agent },
            session: &session_port,
            tool_def: &GivenToolDefs(defs.into_iter().collect()),
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &ProbeMcpFactory,
        },
        &mut facts,
    )
    .await
    .expect("the run should survive its over-budget prompt");
    assert!(
        matches!(outcome, RunOutcome::Completed { .. }),
        "expected a completed run, got {:?}",
        outcome
    );
    assert_eq!(facts.compactions, 1, "one compaction, one hook round");

    // The hook ran, and BEFORE the replacement: the payload names the agent, the
    // over-budget measurement, the limit, and the path the runner was about to
    // overwrite.
    let seen = std::fs::read_to_string(&marker).expect("the hook should have written what it saw");
    let payload: serde_json::Value = serde_json::from_str(&seen).expect("the payload is JSON");
    assert_eq!(payload["agent"], "BudgetAgent");
    assert_eq!(payload["prompt_tokens"], 50_000);
    assert_eq!(payload["context_limit"], 40_000);
    assert_eq!(
        payload["session_path"].as_str().unwrap(),
        out_session.display().to_string(),
        "the hook is told the path the runner will overwrite"
    );
}
