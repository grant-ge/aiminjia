//! AGENTS.md 加载器测试
//!
//! 覆盖 spec §4.5：
//! - 仅从 `{authorized_workspace}/AGENTS.md` 加载
//! - None → 返回空 Vec
//! - 文件不存在 → 返回空 Vec
//! - 64 KiB 截断
//! - 反向断言：.aijia / 父目录 / AGENTS.local.md 均不加载

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_lib::runtime::agents_md::AgentsMdFile;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, RuntimeChatTurnDriver, RuntimeLlmExecutor,
    TurnError,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::store::AuthorizedWorkspaceRef;
use async_trait::async_trait;

// ── 辅助函数 ──────────────────────────────────────────────────────────────────

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

fn make_ws_ref(root_path: PathBuf) -> AuthorizedWorkspaceRef {
    AuthorizedWorkspaceRef {
        id: "ws-test".to_string(),
        root_path,
        display_name: "test workspace".to_string(),
    }
}

// ── 正向测试 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn loads_when_authorized_workspace_has_agents_md() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws_path = tmp.path().to_path_buf();
    std::fs::write(ws_path.join("AGENTS.md"), "# Project\nproject instructions")
        .expect("write AGENTS.md");

    let ws_ref = make_ws_ref(ws_path.clone());
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Some(&ws_ref)).await;

    assert_eq!(files.len(), 1, "should load exactly one file");
    assert_eq!(files[0].path, ws_path.join("AGENTS.md"));
    assert!(files[0].content.contains("project instructions"));
}

#[tokio::test]
async fn returns_empty_when_authorized_workspace_is_none() {
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(None).await;
    assert!(files.is_empty(), "None workspace should return empty vec");
}

#[tokio::test]
async fn returns_empty_when_file_not_present() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws_ref = make_ws_ref(tmp.path().to_path_buf());
    // workspace 根目录下无 AGENTS.md 文件

    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Some(&ws_ref)).await;
    assert!(files.is_empty(), "no AGENTS.md → empty vec");
}

#[tokio::test]
async fn returns_loaded_with_empty_string_when_file_is_empty() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws_path = tmp.path().to_path_buf();
    std::fs::write(ws_path.join("AGENTS.md"), "").expect("write empty");

    let ws_ref = make_ws_ref(ws_path.clone());
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Some(&ws_ref)).await;

    assert_eq!(files.len(), 1, "empty file still loads as one entry");
    assert_eq!(files[0].content, "");
}

#[tokio::test]
async fn truncates_when_file_exceeds_64kib() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws_path = tmp.path().to_path_buf();
    // 70 KiB の ASCII コンテンツ
    let large_content = "x".repeat(70 * 1024);
    std::fs::write(ws_path.join("AGENTS.md"), &large_content).expect("write large");

    let ws_ref = make_ws_ref(ws_path.clone());
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Some(&ws_ref)).await;

    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].content.len(),
        65536,
        "content must be truncated to exactly 65536 bytes"
    );
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

    let ws_ref = make_ws_ref(workspace.clone());
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Some(&ws_ref)).await;

    let project_file = files.iter().find(|f| f.path == workspace.join("AGENTS.md"));
    assert!(project_file.is_some(), "should find workspace AGENTS.md");
    assert!(project_file
        .expect("project file")
        .content
        .contains("project instructions"));
}

#[tokio::test]
async fn ac2_mtime_cache_invalidate_on_change() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().to_path_buf();
    let file_path = workspace.join("AGENTS.md");
    std::fs::write(&file_path, "version 1").expect("write v1");

    let ws_ref = make_ws_ref(workspace.clone());
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files1 = loader.load(Some(&ws_ref)).await;
    assert!(files1.iter().any(|f| f.content.contains("version 1")));

    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file_path, "version 2").expect("write v2");

    let files2 = loader.load(Some(&ws_ref)).await;
    assert!(files2.iter().any(|f| f.content.contains("version 2")));
}

// ── 反向测试（废弃路径不应加载） ──────────────────────────────────────────────

#[tokio::test]
async fn does_not_load_from_aijia_subdirectory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws_path = tmp.path().to_path_buf();
    let dot_aijia = ws_path.join(".aijia");
    std::fs::create_dir_all(&dot_aijia).expect("create .aijia");
    std::fs::write(dot_aijia.join("AGENTS.md"), "aijia subdirectory content")
        .expect("write .aijia/AGENTS.md");
    // workspace 根目录没有 AGENTS.md

    let ws_ref = make_ws_ref(ws_path.clone());
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Some(&ws_ref)).await;

    assert!(
        files.is_empty(),
        ".aijia/AGENTS.md must NOT be loaded; got: {:?}",
        files
    );
}

#[tokio::test]
async fn does_not_load_from_parent_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let parent = tmp.path().to_path_buf();
    let child = parent.join("child");
    std::fs::create_dir_all(&child).expect("create child");
    std::fs::write(parent.join("AGENTS.md"), "parent instructions")
        .expect("write parent AGENTS.md");
    // child 目录内没有 AGENTS.md

    let ws_ref = make_ws_ref(child.clone());
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Some(&ws_ref)).await;

    assert!(
        files.is_empty(),
        "parent directory AGENTS.md must NOT be loaded; got: {:?}",
        files
    );
}

#[tokio::test]
async fn does_not_load_agents_local_md_variant() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws_path = tmp.path().to_path_buf();
    std::fs::write(ws_path.join("AGENTS.local.md"), "local override content")
        .expect("write AGENTS.local.md");
    // workspace 根目录下无 AGENTS.md 文件

    let ws_ref = make_ws_ref(ws_path.clone());
    let mut loader = app_lib::runtime::agents_md::AgentsMdLoader::new();
    let files = loader.load(Some(&ws_ref)).await;

    assert!(
        files.is_empty(),
        "AGENTS.local.md must NOT be loaded; got: {:?}",
        files
    );
}

// ── 架构约束回归 ──────────────────────────────────────────────────────────────

#[test]
fn review_agents_md_loader_has_no_tauri_dependency() {
    let source = std::fs::read_to_string("src/runtime/agents_md.rs").expect("read agents_md.rs");
    assert!(
        !source.contains("use tauri::"),
        "runtime/agents_md.rs must not depend on tauri::*"
    );
}

// ── ac3：driver 注入独立 agentsMd 上下文消息 ─────────────────────────────────

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
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(self.workspace_path.clone())
    }

    async fn load_agents_md(
        &self,
        _authorized_workspace: Option<&app_lib::runtime::store::AuthorizedWorkspaceRef>,
    ) -> Result<Vec<AgentsMdFile>, TurnError> {
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
        Ok(vec![])
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
        context_message.contains("Project instructions are shown below"),
        "messages[1] must be a separate meta context message, got: {}",
        context_message
    );
    assert!(
        context_message.contains("OVERRIDE any default behavior"),
        "messages[1] must declare override semantics, got: {}",
        context_message
    );
    assert!(context_message.contains("# agentsMd"));
    assert!(context_message.contains(renlijia_path.to_string_lossy().as_ref()));
    assert!(context_message.contains(renlijia_content));
    assert_eq!(first_call_messages[2]["content"], "hello");
}
