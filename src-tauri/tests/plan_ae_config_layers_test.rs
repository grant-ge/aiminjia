use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, ResolvedLlmSettings, RuntimeChatTurnDriver,
    RuntimeLlmExecutor, TurnError,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::storage::file_store::AppStorage;
use async_trait::async_trait;
use serde_json::json;
use tempfile::TempDir;

struct CapturingSettingsExecutor {
    settings: ResolvedLlmSettings,
    seen_models: Mutex<Vec<String>>,
    seen_api_keys: Mutex<Vec<String>>,
}

impl CapturingSettingsExecutor {
    fn new(settings: ResolvedLlmSettings) -> Self {
        Self {
            settings,
            seen_models: Mutex::new(Vec::new()),
            seen_api_keys: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CapturingSettingsExecutor {
    async fn load_llm_settings_for_turn(
        &self,
        _request: &ChatTurnRequest,
    ) -> Result<ResolvedLlmSettings, TurnError> {
        Ok(self.settings.clone())
    }

    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.seen_models
            .lock()
            .unwrap()
            .push(input.llm_settings.primary_model.clone());
        self.seen_api_keys
            .lock()
            .unwrap()
            .push(input.llm_settings.primary_api_key.clone());
        Ok(LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("msg-ae".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])  // 显式声明此 mock 不关心 tool_defs
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn setup_storage() -> (AppStorage, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let storage = AppStorage::new(dir.path()).expect("storage");
    (storage, dir)
}

fn write_workspace_settings(workspace: &Path, value: serde_json::Value) {
    let aijia_dir = workspace.join(".aijia");
    fs::create_dir_all(&aijia_dir).expect("create .aijia");
    fs::write(
        aijia_dir.join("settings.json"),
        serde_json::to_vec_pretty(&value).expect("serialize workspace settings"),
    )
    .expect("write workspace settings");
}

#[test]
fn ae1_model_override_persisted() {
    let (storage, _dir) = setup_storage();
    storage
        .create_conversation("conv-ae1", "Plan AE")
        .expect("create conversation");

    storage
        .set_conversation_model_override("conv-ae1", Some("claude".to_string()))
        .expect("set override");

    let model = storage
        .get_conversation_model_override("conv-ae1")
        .expect("get override");
    assert_eq!(model.as_deref(), Some("claude"));
}

#[test]
fn ae1_model_override_default_none() {
    let (storage, dir) = setup_storage();
    let conv_dir = dir.path().join("conversations").join("conv-legacy");
    fs::create_dir_all(&conv_dir).expect("create conversation dir");
    fs::write(
        conv_dir.join("conv.json"),
        serde_json::to_vec_pretty(&json!({
            "id": "conv-legacy",
            "title": "Legacy",
            "mode": "daily",
            "createdAt": "2026-04-19T00:00:00Z",
            "updatedAt": "2026-04-19T00:00:00Z",
            "isArchived": false
        }))
        .expect("serialize legacy meta"),
    )
    .expect("write legacy conv.json");

    let model = storage
        .get_conversation_model_override("conv-legacy")
        .expect("load legacy override");
    assert_eq!(model, None);
}

#[test]
fn ae1_model_override_clear() {
    let (storage, dir) = setup_storage();
    storage
        .create_conversation("conv-clear", "Clear")
        .expect("create conversation");
    storage
        .set_conversation_model_override("conv-clear", Some("claude".to_string()))
        .expect("set override");
    storage
        .set_conversation_model_override("conv-clear", None)
        .expect("clear override");

    let raw = fs::read_to_string(
        dir.path()
            .join("conversations")
            .join("conv-clear")
            .join("conv.json"),
    )
    .expect("read conv.json");
    assert!(!raw.contains("modelOverride"));
}

#[test]
fn ae4_workspace_settings_loaded() {
    let (storage, dir) = setup_storage();
    let workspace = dir.path().join("workspace-loaded");
    fs::create_dir_all(&workspace).expect("create workspace");
    storage
        .set_setting("primaryModel", "deepseek-v3")
        .expect("set global primary model");
    write_workspace_settings(&workspace, json!({ "primaryModel": "claude" }));

    let settings = storage
        .get_effective_settings(Some(&workspace))
        .expect("load effective settings");
    assert_eq!(
        settings.get("primaryModel").map(String::as_str),
        Some("claude")
    );
}

#[test]
fn ae4_workspace_settings_absent() {
    let (storage, dir) = setup_storage();
    let workspace = dir.path().join("workspace-absent");
    fs::create_dir_all(&workspace).expect("create workspace");
    storage
        .set_setting("primaryModel", "deepseek-v3")
        .expect("set global primary model");

    let settings = storage
        .get_effective_settings(Some(&workspace))
        .expect("load effective settings");
    assert_eq!(
        settings.get("primaryModel").map(String::as_str),
        Some("deepseek-v3")
    );
}

#[test]
fn ae4_workspace_settings_partial_override() {
    let (storage, dir) = setup_storage();
    let workspace = dir.path().join("workspace-partial");
    fs::create_dir_all(&workspace).expect("create workspace");
    storage
        .set_setting("primaryModel", "deepseek-v3")
        .expect("set global primary model");
    storage
        .set_setting("autoModelRouting", "true")
        .expect("set global auto routing");
    write_workspace_settings(&workspace, json!({ "primaryModel": "claude" }));

    let settings = storage
        .get_effective_settings(Some(&workspace))
        .expect("load effective settings");
    assert_eq!(
        settings.get("primaryModel").map(String::as_str),
        Some("claude")
    );
    assert_eq!(
        settings.get("autoModelRouting").map(String::as_str),
        Some("true")
    );
}

#[test]
fn ae4_workspace_settings_malformed() {
    let (storage, dir) = setup_storage();
    let workspace = dir.path().join("workspace-malformed");
    let aijia_dir = workspace.join(".aijia");
    fs::create_dir_all(&aijia_dir).expect("create .aijia");
    fs::write(aijia_dir.join("settings.json"), b"{not-json").expect("write malformed");
    storage
        .set_setting("primaryModel", "deepseek-v3")
        .expect("set global primary model");

    let settings = storage
        .get_effective_settings(Some(&workspace))
        .expect("load effective settings");
    assert_eq!(
        settings.get("primaryModel").map(String::as_str),
        Some("deepseek-v3")
    );
}

#[test]
fn ae4_workspace_settings_ignores_sensitive_keys() {
    let (storage, dir) = setup_storage();
    let workspace = dir.path().join("workspace-sensitive");
    fs::create_dir_all(&workspace).expect("create workspace");
    storage
        .set_setting("primaryModel", "deepseek-v3")
        .expect("set global primary model");
    storage
        .set_setting("primaryApiKey", "encrypted-global-key")
        .expect("set global api key");
    write_workspace_settings(
        &workspace,
        json!({
            "primaryModel": "claude",
            "primaryApiKey": "plaintext-should-be-ignored"
        }),
    );

    let settings = storage
        .get_effective_settings(Some(&workspace))
        .expect("load effective settings");
    assert_eq!(
        settings.get("primaryModel").map(String::as_str),
        Some("claude")
    );
    assert_eq!(
        settings.get("primaryApiKey").map(String::as_str),
        Some("encrypted-global-key")
    );
}

#[tokio::test]
async fn ae2_model_override_applied_to_resolved_settings() {
    let executor = Arc::new(CapturingSettingsExecutor::new(ResolvedLlmSettings {
        primary_model: "claude".to_string(),
        primary_api_key: "pk-global".to_string(),
        auto_model_routing: true,
        custom_model_endpoint: String::new(),
        custom_model_name: String::new(),
        use_cloud: false,
        cloud_model: String::new(),
        cloud_model_type: String::new(),
        thinking_type: "disabled".to_string(),
        thinking_budget_tokens: 8000,
        masking_level: "strict".to_string(),
    }));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let mut turn = TurnState::new(
        IdentityMapping::from_legacy_conversation_id("conv-ae2-override"),
        RunId::new("run-ae2-override"),
        "hello".to_string(),
    );
    let request = ChatTurnRequest::new("conv-ae2-override", "hello", vec![]);

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(
        executor.seen_models.lock().unwrap().as_slice(),
        &["claude".to_string()]
    );
}

#[tokio::test]
async fn ae2_no_override_falls_back_to_effective_settings() {
    let executor = Arc::new(CapturingSettingsExecutor::new(ResolvedLlmSettings {
        primary_model: "deepseek-v3".to_string(),
        primary_api_key: "pk-global".to_string(),
        auto_model_routing: true,
        custom_model_endpoint: String::new(),
        custom_model_name: String::new(),
        use_cloud: false,
        cloud_model: String::new(),
        cloud_model_type: String::new(),
        thinking_type: "disabled".to_string(),
        thinking_budget_tokens: 8000,
        masking_level: "strict".to_string(),
    }));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let mut turn = TurnState::new(
        IdentityMapping::from_legacy_conversation_id("conv-ae2-global"),
        RunId::new("run-ae2-global"),
        "hello".to_string(),
    );
    let request = ChatTurnRequest::new("conv-ae2-global", "hello", vec![]);

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(
        executor.seen_models.lock().unwrap().as_slice(),
        &["deepseek-v3".to_string()]
    );
}

#[tokio::test]
async fn ae2_empty_override_treated_as_none() {
    let executor = Arc::new(CapturingSettingsExecutor::new(ResolvedLlmSettings {
        primary_model: "deepseek-v3".to_string(),
        primary_api_key: "pk-global".to_string(),
        auto_model_routing: true,
        custom_model_endpoint: String::new(),
        custom_model_name: String::new(),
        use_cloud: false,
        cloud_model: String::new(),
        cloud_model_type: String::new(),
        thinking_type: "disabled".to_string(),
        thinking_budget_tokens: 8000,
        masking_level: "strict".to_string(),
    }));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let mut turn = TurnState::new(
        IdentityMapping::from_legacy_conversation_id("conv-ae2-empty"),
        RunId::new("run-ae2-empty"),
        "hello".to_string(),
    );
    let request = ChatTurnRequest::new("conv-ae2-empty", "hello", vec![]);

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(
        executor.seen_models.lock().unwrap().as_slice(),
        &["deepseek-v3".to_string()]
    );
}

#[test]
fn review_ae_conversation_meta_has_model_override_field() {
    let (storage, dir) = setup_storage();
    let _ = storage;
    let conv_dir = dir.path().join("conversations").join("conv-review");
    fs::create_dir_all(&conv_dir).expect("create conversation dir");
    fs::write(
        conv_dir.join("conv.json"),
        serde_json::to_vec_pretty(&json!({
            "id": "conv-review",
            "title": "Legacy",
            "mode": "daily",
            "createdAt": "2026-04-19T00:00:00Z",
            "updatedAt": "2026-04-19T00:00:00Z",
            "isArchived": false
        }))
        .expect("serialize legacy meta"),
    )
    .expect("write legacy conv.json");
    let raw = fs::read_to_string(conv_dir.join("conv.json")).expect("read conv.json");
    assert!(!raw.contains("modelOverride"));
}

#[test]
fn review_ae_workspace_settings_does_not_expose_workspace_path() {
    let source = fs::read_to_string(
        repo_root().join("src-tauri/src/storage/file_store/workspace_settings.rs"),
    )
    .expect("read workspace settings source");
    assert!(!source.contains("workspace_path"));
}

#[test]
fn review_ae_model_override_none_does_not_touch_primary_model() {
    let settings = ResolvedLlmSettings {
        primary_model: "deepseek-v3".to_string(),
        ..ResolvedLlmSettings::default()
    };
    assert_eq!(settings.primary_model, "deepseek-v3");
}

#[test]
fn review_ae_workspace_settings_only_merges_allowed_keys() {
    let source = fs::read_to_string(
        repo_root().join("src-tauri/src/storage/file_store/workspace_settings.rs"),
    )
    .expect("read workspace settings source");
    assert!(source.contains("primary_model"));
    assert!(!source.contains("primary_api_key"));
    assert!(!source.contains("tavily_api_key"));
    assert!(!source.contains("bocha_api_key"));
}

#[test]
fn review_ae_workspace_settings_never_override_api_keys() {
    let (storage, dir) = setup_storage();
    let workspace = dir.path().join("workspace-review-api-key");
    fs::create_dir_all(&workspace).expect("create workspace");
    storage
        .set_setting("primaryApiKey", "global-encrypted")
        .expect("set global api key");
    write_workspace_settings(
        &workspace,
        json!({ "primaryApiKey": "workspace-plaintext" }),
    );

    let settings = storage
        .get_effective_settings(Some(&workspace))
        .expect("load effective settings");
    assert_eq!(
        settings.get("primaryApiKey").map(String::as_str),
        Some("global-encrypted")
    );
}

#[test]
fn review_ae_runtime_does_not_import_tauri() {
    fn visit(dir: &Path, violations: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("read runtime dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.is_dir() {
                visit(&path, violations);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                let source = fs::read_to_string(&path).expect("read runtime source");
                if source.contains("use tauri::") {
                    violations.push(path.display().to_string());
                }
            }
        }
    }

    let mut violations = Vec::new();
    visit(&repo_root().join("src-tauri/src/runtime"), &mut violations);
    assert!(
        violations.is_empty(),
        "runtime imported tauri in: {violations:?}"
    );
}
