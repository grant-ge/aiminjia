use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, RuntimeChatTurnDriver, RuntimeLlmExecutor,
    TurnError,
};
use app_lib::runtime::renlijia_md::RenlijiaMdFile;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::project_memory::{
    ProjectMemoryContext, ProjectMemoryEntryDraft, ProjectMemoryService, ProjectMemoryType,
};
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

#[test]
fn u4_save_memory_writes_frontmatter_and_updates_memory_index() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let app_data_dir = tmp.path().join("app-data");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let service = ProjectMemoryService::new(&app_data_dir, &workspace);
    let saved = service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "回复要简洁".to_string(),
            description: "用户偏好简洁、直接的回复风格".to_string(),
            content: "回答尽量短，不要重复总结已经完成的改动。".to_string(),
            source: Some("u4-test".to_string()),
        })
        .expect("save memory");

    let entry_content = std::fs::read_to_string(&saved.path).expect("read entry");
    assert!(entry_content.contains("type: user_preference"));
    assert!(entry_content.contains("name: 回复要简洁"));
    assert!(entry_content.contains("description: 用户偏好简洁、直接的回复风格"));
    assert!(entry_content.contains("回答尽量短"));

    let index_content =
        std::fs::read_to_string(service.entrypoint_path()).expect("read memory index");
    assert!(index_content.contains("MEMORY.md"));
    assert!(index_content.contains("回复要简洁"));
    assert!(index_content.contains("用户偏好简洁、直接的回复风格"));
}

#[test]
fn u4_lazy_migrates_legacy_core_memory_into_project_bucket() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let app_data_dir = tmp.path().join("app-data");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("nested")).expect("create workspace");
    std::fs::create_dir_all(app_data_dir.join("shared").join("cognitive"))
        .expect("create legacy cognitive dir");
    std::fs::write(
        app_data_dir.join("shared").join("cognitive").join("mem.md"),
        "旧核心记忆：优先用 cargo test 验证 Rust 改动。",
    )
    .expect("write legacy mem");

    let service = ProjectMemoryService::new(&app_data_dir, workspace.join("nested"));
    let context = service
        .load_context("cargo test")
        .expect("load migrated context");

    assert!(
        service.memory_root().starts_with(&app_data_dir),
        "project memory must live under app data, got {}",
        service.memory_root().display()
    );
    assert!(
        !service.memory_root().starts_with(&workspace),
        "project memory bucket must not be written into workspace"
    );
    assert!(
        context.render_for_prompt().contains("旧核心记忆"),
        "lazy migration should make legacy core memory available in runtime context"
    );
    assert!(
        std::fs::read_to_string(service.entrypoint_path())
            .expect("read index after migration")
            .contains("legacy-core-memory"),
        "migration should leave an auditable MEMORY.md pointer"
    );
}

#[test]
fn u4_recall_uses_relevant_entries_not_full_memory_blob() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let app_data_dir = tmp.path().join("app-data");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let service = ProjectMemoryService::new(&app_data_dir, &workspace);
    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::UserPreference,
            name: "薪资分析偏好箱线图".to_string(),
            description: "用户分析薪资分布时偏好 box plot".to_string(),
            content: "遇到薪资分布分析时优先建议箱线图，而不是饼图。".to_string(),
            source: None,
        })
        .expect("save user preference memory");
    service
        .save_memory(ProjectMemoryEntryDraft {
            memory_type: ProjectMemoryType::ProjectConstraint,
            name: "移动端发版冻结".to_string(),
            description: "2026-04-25 起非关键改动暂停合并".to_string(),
            content: "发版冻结期间，非关键 PR 不建议合并。".to_string(),
            source: None,
        })
        .expect("save project constraint memory");

    let context = service
        .load_context("请帮我分析薪资分布，优先看 box plot")
        .expect("load context");

    let rendered = context.render_for_prompt();
    assert!(rendered.contains("薪资分析偏好箱线图"));
    assert!(rendered.contains("箱线图"));
    assert!(
        !rendered.contains("移动端发版冻结"),
        "runtime recall should surface relevant entries instead of dumping the full memory set"
    );
}

#[test]
fn u4_distill_rebuilds_memory_index_from_existing_entry_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let app_data_dir = tmp.path().join("app-data");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let service = ProjectMemoryService::new(&app_data_dir, &workspace);
    std::fs::create_dir_all(service.memory_root().join("entries")).expect("create entries dir");
    std::fs::write(
        service
            .memory_root()
            .join("entries")
            .join("frozen-window.md"),
        r#"---
type: project_constraint
name: 发布冻结窗口
description: 2026-04-25 起非关键改动暂停合并
---

移动端发版前暂停非关键改动合并。
"#,
    )
    .expect("write entry file");
    std::fs::write(service.entrypoint_path(), "").expect("seed blank index");

    let rebuilt = service.distill_index().expect("distill index");
    let index = std::fs::read_to_string(service.entrypoint_path()).expect("read rebuilt index");

    assert_eq!(rebuilt, 1, "distill should report rebuilt entry count");
    assert!(index.contains("发布冻结窗口"));
    assert!(index.contains("2026-04-25 起非关键改动暂停合并"));
}

struct ProjectMemoryCapturingExecutor {
    workspace_path: PathBuf,
    project_memory: ProjectMemoryContext,
    renlijia_md_files: Vec<RenlijiaMdFile>,
    captured_messages: Mutex<Vec<Vec<serde_json::Value>>>,
    captured_dynamic_contexts: Mutex<Vec<String>>,
    load_project_memory_calls: Mutex<u32>,
    load_core_memory_calls: Mutex<u32>,
}

impl ProjectMemoryCapturingExecutor {
    fn new(
        workspace_path: PathBuf,
        project_memory: ProjectMemoryContext,
        renlijia_md_files: Vec<RenlijiaMdFile>,
    ) -> Self {
        Self {
            workspace_path,
            project_memory,
            renlijia_md_files,
            captured_messages: Mutex::new(Vec::new()),
            captured_dynamic_contexts: Mutex::new(Vec::new()),
            load_project_memory_calls: Mutex::new(0),
            load_core_memory_calls: Mutex::new(0),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for ProjectMemoryCapturingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.captured_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());
        self.captured_dynamic_contexts
            .lock()
            .unwrap()
            .push(input.dynamic_context.to_string());

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

    async fn load_renlijia_md(&self, workspace_path: &Path) -> Result<Vec<RenlijiaMdFile>, TurnError> {
        assert_eq!(workspace_path, self.workspace_path.as_path());
        Ok(self.renlijia_md_files.clone())
    }

    async fn load_project_memory(
        &self,
        workspace_path: &Path,
        query: &str,
    ) -> Result<ProjectMemoryContext, TurnError> {
        assert_eq!(workspace_path, self.workspace_path.as_path());
        assert!(
            query.contains("箱线图"),
            "project memory recall should receive the current user query"
        );
        *self.load_project_memory_calls.lock().unwrap() += 1;
        Ok(self.project_memory.clone())
    }

    async fn load_core_memory(&self, _conversation_id: &str) -> Result<String, TurnError> {
        *self.load_core_memory_calls.lock().unwrap() += 1;
        Ok("legacy core memory should stay unused on runtime-native path".to_string())
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
}

#[tokio::test]
async fn u4_driver_injects_project_memory_as_separate_runtime_context() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let renlijia_path = workspace.join("RENLIJIA.md");
    let project_memory = ProjectMemoryContext {
        index_text: "- [薪资分析偏好箱线图](entries/boxplot.md) - 用户偏好 box plot".to_string(),
        recalled_entries: vec![],
    };
    let executor = Arc::new(ProjectMemoryCapturingExecutor::new(
        workspace.clone(),
        project_memory,
        vec![RenlijiaMdFile {
            path: renlijia_path.clone(),
            content: "project instructions".to_string(),
        }],
    ));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus, executor.clone());
    let mut turn = make_test_turn("conv-project-memory");
    let request = ChatTurnRequest::new(
        "conv-project-memory",
        "请继续分析薪资分布，优先用箱线图",
        vec![],
    );

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    assert_eq!(
        *executor.load_project_memory_calls.lock().unwrap(),
        1,
        "driver should load runtime-native project memory once per turn"
    );
    assert_eq!(
        *executor.load_core_memory_calls.lock().unwrap(),
        0,
        "driver must not fall back to legacy core_memory on the production path"
    );

    let captured_dynamic_contexts = executor.captured_dynamic_contexts.lock().unwrap();
    assert!(
        captured_dynamic_contexts[0].contains("[项目记忆]"),
        "dynamic context must contain a dedicated project memory block"
    );
    assert!(
        captured_dynamic_contexts[0].contains("薪资分析偏好箱线图"),
        "dynamic context must contain rendered project memory index"
    );

    let captured_messages = executor.captured_messages.lock().unwrap();
    let first_call_messages = &captured_messages[0];
    let combined_messages = first_call_messages
        .iter()
        .filter_map(|msg| msg.get("content").and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !combined_messages.contains("[项目记忆]"),
        "project memory should stay in dynamic context instead of being mixed into message history"
    );
    assert!(
        combined_messages.contains("# renlijiaMd"),
        "RENLIJIA.md should remain a separate context message"
    );
}

#[test]
fn u4_compat_tool_definition_helper_excludes_retired_memory_tools() {
    let names = app_lib::llm::tools::get_tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    for retired in [
        "save_memory",
        "search_memory",
        "core_memory",
        "distill_memory",
    ] {
        assert!(
            !names.iter().any(|name| name == retired),
            "compat llm::tools helper must not expose retired memory tool '{}'",
            retired
        );
    }
}
