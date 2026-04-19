use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::path::Path;

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, RuntimeChatTurnDriver, RuntimeLlmExecutor,
    TurnError,
};
use app_lib::runtime::claude_md::ClaudeMdFile;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

#[tokio::test]
async fn ac1_load_project_claude_md_from_workspace() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("project");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::write(
        workspace.join("CLAUDE.md"),
        "# Project\nproject instructions",
    )
    .expect("write claude md");

    let mut loader = app_lib::runtime::claude_md::ClaudeMdLoader::new();
    let files = loader.load(&workspace).await;

    let project_file = files
        .iter()
        .find(|f| f.path == workspace.join("CLAUDE.md"));
    assert!(project_file.is_some(), "should find workspace CLAUDE.md");
    assert!(
        project_file
            .expect("project file")
            .content
            .contains("project instructions")
    );
}

#[tokio::test]
async fn ac1_load_order_root_before_workspace() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().join("parent");
    let child = parent.join("child");
    std::fs::create_dir_all(&child).expect("create child");
    std::fs::write(parent.join("CLAUDE.md"), "parent instructions").expect("write parent");
    std::fs::write(child.join("CLAUDE.md"), "child instructions").expect("write child");

    let mut loader = app_lib::runtime::claude_md::ClaudeMdLoader::new();
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
async fn ac1_load_dot_claude_and_local_claude_md() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("project");
    let dot_claude = workspace.join(".claude");
    std::fs::create_dir_all(&dot_claude).expect("create .claude");
    std::fs::write(dot_claude.join("CLAUDE.md"), "dot-claude instructions")
        .expect("write dot claude");
    std::fs::write(workspace.join("CLAUDE.local.md"), "local override")
        .expect("write local claude");

    let mut loader = app_lib::runtime::claude_md::ClaudeMdLoader::new();
    let files = loader.load(&workspace).await;

    assert!(
        files.iter()
            .any(|f| f.content.contains("dot-claude instructions"))
    );
    assert!(files.iter().any(|f| f.content.contains("local override")));
}

#[tokio::test]
async fn ac2_mtime_cache_invalidate_on_change() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().to_path_buf();
    let file_path = workspace.join("CLAUDE.md");
    std::fs::write(&file_path, "version 1").expect("write v1");

    let mut loader = app_lib::runtime::claude_md::ClaudeMdLoader::new();
    let files1 = loader.load(&workspace).await;
    assert!(files1.iter().any(|f| f.content.contains("version 1")));

    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file_path, "version 2").expect("write v2");

    let files2 = loader.load(&workspace).await;
    assert!(files2.iter().any(|f| f.content.contains("version 2")));
}

#[tokio::test]
async fn review_claude_md_loader_empty_path_does_not_panic() {
    let mut loader = app_lib::runtime::claude_md::ClaudeMdLoader::new();
    let files = loader.load(Path::new("")).await;
    let _ = files;
}

#[test]
fn review_claude_md_loader_has_no_tauri_dependency() {
    let source = std::fs::read_to_string("src/runtime/claude_md.rs").expect("read claude_md.rs");
    assert!(
        !source.contains("use tauri::"),
        "runtime/claude_md.rs must not depend on tauri::*"
    );
}

struct ClaudeMdContextExecutor {
    workspace_path: PathBuf,
    claude_md_files: Vec<ClaudeMdFile>,
    received_messages: Mutex<Vec<Vec<serde_json::Value>>>,
}

impl ClaudeMdContextExecutor {
    fn new(workspace_path: PathBuf, claude_md_files: Vec<ClaudeMdFile>) -> Self {
        Self {
            workspace_path,
            claude_md_files,
            received_messages: Mutex::new(Vec::new()),
        }
    }

    fn all_messages(&self) -> Vec<Vec<serde_json::Value>> {
        self.received_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for ClaudeMdContextExecutor {
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
        })
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(self.workspace_path.clone())
    }

    async fn load_claude_md(&self, workspace_path: &Path) -> Result<Vec<ClaudeMdFile>, TurnError> {
        assert_eq!(workspace_path, self.workspace_path.as_path());
        Ok(self.claude_md_files.clone())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }
}

#[tokio::test]
async fn ac3_driver_inserts_separate_claude_md_context_message_after_system_reminder() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("project");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let claude_path = workspace.join("CLAUDE.md");
    let claude_content = "project instructions";
    let executor = Arc::new(ClaudeMdContextExecutor::new(
        workspace.clone(),
        vec![ClaudeMdFile {
            path: claude_path.clone(),
            content: claude_content.to_string(),
        }],
    ));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus, executor.clone());
    let mut turn = make_test_turn("conv-claude-md");
    let request = ChatTurnRequest::new("conv-claude-md", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let messages = executor.all_messages();
    assert!(!messages.is_empty(), "executor must receive initial messages");
    let first_call_messages = &messages[0];
    assert!(
        first_call_messages.len() >= 3,
        "must have [system-reminder, claude-md-context, user]"
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
    assert!(context_message.contains("# claudeMd"));
    assert!(context_message.contains(claude_path.to_string_lossy().as_ref()));
    assert!(context_message.contains(claude_content));
    assert_eq!(first_call_messages[2]["content"], "hello");
}
