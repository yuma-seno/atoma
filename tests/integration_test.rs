mod common;

use anyhow::Result;
use async_trait::async_trait;
use atoma::infra::persistence::session as file_session;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

use atoma::application::runner::{run, CompletionReason, RunDeps, RunOutcome, RunSettings};
use atoma::domain::agent::{AgentDef, ParsedAgentDef};
use atoma::domain::ports::{
    AgentDefPort, FinishReason, LlmChoice, LlmResponse, McpFactory, SessionPort, ToolDefPort,
    ToolPort,
};
use atoma::domain::session::{Message, Session};
use atoma::domain::tool::ToolDef;

use common::mock_llm::MockLlmClient;
use common::mock_mcp::MockMcpRegistry;

// ── Minimal stub adapters ─────────────────────────────────────────────────────

/// Returns a fixed `ParsedAgentDef` regardless of path.
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

/// Always returns an empty / default session.
struct StubSessionPort;

impl SessionPort for StubSessionPort {
    fn load(&self, _path: &Path) -> Result<Session> {
        Ok(Session::default())
    }
    fn save(&self, _session: &Session, _path: &Path) -> Result<()> {
        Ok(())
    }
}

/// A session port that keeps what it was handed.
///
/// For the question `StubSessionPort` cannot answer: not "did saving work" but "was
/// anything saved at all, and was it resumable".
#[derive(Default)]
struct RecordingSessionPort {
    saved: Arc<Mutex<Option<Session>>>,
}

impl SessionPort for RecordingSessionPort {
    fn load(&self, _path: &Path) -> Result<Session> {
        Ok(Session::default())
    }
    fn save(&self, session: &Session, _path: &Path) -> Result<()> {
        *self.saved.lock().unwrap() = Some(session.clone());
        Ok(())
    }
}

/// Returns an empty tool map.
struct StubToolDefPort;

impl ToolDefPort for StubToolDefPort {
    fn load(&self, _path: &Path) -> Result<HashMap<String, ToolDef>> {
        Ok(HashMap::new())
    }
}

/// Returns a tool map with a single entry for the given key.
struct SingleEntryToolDefPort {
    key: String,
    tool_def: ToolDef,
}

impl SingleEntryToolDefPort {
    fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            tool_def: ToolDef {
                name: key.to_string(),
                unprefixed: false,
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
                url: None,
                headers: HashMap::new(),
                max_output_chars: None,
                hooks: atoma::domain::tool::Hooks::default(),
                request_timeout_secs: None,
                guess_severity_from_output: false,
            },
        }
    }
}

impl ToolDefPort for SingleEntryToolDefPort {
    fn load(&self, _path: &Path) -> Result<HashMap<String, ToolDef>> {
        let mut map = HashMap::new();
        map.insert(self.key.clone(), self.tool_def.clone());
        Ok(map)
    }
}

/// An MCP factory that always returns the provided mock registry.
struct StubMcpFactory {
    registry: std::sync::Mutex<Option<MockMcpRegistry>>,
}

impl StubMcpFactory {
    fn new(registry: MockMcpRegistry) -> Self {
        Self {
            registry: std::sync::Mutex::new(Some(registry)),
        }
    }
}

#[async_trait]
impl McpFactory for StubMcpFactory {
    async fn build(&self, _tool_defs: &[ToolDef]) -> Result<Box<dyn ToolPort + Send>> {
        let registry = self
            .registry
            .lock()
            .unwrap()
            .take()
            .expect("StubMcpFactory: registry already consumed");
        Ok(Box::new(registry))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn minimal_agent(name: &str) -> AgentDef {
    AgentDef {
        name: name.to_string(),
        description: "Test agent".to_string(),
        model: "gpt-4o-mini".to_string(),
        provider: None,
        vision: false,
        knows_about: vec![],
        mcp_servers: vec![],
        extra_body: HashMap::new(),
        extra_headers: HashMap::new(),
    }
}

// ── Integration tests ─────────────────────────────────────────────────────────

/// Single text response with finish_reason "stop".
#[tokio::test]
async fn test_single_text_response() {
    let llm = MockLlmClient::new().enqueue_text("Hello from the agent!");

    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("TestAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");

    // Write a dummy file so the path exists (the stub ignores it, but runner
    // reads the parent directory for knows_about expansion).
    std::fs::write(&agent_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}

/// Tool call followed by final text response.
#[tokio::test]
async fn test_tool_call_then_text_response() {
    use common::mock_llm::make_tool_call;

    let tool_call = make_tool_call("c1", "test_tool", r#"{"input":"hello"}"#);
    let llm = MockLlmClient::new()
        .enqueue_tool_calls(vec![tool_call])
        .enqueue_text("Done!");

    let registry = MockMcpRegistry::new()
        .with_tool("test_tool", "A test tool")
        .with_response("test_tool", "tool result");

    let agent_def = AgentDef {
        mcp_servers: vec!["test_server".to_string()],
        ..minimal_agent("ToolAgent")
    };

    let agent_port = StubAgentDefPort { agent_def };
    let session_port = StubSessionPort;
    let tool_def_port = SingleEntryToolDefPort::new("test_server");
    let mcp_factory = StubMcpFactory::new(registry);

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();
    let tools_path = dir.path().join("tools.yaml");
    std::fs::write(&tools_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: Some(tools_path),
            skills_dir: None,
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}

/// Built-in skill loading is available without MCP configuration and persists
/// through the ordinary assistant/tool message history.
#[tokio::test]
async fn test_skill_load_is_persisted_as_tool_history() {
    use atoma::application::tools::LOAD_SKILL_TOOL;
    use common::mock_llm::make_tool_call;

    let tool_call = make_tool_call(
        "skill-1",
        LOAD_SKILL_TOOL,
        r#"{"skill_name":"engineering/tdd"}"#,
    );
    let llm = MockLlmClient::new()
        .enqueue_tool_calls(vec![tool_call])
        .enqueue_text("Applied the skill.");

    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("SkillAgent"),
    };
    let session_port = atoma::infra::persistence::session::FileSessionAdapter;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    let skills_dir = dir.path().join("skills");
    let out_session_path = dir.path().join("session.json");
    std::fs::write(&agent_path, "").unwrap();
    std::fs::create_dir(&skills_dir).unwrap();
    std::fs::write(
        skills_dir.join("tdd.md"),
        "---\nname: engineering/tdd\ndescription: Test first.\n---\n\nUse red-green-refactor.\n",
    )
    .unwrap();

    run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: Some(out_session_path.clone()),
            template_path: None,
            tools_file: None,
            skills_dir: Some(skills_dir),
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await
    .unwrap();

    let saved = file_session::load(&out_session_path).unwrap();
    let tool_call_index = saved
        .messages
        .iter()
        .position(|message| {
            message.tool_calls.as_ref().is_some_and(|calls| {
                calls
                    .iter()
                    .any(|call| call.function.name == LOAD_SKILL_TOOL)
            })
        })
        .unwrap();
    let skill_result = &saved.messages[tool_call_index + 1];
    assert_eq!(skill_result.role, "tool");
    assert_eq!(skill_result.tool_call_id.as_deref(), Some("skill-1"));
    assert!(skill_result
        .content
        .as_ref()
        .and_then(|content| content.as_str())
        .unwrap()
        .contains("Use red-green-refactor."));
}

/// Max iterations exceeded returns an error.
#[tokio::test]
async fn test_max_iterations_exceeded() {
    use common::mock_llm::make_tool_call;

    // LLM always requests tool calls  Enever stops.
    let tool_call = || make_tool_call("c1", "test_tool", "{}");
    let llm = MockLlmClient::new()
        .enqueue_tool_calls(vec![tool_call()])
        .enqueue_tool_calls(vec![tool_call()])
        .enqueue_tool_calls(vec![tool_call()]);

    let registry = MockMcpRegistry::new()
        .with_tool("test_tool", "looping tool")
        .with_response("test_tool", "ok");

    let agent_def = AgentDef {
        mcp_servers: vec!["srv".to_string()],
        ..minimal_agent("LoopAgent")
    };

    let agent_port = StubAgentDefPort { agent_def };
    let session_port = StubSessionPort;
    let tool_def_port = SingleEntryToolDefPort::new("srv");
    let mcp_factory = StubMcpFactory::new(registry);

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();
    let tools_path = dir.path().join("tools.yaml");
    std::fs::write(&tools_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: Some(tools_path),
            skills_dir: None,
            max_iterations: Some(2),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("maximum iterations"),
        "Expected max iterations error, got: {}",
        msg
    );
}

/// A time limit stops the run, and says so in its own words.
///
/// Zero seconds rather than a sleep: the check is `elapsed() >= limit`, and any
/// elapsed time is at least zero, so the first iteration trips it. A test that waited
/// for real time to pass would be testing the clock.
#[tokio::test]
async fn test_max_runtime_exceeded() {
    use common::mock_llm::make_tool_call;

    let tool_call = || make_tool_call("c1", "test_tool", "{}");
    let llm = MockLlmClient::new()
        .enqueue_tool_calls(vec![tool_call()])
        .enqueue_tool_calls(vec![tool_call()]);

    let registry = MockMcpRegistry::new()
        .with_tool("test_tool", "looping tool")
        .with_response("test_tool", "ok");

    let agent_def = AgentDef {
        mcp_servers: vec!["srv".to_string()],
        ..minimal_agent("SlowAgent")
    };

    let agent_port = StubAgentDefPort { agent_def };
    let session_port = StubSessionPort;
    let tool_def_port = SingleEntryToolDefPort::new("srv");
    let mcp_factory = StubMcpFactory::new(registry);

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();
    let tools_path = dir.path().join("tools.yaml");
    std::fs::write(&tools_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: Some(tools_path),
            skills_dir: None,
            max_iterations: None,
            max_runtime: Some(std::time::Duration::from_secs(0)),
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    let err = result.expect_err("a run past its time limit is an error");
    assert!(
        atoma::application::runner::is_soft_stop(&err),
        "a time limit is a ceiling the caller asked for, not a failure: {}",
        err
    );
    let msg = format!("{}", err);
    assert!(
        msg.contains("time limit"),
        "Expected a time limit error, got: {}",
        msg
    );
}

/// A run that fails still keeps its work, and keeps it resumable.
///
/// Two things at once, because they are one change. The abort fires in the middle of a
/// batch of three parallel calls, which used to walk out of the loop and leave the
/// third with no result -- a conversation every provider refuses. And the session was
/// thrown away, so nobody found out.
#[tokio::test]
async fn test_a_failed_run_saves_a_resumable_session() {
    use common::mock_llm::make_tool_call;

    // Four identical failing calls in ONE turn. The third trips the counter, so the
    // fourth is reached only if the batch is finished rather than abandoned -- which
    // is the whole property under test.
    //
    // One turn, and every call in it failing, because of a second gap in the same
    // detector (atoma#17): any successful call resets the counter, so a batch with a
    // working call beside the failing one can never reach three at all.
    let failing = |id: &str| make_tool_call(id, "failer", r#"{"input":"same"}"#);
    let llm = MockLlmClient::new().enqueue_tool_calls(vec![
        failing("c1"),
        failing("c2"),
        failing("c3"),
        failing("c4"),
    ]);

    let registry = MockMcpRegistry::new().with_tool("failer", "a tool with no response");

    let agent_port = StubAgentDefPort {
        agent_def: AgentDef {
            mcp_servers: vec!["srv".to_string()],
            ..minimal_agent("DoomedAgent")
        },
    };
    let session_port = RecordingSessionPort::default();
    let saved = session_port.saved.clone();
    let tool_def_port = SingleEntryToolDefPort::new("srv");
    let mcp_factory = StubMcpFactory::new(registry);

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();
    let tools_path = dir.path().join("tools.yaml");
    std::fs::write(&tools_path, "").unwrap();

    let error = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: Some(dir.path().join("session.json")),
            template_path: None,
            tools_file: Some(tools_path),
            skills_dir: None,
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await
    .unwrap_err();

    assert!(
        error.to_string().contains("3 identical failed calls"),
        "{error}"
    );
    assert!(
        !atoma::application::runner::is_soft_stop(&error),
        "an abort is a failure, and saying otherwise would report it as a pause"
    );

    let session = saved
        .lock()
        .unwrap()
        .clone()
        .expect("a failed run must still save what it reached");

    // Every call answered. This is the property a provider enforces, and the reason
    // the session is worth saving at all.
    let calls: Vec<String> = session
        .messages
        .iter()
        .filter_map(|m| m.tool_calls.as_ref())
        .flatten()
        .map(|c| c.id.clone())
        .collect();
    let answers: Vec<String> = session
        .messages
        .iter()
        .filter_map(|m| m.tool_call_id.clone())
        .collect();
    for id in &calls {
        assert!(
            answers.contains(id),
            "tool call '{id}' has no result; this session cannot be resumed"
        );
    }
    assert_eq!(calls.len(), 4, "all four calls belong in the session");
}

/// A stop file that exists ends the run, and says so in its own words.
///
/// The file is created before the run rather than during it: what is under test is
/// that the loop consults it at all, and at the top, where the conversation is whole.
/// Racing a real writer against a real loop would test the scheduler.
#[tokio::test]
async fn test_stop_file_ends_the_run() {
    use common::mock_llm::make_tool_call;

    let tool_call = || make_tool_call("c1", "test_tool", "{}");
    let llm = MockLlmClient::new()
        .enqueue_tool_calls(vec![tool_call()])
        .enqueue_tool_calls(vec![tool_call()]);

    let registry = MockMcpRegistry::new()
        .with_tool("test_tool", "looping tool")
        .with_response("test_tool", "ok");

    let agent_def = AgentDef {
        mcp_servers: vec!["srv".to_string()],
        ..minimal_agent("StoppableAgent")
    };

    let agent_port = StubAgentDefPort { agent_def };
    let session_port = StubSessionPort;
    let tool_def_port = SingleEntryToolDefPort::new("srv");
    let mcp_factory = StubMcpFactory::new(registry);

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();
    let tools_path = dir.path().join("tools.yaml");
    std::fs::write(&tools_path, "").unwrap();
    let stop_path = dir.path().join("stop");
    std::fs::write(&stop_path, "").unwrap();

    let mut facts = atoma::application::runner::RunFacts::default();
    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: Some(tools_path),
            skills_dir: None,
            max_iterations: None,
            max_runtime: None,
            stop_file: Some(stop_path),
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut facts,
    )
    .await;

    let err = result.expect_err("a run asked to stop is an error");
    assert!(
        atoma::application::runner::is_soft_stop(&err),
        "being asked to stop is a hand-back, not a failure: {}",
        err
    );
    assert!(
        format!("{}", err).contains("Stop requested"),
        "Expected a stop-requested error, got: {}",
        err
    );
    // The facts survive the `Err`, which is the whole reason they are an out parameter:
    // this run produced no outcome to attach them to, and its caller still has to say
    // on stdout which of the three ceilings ended it. Exit status 2 cannot, and clap
    // uses 2 as well.
    assert_eq!(facts.ended_because, "stopped");
    let reported = atoma::application::runner::envelope(&facts, None);
    assert_eq!(reported["ended_because"], "stopped");
    assert_eq!(reported["response"], serde_json::Value::Null);
}

/// A stop file that is absent changes nothing.
///
/// The guard against the obvious inversion: a run that names a stop file it has not
/// been asked to use must behave exactly like a run that named none, and a bug that
/// read absence as presence would stop every run on its first turn.
#[tokio::test]
async fn test_an_absent_stop_file_does_not_stop_the_run() {
    let llm = MockLlmClient::new().enqueue_text("nobody stopped me");

    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("UnstoppedAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: None,
            max_runtime: None,
            stop_file: Some(dir.path().join("never-written")),
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(matches!(
        result.expect("a run nobody stopped should complete"),
        RunOutcome::Completed { .. }
    ));
}

/// Identical failed calls abort before consuming the full iteration budget.
#[tokio::test]
async fn test_identical_failed_tool_calls_abort() {
    use common::mock_llm::make_tool_call;

    let llm = MockLlmClient::new()
        .enqueue_tool_calls(vec![make_tool_call(
            "c1",
            "test_tool",
            r#"{"input":"same"}"#,
        )])
        .enqueue_tool_calls(vec![make_tool_call(
            "c2",
            "test_tool",
            r#"{"input":"same"}"#,
        )])
        .enqueue_tool_calls(vec![make_tool_call(
            "c3",
            "test_tool",
            r#"{"input":"same"}"#,
        )]);

    let registry = MockMcpRegistry::new().with_tool("test_tool", "failing tool");
    let agent_port = StubAgentDefPort {
        agent_def: AgentDef {
            mcp_servers: vec!["srv".to_string()],
            ..minimal_agent("LoopAgent")
        },
    };
    let session_port = StubSessionPort;
    let tool_def_port = SingleEntryToolDefPort::new("srv");
    let mcp_factory = StubMcpFactory::new(registry);

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();
    let tools_path = dir.path().join("tools.yaml");
    std::fs::write(&tools_path, "").unwrap();

    let error = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: Some(tools_path),
            skills_dir: None,
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("3 identical failed calls"), "{message}");
    assert!(!message.contains("maximum iterations"), "{message}");
}

/// A contentless completion is re-requested rather than failing the run.
#[tokio::test]
async fn test_empty_completion_is_retried_then_succeeds() {
    // First completion carries neither text nor tool calls; the next one is real.
    let llm = MockLlmClient::new()
        .enqueue_text("")
        .enqueue_text("Recovered on the second attempt.");

    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("FlakyProviderAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
}

/// Consecutive contentless completions abort instead of draining the budget.
#[tokio::test]
async fn test_repeated_empty_completions_abort() {
    let llm = MockLlmClient::new()
        .enqueue_text("")
        .enqueue_text("")
        .enqueue_text("")
        .enqueue_text("");

    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("DeadProviderAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let error = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(50),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("empty response"), "{message}");
    assert!(message.contains("times in a row"), "{message}");
    // Aborted on the bound, not by exhausting the 50-iteration budget.
    assert!(!message.contains("maximum iterations"), "{message}");
}

/// A model that loops inside one completion is cut off mid-stream.
///
/// The whole point of the circuit breaker: the loop is visible in the token stream
/// long before `max_tokens`, and the run ends on the loop rather than on the ceiling.
/// This drives it through the real inference loop, so what is tested is the wiring --
/// the detector being armed per completion, its error reaching the runner, and the
/// run ending on it -- rather than the detector alone, which has its own tests.
#[tokio::test]
async fn test_a_reasoning_loop_is_cut_off_mid_stream() {
    use atoma::domain::ports::{DeltaHandler, LlmPort};

    /// Streams a loop: one word, over and over, as a sentence each time.
    struct LoopingLlm;

    #[async_trait]
    impl LlmPort for LoopingLlm {
        async fn chat_completion(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
        ) -> Result<LlmResponse> {
            unreachable!("the loop must be driven through the streaming path")
        }

        async fn chat_completion_streaming(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
            on_delta: &mut DeltaHandler<'_>,
        ) -> Result<LlmResponse> {
            // Far more than the detector's window, so the threshold is reached well
            // before this would end on its own.
            for _ in 0..200 {
                on_delta("Let me write. ")?;
            }
            unreachable!("the detector must have refused a delta long before here")
        }
    }

    let llm = LoopingLlm;
    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("LoopingAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let error = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(50),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("distinct words"), "{message}");
    assert!(message.contains("reasoning loop"), "{message}");
    // Cut off by the loop, not by the iteration budget.
    assert!(!message.contains("maximum iterations"), "{message}");
}

/// A loop is retried when the caller asked for it, and the run finishes.
///
/// The countermeasure's other half: a loop is a bad sample rather than a verdict, so
/// the aborted completion is discarded, a note is appended, and the turn is asked for
/// again. This drives it through the real inference loop, so what is tested is the
/// wiring -- the retry count reaching the loop, the note entering the session, and the
/// second attempt's answer being the one that is kept.
#[tokio::test]
async fn test_a_loop_is_retried_when_asked_for() {
    use atoma::domain::ports::{DeltaHandler, LlmPort};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Loops on the first attempt and answers on the second.
    struct LoopsOnce {
        attempts: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl LlmPort for LoopsOnce {
        async fn chat_completion(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
        ) -> Result<LlmResponse> {
            unreachable!("driven through the streaming path")
        }

        async fn chat_completion_streaming(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
            on_delta: &mut DeltaHandler<'_>,
        ) -> Result<LlmResponse> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                for _ in 0..200 {
                    on_delta("Let me write. ")?;
                }
                unreachable!("the detector must have refused a delta")
            }
            on_delta("Here is the answer.")?;
            Ok(LlmResponse {
                choices: vec![LlmChoice {
                    message: Message::assistant(Some("Here is the answer."), None),
                    finish_reason: Some(FinishReason::Stop),
                }],
                usage: None,
                request_id: None,
            })
        }
    }

    let attempts = Arc::new(AtomicUsize::new(0));
    let llm = LoopsOnce {
        attempts: Arc::clone(&attempts),
    };
    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("LoopsOnceAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(50),
            max_runtime: None,
            stop_file: None,
            loop_retries: true,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(
        result.is_ok(),
        "the retry must let the run finish: {result:?}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "one loop, one retry, one answer"
    );
}

/// The retry is unbounded, so the run's own ceilings are what stop it.
///
/// `--loop-retries` has no count on purpose: the run already has a clock and a stop
/// file, and a model that loops on every attempt must be stopped by one of those
/// rather than by a third ceiling guessing at the same thing. This is the wiring that
/// makes that true -- the retry loop re-checking the stop file, which it would
/// otherwise never reach, because a model that loops every time never gets back to
/// the top of an iteration where the ceilings are read.
#[tokio::test]
async fn test_a_loop_that_never_stops_is_stopped_by_the_stop_file() {
    use atoma::domain::ports::{DeltaHandler, LlmPort};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Loops every time, and writes the stop file on its first attempt -- standing in
    /// for the person who sees the loop and asks for it to end.
    struct AlwaysLoops {
        attempts: Arc<AtomicUsize>,
        stop_file: std::path::PathBuf,
    }

    #[async_trait]
    impl LlmPort for AlwaysLoops {
        async fn chat_completion(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
        ) -> Result<LlmResponse> {
            unreachable!("driven through the streaming path")
        }

        async fn chat_completion_streaming(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
            on_delta: &mut DeltaHandler<'_>,
        ) -> Result<LlmResponse> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            std::fs::write(&self.stop_file, "").unwrap();
            for _ in 0..200 {
                on_delta("Let me write. ")?;
            }
            unreachable!("the detector must have refused a delta")
        }
    }

    let dir = tempdir().unwrap();
    let stop_file = dir.path().join("stop");
    let attempts = Arc::new(AtomicUsize::new(0));
    let llm = AlwaysLoops {
        attempts: Arc::clone(&attempts),
        stop_file: stop_file.clone(),
    };
    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("AlwaysLoopsAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let error = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(50),
            max_runtime: None,
            stop_file: Some(stop_file),
            loop_retries: true,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await
    .unwrap_err();

    assert!(
        error.to_string().contains("Stop requested"),
        "the stop file, not the loop, is what ended it: {error}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "the first attempt looped, and the re-ask was refused before it was made"
    );
}

/// Ordinary long-form reasoning is not cut short.
///
/// The acceptance criterion that matters most, driven through the same path as the
/// test above: a completion of the kind an agent actually writes must reach its end.
#[tokio::test]
async fn test_ordinary_reasoning_is_not_cut_short() {
    use atoma::domain::ports::{DeltaHandler, LlmPort};

    const REASONING: &str = "I need to look at the failing test first. The test \
        asserts that the parser rejects a malformed header, and it is failing \
        because the parser now accepts one. Let me read the parser and see what \
        changed. The change was in the header check, which used to compare the \
        name case-sensitively and now lowercases it. That is the bug: the check \
        must stay case-sensitive for the value and case-insensitive for the name. \
        I will fix the check and run the test again to confirm.";

    struct ReasoningLlm;

    #[async_trait]
    impl LlmPort for ReasoningLlm {
        async fn chat_completion(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
        ) -> Result<LlmResponse> {
            unreachable!("driven through the streaming path")
        }

        async fn chat_completion_streaming(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
            on_delta: &mut DeltaHandler<'_>,
        ) -> Result<LlmResponse> {
            // A word at a time, which is how a real stream arrives.
            for word in REASONING.split_inclusive(' ') {
                on_delta(word)?;
            }
            Ok(LlmResponse {
                choices: vec![LlmChoice {
                    message: Message::assistant(Some(REASONING), None),
                    finish_reason: Some(FinishReason::Stop),
                }],
                usage: None,
                request_id: None,
            })
        }
    }

    let llm = ReasoningLlm;
    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("ReasoningAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(50),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(
        result.is_ok(),
        "reasoning must not be cut short: {result:?}"
    );
}

/// content_filter finish_reason returns an error.
#[tokio::test]
async fn test_content_filter_returns_error() {
    use atoma::domain::ports::{FinishReason, LlmChoice, LlmResponse};
    use atoma::domain::session::Message;

    struct ContentFilterLlm;
    #[async_trait]
    impl atoma::domain::ports::LlmPort for ContentFilterLlm {
        async fn chat_completion(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
        ) -> Result<LlmResponse> {
            Ok(LlmResponse {
                choices: vec![LlmChoice {
                    message: Message::assistant(Some("filtered"), None),
                    finish_reason: Some(FinishReason::ContentFilter),
                }],
                usage: None,
                request_id: None,
            })
        }
    }

    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("FilterAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &ContentFilterLlm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("content filter"),
        "Expected content filter error, got: {}",
        msg
    );
}

#[tokio::test]
async fn test_truncated_response_reports_length_reason() {
    struct TruncatedLlm;

    #[async_trait]
    impl atoma::domain::ports::LlmPort for TruncatedLlm {
        async fn chat_completion(
            &self,
            _model: &str,
            _messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
        ) -> Result<LlmResponse> {
            Ok(LlmResponse {
                choices: vec![LlmChoice {
                    message: Message::assistant(Some("partial"), None),
                    finish_reason: Some(FinishReason::Length),
                }],
                usage: None,
                request_id: None,
            })
        }
    }

    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("TruncatedAgent"),
    };
    let session_port = StubSessionPort;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());
    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    std::fs::write(&agent_path, "").unwrap();

    let outcome = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: None,
            prompt_file: None,
            out_session: None,
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &TruncatedLlm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        RunOutcome::Completed {
            reason: CompletionReason::Length,
            ..
        }
    ));
}

#[tokio::test]
async fn test_prompt_file_is_appended_and_persisted() {
    struct RecordingLlm {
        seen_messages: Arc<Mutex<Vec<Message>>>,
    }

    #[async_trait]
    impl atoma::domain::ports::LlmPort for RecordingLlm {
        async fn chat_completion(
            &self,
            _model: &str,
            messages: &[Message],
            _tools: Option<&[serde_json::Value]>,
            _extra_body: &HashMap<String, serde_json::Value>,
        ) -> Result<LlmResponse> {
            *self.seen_messages.lock().unwrap() = messages.to_vec();

            Ok(LlmResponse {
                choices: vec![LlmChoice {
                    message: Message::assistant(Some("Done!"), None),
                    finish_reason: Some(FinishReason::Stop),
                }],
                usage: None,
                request_id: None,
            })
        }
    }

    let seen_messages = Arc::new(Mutex::new(Vec::new()));
    let llm = RecordingLlm {
        seen_messages: seen_messages.clone(),
    };

    let agent_port = StubAgentDefPort {
        agent_def: minimal_agent("ContextAgent"),
    };
    let session_port = atoma::infra::persistence::session::FileSessionAdapter;
    let tool_def_port = StubToolDefPort;
    let mcp_factory = StubMcpFactory::new(MockMcpRegistry::new());

    let dir = tempdir().unwrap();
    let agent_path = dir.path().join("agent.md");
    let in_session_path = dir.path().join("input-session.json");
    let prompt_path = dir.path().join("prompt.txt");
    let out_session_path = dir.path().join("out-session.json");
    std::fs::write(&agent_path, "").unwrap();
    std::fs::write(&prompt_path, "new prompt").unwrap();

    let mut persisted_session = Session::default();
    persisted_session
        .messages
        .push(Message::user("persistent history"));
    file_session::save(&persisted_session, &in_session_path).unwrap();

    let result = run(
        RunSettings {
            agent_def_path: agent_path,
            in_session: Some(in_session_path),
            prompt_file: Some(prompt_path),
            out_session: Some(out_session_path.clone()),
            template_path: None,
            tools_file: None,
            skills_dir: None,
            max_iterations: Some(10),
            max_runtime: None,
            stop_file: None,
            loop_retries: false,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            template: &atoma::infra::template::FileTemplateAdapter,
            mcp_factory: &mcp_factory,
        },
        &mut atoma::application::runner::RunFacts::default(),
    )
    .await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);

    let seen = seen_messages.lock().unwrap().clone();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].role, "system");
    assert_eq!(
        seen[1].content.as_ref().and_then(|value| value.as_str()),
        Some("persistent history")
    );
    assert_eq!(
        seen[2].content.as_ref().and_then(|value| value.as_str()),
        Some("new prompt")
    );

    let saved = file_session::load(&out_session_path).unwrap();
    let saved_texts: Vec<&str> = saved
        .messages
        .iter()
        .filter_map(|message| message.content.as_ref().and_then(|value| value.as_str()))
        .collect();

    assert!(saved_texts.contains(&"persistent history"));
    assert!(saved_texts.contains(&"new prompt"));
    assert!(saved_texts.contains(&"Done!"));
}
