use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, RuntimeChatTurnDriver, RuntimeLlmExecutor,
    TurnError,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::agents_md::AgentsMdFile;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

#[tokio::test]
async fn ac1_load_project_agent_md_from_workspace() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("project");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::write(
        workspace.join("AGENTS.md"),
        "# Project\nproject instructions",
    )
    .expect("write agent md");

    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(&workspace).await;

    let project_file = files.iter().find(|f| f.path == workspace.join("AGENTS.md"));
    assert!(project_file.is_some(), "should find workspace AGENTS.md");
    assert!(project_file
        .expect("project file")
        .content
        .contains("project instructions"));
}

#[tokio::test]
async fn ac1_load_order_root_before_workspace() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().join("parent");
    let child = parent.join("child");
    std::fs::create_dir_all(&child).expect("create child");
    std::fs::write(parent.join("AGENTS.md"), "parent instructions").expect("write parent");
    std::fs::write(child.join("AGENTS.md"), "child instructions").expect("write child");

    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(&child).await;

    let contents: Vec<&str> = files.iter().map(|f| f.content.as_str()).collect();
    let parent_pos = contents
        .iter()
        .position(|c| c.contains("parent instructions"))
        .expect("parent file present");
    let child_pos = contents
        .iter()
        .position(|c| c.contains("child instructions"))
        .expect("child file present");
    assert!(parent_pos < child_pos, "parent should come before child");
}

#[tokio::test]
async fn ac1_load_dot_aijia_and_local_agent_md() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("project");
    let dot_claude = workspace.join(".aijia");
    std::fs::create_dir_all(&dot_claude).expect("create .aijia");
    std::fs::write(dot_claude.join("AGENTS.md"), "dot-claude instructions")
        .expect("write dot claude");
    std::fs::write(workspace.join("AGENTS.local.md"), "local override").expect("write local claude");

    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(&workspace).await;

    assert!(files
        .iter()
        .any(|f| f.content.contains("dot-claude instructions")));
    assert!(files.iter().any(|f| f.content.contains("local override")));
}

#[tokio::test]
async fn ac2_mtime_cache_invalidate_on_change() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().to_path_buf();
    let file_path = workspace.join("AGENTS.md");
    std::fs::write(&file_path, "version 1").expect("write v1");

    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files1 = loader.load(&workspace).await;
    assert!(files1.iter().any(|f| f.content.contains("version 1")));

    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file_path, "version 2").expect("write v2");

    let files2 = loader.load(&workspace).await;
    assert!(files2.iter().any(|f| f.content.contains("version 2")));
}

#[tokio::test]
async fn review_agents_md_loader_empty_path_does_not_panic() {
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Path::new("")).await;
    let _ = files;
}

#[test]
fn review_agents_md_loader_has_no_tauri_dependency() {
    let source =
        std::fs::read_to_string("src/runtime/agents_md.rs").expect("read agents_md.rs");
    assert!(
        !source.contains("use tauri::"),
        "runtime/agents_md.rs must not depend on tauri::*"
    );
}

struct AgentsMdContextExecutor {
    workspace_path: PathBuf,
    agents_md_files: Vec<AgentsMdFile>,
    received_messages: Mutex<Vec<Vec<serde_json::Value>>>,
}

impl AgentsMdContextExecutor {
    fn new(workspace_path: PathBuf, agents_md_files: Vec<AgentsMdFile>) -> Self {
        Self {
            workspace_path,
            agents_md_files,
            received_messages: Mutex::new(Vec::new()),
        }
    }

    fn all_messages(&self) -> Vec<Vec<serde_json::Value>> {
        self.received_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for AgentsMdContextExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.received_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());
        Ok(LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(self.workspace_path.clone())
    }

    async fn load_agents_md(
        &self,
        workspace_path: &Path,
    ) -> Result<Vec<AgentsMdFile>, TurnError> {
        assert_eq!(workspace_path, self.workspace_path.as_path());
        Ok(self.agents_md_files.clone())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])  // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn ac3_driver_inserts_separate_agents_md_context_message_after_system_reminder() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("project");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let renlijia_path = workspace.join("AGENTS.md");
    let renlijia_content = "project instructions";
    let executor = Arc::new(AgentsMdContextExecutor::new(
        workspace.clone(),
        vec![AgentsMdFile {
            path: renlijia_path.clone(),
            content: renlijia_content.to_string(),
        }],
    ));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus, executor.clone());
    let mut turn = make_test_turn("conv-renlijia-md");
    let request = ChatTurnRequest::new("conv-renlijia-md", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let messages = executor.all_messages();
    assert!(
        !messages.is_empty(),
        "executor must receive initial messages"
    );
    let first_call_messages = &messages[0];
    assert!(
        first_call_messages.len() >= 3,
        "must have [system-reminder, renlijia-md-context, user]"
    );
    assert!(
        first_call_messages[0]["content"]
            .as_str()
            .unwrap_or("")
            .contains("<system-reminder>"),
        "messages[0] must remain the date reminder"
    );
    let context_message = first_call_messages[1]["content"].as_str().unwrap_or("");
    assert!(
        context_message.contains("As you answer the user's questions"),
        "messages[1] must be a separate meta context message, got: {}",
        context_message
    );
    assert!(context_message.contains("# agentsMd"));
    assert!(context_message.contains(renlijia_path.to_string_lossy().as_ref()));
    assert!(context_message.contains(renlijia_content));
    assert_eq!(first_call_messages[2]["content"], "hello");
}
