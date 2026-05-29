use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use app_lib::runtime::agents_md::AgentsMdFile;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, RuntimeChatTurnDriver, RuntimeLlmExecutor,
    TurnError,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::project_memory::ProjectMemoryContext;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::permission::AllowAllPermissionPipeline;
use app_lib::runtime::tools::ToolDispatcher;
use async_trait::async_trait;
use serde_json::{json, Value};

enum ProjectMemoryBehavior {
    Context(ProjectMemoryContext),
    Error(String),
}

struct MemoryTurnExecutor {
    workspace_path: PathBuf,
    project_memory: ProjectMemoryBehavior,
    core_memory: String,
    env_info: String,
    agents_files: Vec<AgentsMdFile>,
    responses: Mutex<Vec<LlmStepResult>>,
    load_project_memory_calls: Mutex<Vec<(PathBuf, String)>>,
    load_core_memory_calls: Mutex<Vec<String>>,
    dynamic_contexts: Mutex<Vec<String>>,
    messages: Mutex<Vec<Vec<Value>>>,
}

impl MemoryTurnExecutor {
    fn new(workspace_path: PathBuf, project_memory: ProjectMemoryContext) -> Self {
        Self {
            workspace_path,
            project_memory: ProjectMemoryBehavior::Context(project_memory),
            core_memory: String::new(),
            env_info: String::new(),
            agents_files: Vec::new(),
            responses: Mutex::new(vec![content_complete()]),
            load_project_memory_calls: Mutex::new(Vec::new()),
            load_core_memory_calls: Mutex::new(Vec::new()),
            dynamic_contexts: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        }
    }

    fn with_core_memory(mut self, core_memory: impl Into<String>) -> Self {
        self.core_memory = core_memory.into();
        self
    }

    fn with_env_info(mut self, env_info: impl Into<String>) -> Self {
        self.env_info = env_info.into();
        self
    }

    fn with_agents_files(mut self, agents_files: Vec<AgentsMdFile>) -> Self {
        self.agents_files = agents_files;
        self
    }

    fn with_responses(mut self, responses: Vec<LlmStepResult>) -> Self {
        self.responses = Mutex::new(responses);
        self
    }

    fn with_project_memory_error(workspace_path: PathBuf, error: impl Into<String>) -> Self {
        Self {
            workspace_path,
            project_memory: ProjectMemoryBehavior::Error(error.into()),
            core_memory: String::new(),
            env_info: String::new(),
            agents_files: Vec::new(),
            responses: Mutex::new(vec![content_complete()]),
            load_project_memory_calls: Mutex::new(Vec::new()),
            load_core_memory_calls: Mutex::new(Vec::new()),
            dynamic_contexts: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        }
    }

    fn dynamic_contexts(&self) -> Vec<String> {
        self.dynamic_contexts.lock().unwrap().clone()
    }

    fn messages(&self) -> Vec<Vec<Value>> {
        self.messages.lock().unwrap().clone()
    }

    fn project_calls(&self) -> Vec<(PathBuf, String)> {
        self.load_project_memory_calls.lock().unwrap().clone()
    }

    fn core_calls(&self) -> Vec<String> {
        self.load_core_memory_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for MemoryTurnExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.dynamic_contexts
            .lock()
            .unwrap()
            .push(input.dynamic_context.to_string());
        self.messages.lock().unwrap().push(input.messages.clone());
        let mut responses = self.responses.lock().unwrap();
        Ok(responses.remove(0))
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(self.workspace_path.clone())
    }

    async fn get_env_info(&self, _conversation_id: &str) -> Result<String, TurnError> {
        Ok(self.env_info.clone())
    }

    async fn load_agents_md(
        &self,
        _authorized_workspace: Option<&app_lib::runtime::store::AuthorizedWorkspaceRef>,
    ) -> Result<Vec<AgentsMdFile>, TurnError> {
        Ok(self.agents_files.clone())
    }

    async fn load_project_memory(
        &self,
        workspace_path: &Path,
        query: &str,
    ) -> Result<ProjectMemoryContext, TurnError> {
        self.load_project_memory_calls
            .lock()
            .unwrap()
            .push((workspace_path.to_path_buf(), query.to_string()));
        match &self.project_memory {
            ProjectMemoryBehavior::Context(ctx) => Ok(ctx.clone()),
            ProjectMemoryBehavior::Error(message) => {
                Err(TurnError::PersistenceError(message.clone()))
            }
        }
    }

    async fn load_core_memory(&self, conversation_id: &str) -> Result<String, TurnError> {
        self.load_core_memory_calls
            .lock()
            .unwrap()
            .push(conversation_id.to_string());
        Ok(self.core_memory.clone())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[Value],
        _generated_file_ids: &[String],
        _file_metas: &[Value],
        _thinking_blocks: &[Value],
    ) -> Result<String, TurnError> {
        Ok("assistant-msg".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

fn make_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("run-memory-turn"), "hi".to_string())
}

fn driver(executor: Arc<MemoryTurnExecutor>) -> RuntimeChatTurnDriver {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::with_dispatcher(dispatcher),
        RuntimeEventBus::new(),
        executor,
    )
}

fn content_complete() -> LlmStepResult {
    LlmStepResult::ContentComplete {
        content: "ok".to_string(),
        tokens_in: 1,
        tokens_out: 1,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
        stop_reason: Some("end_turn".to_string()),
    }
}

fn tool_call_step(id: &str) -> LlmStepResult {
    LlmStepResult::ToolCalls {
        assistant_content: "".to_string(),
        tool_calls: vec![RuntimeToolCallRequest {
            tool_call_id: id.to_string(),
            tool_name: "unknown_tool".to_string(),
            args: json!({}),
            purpose: None,
        }],
        tokens_in: 1,
        tokens_out: 1,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0, thinking_blocks: Vec::new(),
    }
}

fn project_memory() -> ProjectMemoryContext {
    ProjectMemoryContext {
        index_text: "- [薪资分析偏好箱线图](entries/boxplot.md) - 用户偏好 box plot".to_string(),
        recalled_entries: Vec::new(),
    }
}

fn combined_message_content(messages: &[Value]) -> String {
    messages
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn turn_loads_project_memory_once_with_current_user_message_as_query() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let executor = Arc::new(MemoryTurnExecutor::new(workspace.clone(), project_memory()));
    let mut turn = make_turn("conv-memory-query");
    let request = ChatTurnRequest::new(
        "conv-memory-query",
        "请继续分析薪资分布，优先用箱线图",
        vec![],
    );

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    let calls = executor.project_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, workspace);
    assert_eq!(calls[0].1, "请继续分析薪资分布，优先用箱线图");
}

#[tokio::test]
async fn project_memory_is_injected_into_dynamic_context_before_env_info() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let executor = Arc::new(
        MemoryTurnExecutor::new(workspace, project_memory())
            .with_env_info("# env_info\nPlatform: test"),
    );
    let mut turn = make_turn("conv-memory-dynamic");
    let request = ChatTurnRequest::new("conv-memory-dynamic", "薪资 箱线图", vec![]);

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    let dynamic = &executor.dynamic_contexts()[0];
    assert!(dynamic.starts_with("[动态上下文 — 请勿回复此消息]"));
    assert!(dynamic.contains("[项目记忆]"));
    assert!(dynamic.contains("薪资分析偏好箱线图"));
    assert!(dynamic.find("[项目记忆]").unwrap() < dynamic.find("# env_info").unwrap());
}

#[tokio::test]
async fn project_memory_stays_out_of_message_history() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let executor = Arc::new(MemoryTurnExecutor::new(workspace, project_memory()));
    let mut turn = make_turn("conv-memory-not-message");
    let request = ChatTurnRequest::new("conv-memory-not-message", "薪资 箱线图", vec![]);

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    let dynamic = &executor.dynamic_contexts()[0];
    assert!(dynamic.contains("薪资分析偏好箱线图"));
    let combined = combined_message_content(&executor.messages()[0]);
    assert!(!combined.contains("[项目记忆]"));
    assert!(!combined.contains("薪资分析偏好箱线图"));
}

#[tokio::test]
async fn empty_project_memory_falls_back_to_legacy_core_memory() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let executor = Arc::new(
        MemoryTurnExecutor::new(workspace, ProjectMemoryContext::default())
            .with_core_memory("旧核心记忆内容"),
    );
    let mut turn = make_turn("conv-memory-core");
    let request = ChatTurnRequest::new("conv-memory-core", "anything", vec![]);

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    assert_eq!(executor.project_calls().len(), 1);
    assert_eq!(executor.core_calls(), vec!["conv-memory-core".to_string()]);
    let dynamic = &executor.dynamic_contexts()[0];
    assert!(dynamic.contains("[核心记忆]"));
    assert!(dynamic.contains("旧核心记忆内容"));
    assert!(!dynamic.contains("[项目记忆]"));
}

#[tokio::test]
async fn non_empty_project_memory_skips_legacy_core_memory() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let executor = Arc::new(
        MemoryTurnExecutor::new(workspace, project_memory()).with_core_memory("旧核心记忆内容"),
    );
    let mut turn = make_turn("conv-memory-no-core");
    let request = ChatTurnRequest::new("conv-memory-no-core", "薪资 箱线图", vec![]);

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    assert_eq!(executor.project_calls().len(), 1);
    assert!(executor.core_calls().is_empty());
    let dynamic = &executor.dynamic_contexts()[0];
    assert!(dynamic.contains("[项目记忆]"));
    assert!(!dynamic.contains("[核心记忆]"));
}

#[tokio::test]
async fn multi_step_turn_reuses_single_project_memory_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let executor = Arc::new(
        MemoryTurnExecutor::new(workspace, project_memory()).with_responses(vec![
            tool_call_step("tc-memory-1"),
            tool_call_step("tc-memory-2"),
            content_complete(),
        ]),
    );
    let mut turn = make_turn("conv-memory-multi");
    let request = ChatTurnRequest::new("conv-memory-multi", "薪资 箱线图", vec![]);

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    assert_eq!(executor.project_calls().len(), 1);
    let contexts = executor.dynamic_contexts();
    assert_eq!(contexts.len(), 3);
    for dynamic in contexts {
        assert!(dynamic.contains("[项目记忆]"));
        assert!(dynamic.contains("薪资分析偏好箱线图"));
    }
}

#[tokio::test]
async fn project_memory_load_failure_degrades_without_blocking_turn() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let executor = Arc::new(MemoryTurnExecutor::with_project_memory_error(
        workspace,
        "project memory exploded",
    ));
    let mut turn = make_turn("conv-memory-error");
    let request = ChatTurnRequest::new("conv-memory-error", "anything", vec![]);

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    assert_eq!(executor.dynamic_contexts().len(), 1);
    let dynamic = &executor.dynamic_contexts()[0];
    assert!(!dynamic.contains("project memory exploded"));
    assert!(!dynamic.contains("[项目记忆]"));
    assert!(!dynamic.contains("[核心记忆]"));
}

#[tokio::test]
async fn empty_project_memory_rendering_does_not_inject_empty_memory_headers() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let executor = Arc::new(MemoryTurnExecutor::new(
        workspace,
        ProjectMemoryContext::default(),
    ));
    let mut turn = make_turn("conv-memory-empty");
    let request = ChatTurnRequest::new("conv-memory-empty", "anything", vec![]);

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    let dynamic = &executor.dynamic_contexts()[0];
    assert!(dynamic.starts_with("[动态上下文 — 请勿回复此消息]"));
    assert!(!dynamic.contains("[项目记忆]"));
    assert!(!dynamic.contains("[核心记忆]"));
}

#[tokio::test]
async fn project_memory_agents_md_and_env_info_remain_separate_context_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let renlijia_path = workspace.join("RENLIJIA.md");
    let executor = Arc::new(
        MemoryTurnExecutor::new(workspace.clone(), project_memory())
            .with_env_info("# env_info\nPlatform: test")
            .with_agents_files(vec![AgentsMdFile {
                path: renlijia_path,
                content: "项目指令内容".to_string(),
            }]),
    );
    let mut turn = make_turn("conv-memory-separate");
    let request = ChatTurnRequest::new("conv-memory-separate", "薪资 箱线图", vec![]);

    driver(executor.clone())
        .run_chat_turn(&mut turn, &request)
        .await
        .unwrap();

    let dynamic = &executor.dynamic_contexts()[0];
    assert!(dynamic.contains("[项目记忆]"));
    assert!(dynamic.contains("薪资分析偏好箱线图"));
    assert!(dynamic.contains("# env_info"));
    assert!(!dynamic.contains("项目指令内容"));

    let combined = combined_message_content(&executor.messages()[0]);
    assert!(combined.contains("项目指令内容"));
    assert!(!combined.contains("薪资分析偏好箱线图"));
}
