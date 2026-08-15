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
    AgentDefPort, LlmChoice, LlmResponse, McpFactory, SessionPort, ToolDefPort, ToolPort,
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
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
                hooks: atoma::domain::tool::Hooks::default(),
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
        callable_by: vec![],
        mcp_servers: vec![],
        extra_body: HashMap::new(),
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
            max_iterations: 10,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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
            max_iterations: 10,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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

    let tool_call = make_tool_call("skill-1", LOAD_SKILL_TOOL, r#"{"name":"engineering/tdd"}"#);
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
            max_iterations: 10,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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

    // LLM always requests tool calls — never stops.
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
            max_iterations: 2,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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
            max_iterations: 10,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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
            max_iterations: 10,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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
            max_iterations: 50,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
    )
    .await
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("empty response"), "{message}");
    assert!(message.contains("times in a row"), "{message}");
    // Aborted on the bound, not by exhausting the 50-iteration budget.
    assert!(!message.contains("maximum iterations"), "{message}");
}

/// content_filter finish_reason returns an error.
#[tokio::test]
async fn test_content_filter_returns_error() {
    use atoma::domain::ports::{LlmChoice, LlmResponse};
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
                    finish_reason: Some("content_filter".to_string()),
                }],
                usage: None,
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
            max_iterations: 10,
        },
        RunDeps {
            llm: &ContentFilterLlm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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
                    finish_reason: Some("length".to_string()),
                }],
                usage: None,
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
            max_iterations: 10,
        },
        RunDeps {
            llm: &TruncatedLlm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
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
            max_iterations: 10,
        },
        RunDeps {
            llm: &llm,
            agent_def: &agent_port,
            session: &session_port,
            tool_def: &tool_def_port,
            skill: &atoma::infra::persistence::skill::FileSkillAdapter,
            mcp_factory: &mcp_factory,
        },
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
