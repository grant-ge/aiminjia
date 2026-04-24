use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use app_lib::llm::masking::{MaskingContext, MaskingLevel};
use app_lib::llm::streaming::ChatMessage;
use app_lib::models::settings::AppSettings;
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
use tempfile::TempDir;

#[derive(Clone)]
enum SettingsSource {
    Fixed(&'static str),
    FromStorage {
        storage: Arc<AppStorage>,
        workspace_path: Option<PathBuf>,
    },
}

struct MaskingProbeExecutor {
    responses: Mutex<Vec<LlmStepResult>>,
    settings_source: SettingsSource,
    load_calls: AtomicUsize,
    seen_masking_levels: Mutex<Vec<String>>,
    seen_masked_batches: Mutex<Vec<Vec<String>>>,
}

impl MaskingProbeExecutor {
    fn new(settings_source: SettingsSource, responses: Vec<LlmStepResult>) -> Self {
        Self {
            responses: Mutex::new(responses),
            settings_source,
            load_calls: AtomicUsize::new(0),
            seen_masking_levels: Mutex::new(Vec::new()),
            seen_masked_batches: Mutex::new(Vec::new()),
        }
    }

    fn load_calls(&self) -> usize {
        self.load_calls.load(Ordering::SeqCst)
    }

    fn seen_masking_levels(&self) -> Vec<String> {
        self.seen_masking_levels.lock().unwrap().clone()
    }

    fn latest_masked_contents(&self) -> Vec<String> {
        self.seen_masked_batches
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for MaskingProbeExecutor {
    async fn load_llm_settings_for_turn(
        &self,
        _request: &ChatTurnRequest,
    ) -> Result<ResolvedLlmSettings, TurnError> {
        self.load_calls.fetch_add(1, Ordering::SeqCst);
        let masking_level = match &self.settings_source {
            SettingsSource::Fixed(value) => value.to_string(),
            SettingsSource::FromStorage {
                storage,
                workspace_path,
            } => {
                let settings_map = storage
                    .get_effective_settings(workspace_path.as_deref())
                    .map_err(|err| TurnError::PersistenceError(err.to_string()))?;
                let settings = AppSettings::from_string_map(&settings_map);
                MaskingLevel::from_str_or_strict(&settings.data_masking_level)
                    .to_str()
                    .to_string()
            }
        };

        Ok(ResolvedLlmSettings {
            primary_model: "deepseek-v3".to_string(),
            primary_api_key: "pk-mask".to_string(),
            auto_model_routing: true,
            custom_model_endpoint: String::new(),
            custom_model_name: String::new(),
            use_cloud: false,
            cloud_model: String::new(),
            cloud_model_type: String::new(),
            thinking_type: "disabled".to_string(),
            thinking_budget_tokens: 8000,
            masking_level,
        })
    }

    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.seen_masking_levels
            .lock()
            .unwrap()
            .push(input.masking_level.to_string());

        let chat_messages: Vec<ChatMessage> = input
            .messages
            .iter()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect();
        let mut mask_ctx = MaskingContext::new(MaskingLevel::from_str_or_strict(input.masking_level));
        let masked = mask_ctx.mask_messages(&chat_messages);
        self.seen_masked_batches.lock().unwrap().push(
            masked
                .into_iter()
                .map(|message| message.content)
                .collect(),
        );

        Ok(self.responses.lock().unwrap().remove(0))
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("msg-mask".to_string())
    }
}

fn make_turn(conversation_id: &str, content: &str) -> (TurnState, ChatTurnRequest) {
    (
        TurnState::new(
            IdentityMapping::from_legacy_conversation_id(conversation_id),
            RunId::new(format!("run-{conversation_id}")),
            content.to_string(),
        ),
        ChatTurnRequest::new(conversation_id, content, vec![]),
    )
}

fn content_complete() -> LlmStepResult {
    LlmStepResult::ContentComplete {
        content: "done".to_string(),
        tokens_in: 1,
        tokens_out: 1,
        stop_reason: Some("end_turn".to_string()),
    }
}

#[tokio::test]
async fn masking_level_relaxed_turn_keeps_id_card_unmasked_for_llm() {
    let executor = Arc::new(MaskingProbeExecutor::new(
        SettingsSource::Fixed("relaxed"),
        vec![content_complete()],
    ));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let (mut turn, request) = make_turn(
        "conv-mask-relaxed",
        "请帮我检查身份证号110108199001011234是否会被改写",
    );

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(executor.seen_masking_levels(), vec!["relaxed".to_string()]);
    let user_content = executor
        .latest_masked_contents()
        .last()
        .cloned()
        .expect("user content should exist");
    assert!(user_content.contains("110108199001011234"));
    assert!(!user_content.contains("[ID_CARD_1]"));
}

#[tokio::test]
async fn masking_level_strict_turn_masks_all_pii_for_llm() {
    let executor = Arc::new(MaskingProbeExecutor::new(
        SettingsSource::Fixed("strict"),
        vec![content_complete()],
    ));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let (mut turn, request) = make_turn(
        "conv-mask-strict",
        "身份证110108199001011234，手机号13800138000，邮箱alice@example.com",
    );

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(executor.seen_masking_levels(), vec!["strict".to_string()]);
    let user_content = executor
        .latest_masked_contents()
        .last()
        .cloned()
        .expect("user content should exist");
    assert!(user_content.contains("[ID_CARD_1]"));
    assert!(user_content.contains("[PHONE_1]"));
    assert!(user_content.contains("[EMAIL_1]"));
    assert!(!user_content.contains("110108199001011234"));
    assert!(!user_content.contains("13800138000"));
    assert!(!user_content.contains("alice@example.com"));
}

#[tokio::test]
async fn masking_level_invalid_storage_values_fall_back_to_strict() {
    for raw_value in ["", "off"] {
        let dir = TempDir::new().expect("tempdir");
        let storage = Arc::new(AppStorage::new(dir.path()).expect("storage"));
        storage
            .set_setting("dataMaskingLevel", raw_value)
            .expect("set masking level");

        let executor = Arc::new(MaskingProbeExecutor::new(
            SettingsSource::FromStorage {
                storage: storage.clone(),
                workspace_path: None,
            },
            vec![content_complete()],
        ));
        let driver = RuntimeChatTurnDriver::with_llm_executor(
            QueryEngine::default(),
            RuntimeEventBus::new(),
            executor.clone(),
        );
        let (mut turn, request) = make_turn(
            &format!("conv-mask-invalid-{raw_value}"),
            "请处理身份证110108199001011234",
        );

        driver
            .run_chat_turn(&mut turn, &request)
            .await
            .expect("turn should succeed");

        assert_eq!(
            executor.seen_masking_levels(),
            vec!["strict".to_string()],
            "invalid storage value should fall back to strict: {raw_value:?}"
        );
        let user_content = executor
            .latest_masked_contents()
            .last()
            .cloned()
            .expect("user content should exist");
        assert!(user_content.contains("[ID_CARD_1]"));
        assert!(!user_content.contains("110108199001011234"));
    }
}

#[tokio::test]
async fn masking_level_workspace_setting_overrides_global_setting() {
    let dir = TempDir::new().expect("tempdir");
    let storage = Arc::new(AppStorage::new(dir.path()).expect("storage"));
    storage
        .set_setting("dataMaskingLevel", "strict")
        .expect("set global masking level");

    let workspace = dir.path().join("workspace-mask-relaxed");
    std::fs::create_dir_all(workspace.join(".aijia")).expect("create workspace settings dir");
    std::fs::write(
        workspace.join(".aijia").join("settings.json"),
        r#"{
  "dataMaskingLevel": "relaxed"
}"#,
    )
    .expect("write workspace settings");

    let executor = Arc::new(MaskingProbeExecutor::new(
        SettingsSource::FromStorage {
            storage: storage.clone(),
            workspace_path: Some(workspace),
        },
        vec![content_complete()],
    ));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let (mut turn, request) = make_turn(
        "conv-mask-workspace",
        "保留身份证110108199001011234原文",
    );

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(executor.seen_masking_levels(), vec!["relaxed".to_string()]);
    let user_content = executor
        .latest_masked_contents()
        .last()
        .cloned()
        .expect("user content should exist");
    assert!(user_content.contains("110108199001011234"));
    assert!(!user_content.contains("[ID_CARD_1]"));
}

#[tokio::test]
async fn masking_level_malformed_workspace_setting_silently_falls_back_to_global() {
    let dir = TempDir::new().expect("tempdir");
    let storage = Arc::new(AppStorage::new(dir.path()).expect("storage"));
    storage
        .set_setting("dataMaskingLevel", "standard")
        .expect("set global masking level");

    let workspace = dir.path().join("workspace-mask-malformed");
    std::fs::create_dir_all(workspace.join(".aijia")).expect("create workspace settings dir");
    std::fs::write(workspace.join(".aijia").join("settings.json"), b"{not-json")
        .expect("write malformed workspace settings");

    let executor = Arc::new(MaskingProbeExecutor::new(
        SettingsSource::FromStorage {
            storage: storage.clone(),
            workspace_path: Some(workspace),
        },
        vec![content_complete()],
    ));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let (mut turn, request) = make_turn(
        "conv-mask-malformed-workspace",
        "员工张三，身份证110108199001011234",
    );

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(executor.seen_masking_levels(), vec!["standard".to_string()]);
    let user_content = executor
        .latest_masked_contents()
        .last()
        .cloned()
        .expect("user content should exist");
    assert!(user_content.contains("[PERSON_1]"));
    assert!(user_content.contains("110108199001011234"));
    assert!(!user_content.contains("[ID_CARD_1]"));
}

#[tokio::test]
async fn masking_level_snapshot_is_reused_across_multi_step_turn() {
    let executor = Arc::new(MaskingProbeExecutor::new(
        SettingsSource::Fixed("relaxed"),
        vec![
            LlmStepResult::ToolCalls {
                assistant_content: "thinking-1".to_string(),
                tool_calls: vec![],
                tokens_in: 1,
                tokens_out: 1,
            },
            LlmStepResult::ToolCalls {
                assistant_content: "thinking-2".to_string(),
                tool_calls: vec![],
                tokens_in: 1,
                tokens_out: 1,
            },
            LlmStepResult::ToolCalls {
                assistant_content: "thinking-3".to_string(),
                tool_calls: vec![],
                tokens_in: 1,
                tokens_out: 1,
            },
            content_complete(),
        ],
    ));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let (mut turn, request) = make_turn(
        "conv-mask-loop",
        "多轮里保持身份证110108199001011234原样",
    );

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(
        executor.seen_masking_levels(),
        vec![
            "relaxed".to_string(),
            "relaxed".to_string(),
            "relaxed".to_string(),
            "relaxed".to_string(),
        ]
    );
    assert_eq!(
        executor.load_calls(),
        1,
        "settings should be loaded once and reused across steps"
    );
}

#[test]
fn resolved_llm_settings_default_masking_level_is_strict() {
    let settings = ResolvedLlmSettings::default();
    assert_eq!(settings.masking_level, "strict");
}
