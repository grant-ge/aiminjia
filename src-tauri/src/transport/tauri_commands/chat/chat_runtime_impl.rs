use super::chat_support::AgentGuard;
use super::*;
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
use crate::storage::file_store::RuntimeRepositoryFacade;

/// 根据当前 session 是否有授权目录，决定暴露给 LLM 的工具 schema 列表。
/// 有授权目录：暴露所有工具（含 workspace 工具）。
/// 无授权目录：排除 workspace 工具，避免 LLM 看到不可用工具。
const WORKSPACE_TOOL_NAMES: &[&str] = &[
    "list_directory",
    "read_workspace_file",
    "search_files",
    "get_file_info",
];

pub(crate) async fn build_visible_tool_defs(
    registry: &crate::plugin::registry::ToolRegistry,
    has_authorized_workspace: bool,
) -> Vec<crate::llm::streaming::ToolDefinition> {
    use crate::plugin::skill_trait::ToolFilter;
    if has_authorized_workspace {
        registry.get_schemas_filtered(&ToolFilter::All).await
    } else {
        registry.get_schemas_filtered(
            &ToolFilter::Exclude(
                WORKSPACE_TOOL_NAMES.iter().map(|s| s.to_string()).collect()
            )
        ).await
    }
}

/// 根据是否有授权目录，构建 analysis precompute 阶段使用的 SandboxConfig。
/// 独立为 helper 以便测试验证分支逻辑。
pub(crate) fn build_precompute_sandbox(
    workspace_path: &std::path::PathBuf,
    authorized_workspace: Option<&crate::runtime::store::AuthorizedWorkspaceRef>,
) -> crate::python::sandbox::SandboxConfig {
    match authorized_workspace {
        Some(aw) => crate::python::sandbox::SandboxConfig::for_workspace_with_authorized(
            workspace_path,
            vec![aw.root_path.clone()],
        ),
        None => crate::python::sandbox::SandboxConfig::for_workspace(workspace_path),
    }
}

/// 生成"已加载"的内存 key，用于 precompute 路径判断文件是否已加载。
/// scope_id 须与 `LoadFileParams::loaded_scope_id()` 保持一致：
/// 有 run_id 时用 run_id，否则用 conversation_id。
fn file_loaded_key(scope_id: &str, file_id: &str) -> String {
    format!("loaded:{}:{}", scope_id, file_id)
}

/// 生成"加载失败"的内存 key，用于 precompute 路径判断文件是否已失败。
fn file_load_failed_key(scope_id: &str, file_id: &str) -> String {
    format!("load_failed:{}:{}", scope_id, file_id)
}

pub(crate) fn load_authorized_workspace(
    app: &AppHandle,
    conversation_id: &str,
) -> Option<crate::runtime::store::AuthorizedWorkspaceRef> {
    app.try_state::<std::sync::Arc<RuntimeRepositoryFacade>>()
        .and_then(|facade| {
            facade
                .authorized_workspace_store()
                .get_current_for_session(&SessionId::new(conversation_id.to_string()))
                .ok()
                .flatten()
        })
        .map(|aw| crate::runtime::store::AuthorizedWorkspaceRef {
            id: aw.id,
            root_path: aw.root_path,
            display_name: aw.display_name,
        })
}

fn build_workspace_context(
    authorized_workspace: Option<&crate::runtime::store::AuthorizedWorkspaceRef>,
) -> String {
    let Some(authorized_workspace) = authorized_workspace else {
        return String::new();
    };

    format!(
        "\n\n[已连接本地目录]\n- 名称: {}\n- 根目录: {}\n- 当前会话可以直接读取这个目录，不需要先上传或复制文件。\n- 处理本地目录时，优先使用 list_directory / read_workspace_file / search_files / get_file_info。\n- 只有处理用户上传的附件时，才使用 load_file(file_id)。\n- 如果需要进一步计算或生成产物，再结合 execute_python。",
        authorized_workspace.display_name,
        authorized_workspace.root_path.display()
    )
}

pub(crate) fn build_llm_content(
    content: &str,
    file_attachments: &[serde_json::Value],
    has_authorized_workspace: bool,
) -> String {
    if file_attachments.is_empty() {
        return content.to_string();
    }

    let file_refs: Vec<String> = file_attachments
        .iter()
        .map(|f| {
            let name = f["originalName"].as_str().unwrap_or("unknown");
            let ftype = f["fileType"].as_str().unwrap_or("unknown");
            let fid = f["id"].as_str().unwrap_or("");
            format!("- {} (file_id: \"{}\", 类型: {})", name, fid, ftype)
        })
        .collect();

    let hint = if has_authorized_workspace {
        "提示：对这些已上传文件请调用 load_file(file_id) 加载数据；对已连接本地目录请优先使用 list_directory / read_workspace_file / search_files / get_file_info，需要计算或生成文件时再结合 execute_python。"
    } else {
        "提示：对这些已上传文件请先调用 load_file(file_id) 加载文件数据，然后在 execute_python 中直接使用 _df（表格数据）或 _text（文本）变量。"
    };

    format!(
        "{}\n\n[已上传文件]\n{}\n\n{}",
        content,
        file_refs.join("\n"),
        hint
    )
}

pub(crate) async fn legacy_send_message_impl(
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    crypto: Option<Arc<SecureStorage>>,
    tool_registry: Arc<ToolRegistry>,
    skill_registry: Arc<SkillRegistry>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    auth_manager: Arc<crate::auth::AuthManager>,
    app: AppHandle,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
    run_id: RunId,
    session_event_bus: Option<RuntimeEventBus>,
) -> Result<(), String> {
    log::info!(
        "=== send_message START === conversation_id={}, content_len={}, file_ids={:?}",
        conversation_id,
        content.len(),
        file_ids
    );

    // Guard: reject if THIS conversation is already busy
    if gateway.is_conversation_busy(&conversation_id) {
        log::warn!(
            "send_message rejected: conversation {} is already processing",
            conversation_id
        );
        return Err("This conversation is already processing.".to_string());
    }

    // Guard: reject if max concurrent agents reached
    let busy_count = gateway.get_busy_conversations().len();
    if busy_count >= crate::llm::gateway::MAX_CONCURRENT_AGENTS {
        log::warn!(
            "send_message rejected: max concurrent agents reached ({}/{})",
            busy_count,
            crate::llm::gateway::MAX_CONCURRENT_AGENTS
        );
        return Err(format!(
            "Maximum concurrent conversations reached ({}). Please wait.",
            crate::llm::gateway::MAX_CONCURRENT_AGENTS
        ));
    }

    // Claim the conversation slot immediately to prevent TOCTOU race
    // (double-click sending duplicate messages between busy check and set_busy).
    // set_busy is atomic — second caller gets an error even if both pass the check above.
    gateway
        .set_busy_for_run(&conversation_id, run_id.clone())
        .map_err(|e| e)?;

    // 1. Look up attached file metadata (single batch query)
    let file_attachments = if file_ids.is_empty() {
        Vec::new()
    } else {
        match db.get_uploaded_files_by_ids(&file_ids) {
            Ok(f) => f,
            Err(e) => {
                gateway.clear_task(&conversation_id);
                return Err(e.to_string());
            }
        }
    };

    // 2. Save user message to DB (including file references and sender info)
    let msg_id = uuid::Uuid::new_v4().to_string();

    // Get sender information from auth state
    let auth_info = auth_manager.get_auth_info().await;
    let sender_info = if auth_info.logged_in {
        // Logged in: use user's name
        if let Some(user) = auth_info.user {
            serde_json::json!({
                "name": user.name,
                "isLoggedIn": true
            })
        } else {
            // Fallback if user is None
            serde_json::json!({
                "name": "我",
                "isLoggedIn": false
            })
        }
    } else {
        // Not logged in: use default "我"
        serde_json::json!({
            "name": "我",
            "isLoggedIn": false
        })
    };

    let content_json = if file_attachments.is_empty() {
        serde_json::json!({
            "text": content,
            "sender": sender_info
        })
        .to_string()
    } else {
        let files_meta: Vec<serde_json::Value> = file_attachments
            .iter()
            .map(|f| {
                serde_json::json!({
                    "id": f["id"],
                    "fileName": f["originalName"],
                    "fileSize": f["fileSize"],
                    "fileType": f["fileType"],
                    "status": "uploaded",
                })
            })
            .collect();
        serde_json::json!({
            "text": content,
            "files": files_meta,
            "sender": sender_info
        })
        .to_string()
    };

    if let Err(e) = db.insert_message(&msg_id, &conversation_id, "user", &content_json) {
        gateway.clear_task(&conversation_id);
        return Err(e.to_string());
    }

    // NOTE: We do NOT emit "message:updated" for the user message here.
    // The frontend already adds an optimistic user message to the store
    // before calling this IPC command. Emitting here would cause duplicates.

    let authorized_workspace = load_authorized_workspace(&app, &conversation_id);
    let authorized_workspace_present = authorized_workspace.is_some();

    // 3. Build LLM content with file references
    let llm_content = build_llm_content(&content, &file_attachments, authorized_workspace_present);

    // 4. Load settings
    let settings_map = match db.get_all_settings() {
        Ok(m) => m,
        Err(e) => {
            gateway.clear_task(&conversation_id);
            return Err(e.to_string());
        }
    };
    let mut settings: AppSettings = if settings_map.is_empty() {
        log::info!("No settings in DB, using defaults");
        AppSettings::default()
    } else {
        log::info!(
            "Settings map has {} keys: {:?}",
            settings_map.len(),
            settings_map.keys().collect::<Vec<_>>()
        );
        AppSettings::from_string_map(&settings_map)
    };

    log::info!(
        "Settings loaded: primary_model={}, masking={}, auto_routing={}",
        settings.primary_model,
        settings.data_masking_level,
        settings.auto_model_routing
    );

    // Log key metadata only — NEVER log key characters (security)
    let raw_pk_len = settings.primary_api_key.len();
    let raw_pk_has_colon = settings.primary_api_key.contains(':');
    log::info!(
        "Raw primary_api_key: len={}, contains_colon={}",
        raw_pk_len,
        raw_pk_has_colon
    );

    // Decrypt API keys if SecureStorage is available
    if let Some(ss) = crypto.as_ref() {
        log::info!("SecureStorage available, decrypting API keys...");
        settings.primary_api_key = decrypt_key(ss, &settings.primary_api_key);
        settings.tavily_api_key = decrypt_key(ss, &settings.tavily_api_key);
        settings.bocha_api_key = decrypt_key(ss, &settings.bocha_api_key);
    } else {
        log::warn!("SecureStorage NOT available, using raw key values");
    }

    log::info!(
        "After decryption: primary_api_key len={}",
        settings.primary_api_key.len()
    );

    // Cloud mode: only route through Lotus when use_cloud is explicitly enabled
    if settings.use_cloud {
        match auth_manager.get_session_key().await {
            Ok(session_key) => {
                log::info!("Cloud mode active: routing through Lotus gateway");
                settings.primary_model = "lotus".to_string();
                settings.primary_api_key = session_key;
                // cloud_model is already loaded from settings
                if settings.cloud_model.is_empty() {
                    settings.cloud_model = "deepseek-v3".to_string();
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("未登录") {
                    // use_cloud=true but not logged in — emit error
                    log::warn!("Cloud mode enabled but not logged in");
                    let _ = app.emit("streaming:error", serde_json::json!({
                        "conversationId": conversation_id,
                        "error": "云端模式已启用，但尚未登录。请在设置中登录企业账号，或切换到本地模式。"
                    }));
                    gateway.clear_task(&conversation_id);
                    return Ok(());
                } else {
                    // Was logged in but auth expired
                    log::warn!("Cloud auth expired: {}", msg);
                    let _ = app.emit(
                        "auth:expired",
                        serde_json::json!({
                            "message": msg
                        }),
                    );
                    let _ = app.emit(
                        "streaming:error",
                        serde_json::json!({
                            "conversationId": conversation_id,
                            "error": "云端服务暂时不可用，请切换到本地模式或重新登录后重试。"
                        }),
                    );
                    gateway.clear_task(&conversation_id);
                    return Ok(());
                }
            }
        }
    } else {
        log::debug!("Local mode: use_cloud=false, skipping cloud auth");
    }

    // Fall back to built-in default key ONLY for DeepSeek provider
    // (the built-in key is a DeepSeek key, using it for other providers would cause 401)
    if settings.primary_api_key.is_empty() && settings.primary_model == "deepseek-v3" {
        log::info!("Primary API key empty for DeepSeek, falling back to built-in default key");
        let defaults = AppSettings::default();
        settings.primary_api_key = defaults.primary_api_key.clone();
    }

    log::info!(
        "Final primary_api_key: len={}",
        settings.primary_api_key.len()
    );

    // Check if API key is configured
    if settings.primary_api_key.is_empty() {
        let provider_name = match settings.primary_model.as_str() {
            "deepseek-v3" => "DeepSeek",
            "qwen-plus" => "通义千问",
            "openai" => "OpenAI",
            "claude" => "Claude",
            "volcano" => "火山引擎",
            _ => &settings.primary_model,
        };
        let error_msg = format!("请先在设置中配置 {} 的 API Key", provider_name);
        let _ = app.emit(
            "streaming:error",
            serde_json::json!({
                "conversationId": conversation_id,
                "error": error_msg,
            }),
        );
        gateway.clear_task(&conversation_id);
        return Err(error_msg);
    }

    // 5. Build initial message history from DB (sliding window: last N messages)
    const MAX_HISTORY_MESSAGES: u32 = 30;
    let db_messages = match db.get_recent_messages(&conversation_id, MAX_HISTORY_MESSAGES) {
        Ok(m) => m,
        Err(e) => {
            gateway.clear_task(&conversation_id);
            return Err(e.to_string());
        }
    };
    let mut chat_messages: Vec<ChatMessage> = db_messages
        .iter()
        .filter_map(|m| {
            let role = m.get("role")?.as_str()?;
            let content_val = m.get("content")?;
            let text = content_val.get("text")?.as_str()?.to_string();
            // Compress tool results in history to save context tokens
            let text = compress_tool_result(&text);
            Some(ChatMessage::text(role, text))
        })
        .collect();

    // Replace the last user message content with llm_content that includes file references
    if !file_attachments.is_empty() {
        if let Some(last) = chat_messages.last_mut() {
            if last.role == "user" {
                last.content = llm_content;
            }
        }
    }

    log::info!(
        "Chat messages built: count={}, roles={:?}",
        chat_messages.len(),
        chat_messages
            .iter()
            .map(|m| m.role.as_str())
            .collect::<Vec<_>>()
    );

    // 6. Determine masking level
    // Always use Strict masking — PII protection is non-negotiable.
    // The setting is kept for forward compatibility but defaults to strict.
    let masking_level = MaskingLevel::Strict;

    // 7. Derive workspace path for tool execution
    let workspace_path = db
        .get_setting("workspacePath")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| file_mgr.workspace_path().to_path_buf());

    // 8. Determine conversation mode and use the Skill system for routing
    //
    // The conversation.mode column tracks workflow state:
    //   "daily"      → normal chat (may detect Skill activation)
    //   "confirming" → workflow step 0 done, waiting for user to confirm direction
    //   "analyzing"  → workflow steps 1–5 in progress
    //
    // The SkillRegistry replaces the hardcoded orchestrator:
    //   - detect_activation() replaces detect_analysis_mode()
    //   - Skill.on_step_complete() replaces next_action()
    //   - Skill methods provide prompt, tool filter, iterations, budget
    // Check for files: current message attachments OR any files uploaded earlier in this conversation.
    // This ensures skill activation detects files even when the user sends a follow-up text message
    // (e.g., "帮我分析薪酬") without re-attaching the file.
    let has_files = if !file_attachments.is_empty() {
        true
    } else {
        db.get_uploaded_files_for_conversation(&conversation_id)
            .map(|files| !files.is_empty())
            .unwrap_or(false)
    };
    let conversation_mode = db
        .get_conversation_mode(&conversation_id)
        .unwrap_or_else(|_| "daily".to_string());

    log::info!(
        "Conversation {} mode='{}', has_files={}",
        conversation_id,
        conversation_mode,
        has_files
    );

    let step_config: Option<StepConfig> = match conversation_mode.as_str() {
        "daily" => {
            // Check if a Skill with a workflow should activate.
            // Skip detection during cooldown period after recent analysis.
            let in_cooldown = {
                let cooldown_key = format!("note:{}:analysis_completed_at", conversation_id);
                db.get_memory(&cooldown_key)
                    .ok()
                    .flatten()
                    .map_or(false, |ts| {
                        chrono::DateTime::parse_from_rfc3339(&ts).map_or(false, |completed_at| {
                            let elapsed = chrono::Utc::now().signed_duration_since(completed_at);
                            elapsed.num_seconds() < 60 // 60-second cooldown
                        })
                    })
            };

            let detected = if in_cooldown {
                log::info!(
                    "Skipping skill detection for conversation {} (analysis cooldown active)",
                    conversation_id
                );
                None
            } else {
                skill_registry
                    .detect_activation(&content, has_files, skill_registry.default_skill_id())
                    .await
            };

            if let Some(skill_id) = detected {
                let skill = skill_registry.get(&skill_id).await;
                match skill {
                    Some(skill) if skill.workflow().is_some() => {
                        // Skill with workflow detected → activate directly
                        // Files may or may not be present; the workflow prompt will guide
                        // the user to upload if needed.
                        log::info!(
                            "Skill '{}' with workflow detected for conversation {}, activating directly",
                            skill_id, conversation_id
                        );

                        let workflow = skill.workflow().unwrap();
                        let initial_step = &workflow.initial_step;
                        let step_num = initial_step
                            .strip_prefix("step")
                            .and_then(|n| n.parse::<u32>().ok())
                            .unwrap_or(0);

                        if let Err(e) = db.set_conversation_mode(&conversation_id, "confirming") {
                            log::error!("Failed to set mode to 'confirming': {}", e);
                        }
                        let skill_key = format!("note:{}:active_skill", conversation_id);
                        if let Err(e) = db.set_memory(&skill_key, &skill_id, Some("system")) {
                            log::error!("Failed to store active skill ID: {}", e);
                        }
                        if let Err(e) = orchestrator::advance_step(
                            &db.clone(),
                            &conversation_id,
                            step_num,
                            "in_progress",
                        ) {
                            log::error!("Failed to mark step {} as in_progress: {}", step_num, e);
                        }

                        let mut state = SkillState::new(&skill_id);
                        state.current_step = Some(initial_step.clone());
                        state.has_files = has_files;
                        Some(build_config_from_skill(&*skill, &state, &tool_registry).await)
                    }
                    _ => {
                        // No workflow → stay in daily mode
                        log::debug!("Skill detected but no workflow, staying in daily mode");
                        None
                    }
                }
            } else {
                None // daily chat, no analysis
            }
        }

        "confirming" | "analyzing" => {
            // --- Pre-routing: check for exit/daily conditions ---
            //
            // Confirming mode (auto-activated Step 0): be lenient about exit.
            // The user didn't explicitly choose to enter analysis, so any
            // message that isn't engaging with analysis should exit silently.
            //
            // Analyzing mode (Steps 1-5): the user actively confirmed, so
            // only explicit abort or daily-question routing applies.
            let should_exit_analysis = if conversation_mode == "confirming" {
                if crate::plugin::skill_trait::is_abort_keyword(&content) {
                    log::info!(
                        "User explicitly aborted analysis (confirming) for conversation {}",
                        conversation_id
                    );
                    true
                } else if is_daily_question(&content) {
                    // Only exit if the message is clearly a daily question
                    // (e.g., "社保怎么算", "请问年假多少天") — not analysis-related
                    log::info!(
                        "User sent daily question during confirming mode, \
                         exiting analysis for conversation {}",
                        conversation_id
                    );
                    true
                } else {
                    // Default: stay in analysis. Any short reply (确认/继续/开始分析)
                    // or analysis-related message keeps the workflow active.
                    false
                }
            } else {
                // analyzing mode: only explicit abort exits
                // (abort keywords are handled inside on_step_complete below)
                false
            };

            let route_daily_chat = if !should_exit_analysis && conversation_mode == "analyzing" {
                is_daily_question(&content)
            } else {
                false
            };

            if should_exit_analysis {
                // Exit analysis → back to daily mode
                if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                    log::error!("Failed to set mode to 'daily': {}", e);
                }
                if let Err(e) = db.finalize_analysis(&conversation_id, "aborted") {
                    log::error!("Failed to finalize analysis: {}", e);
                }
                clear_analysis_notes(&db, &conversation_id, &workspace_path);
                None // process user's message as daily chat
            } else if route_daily_chat {
                log::info!(
                    "Detected daily question during analyzing mode for conversation {}, routing to daily chat",
                    conversation_id
                );
                None // step_config = None → daily mode agent_loop, keep analysis paused
            } else {
                // --- Active workflow routing ---
                let skill_key = format!("note:{}:active_skill", conversation_id);
                let active_skill_id = match db.get_memory(&skill_key).ok().flatten() {
                    Some(id) => id,
                    None => {
                        log::warn!(
                        "No active_skill stored for conversation {} in mode '{}', resetting to daily",
                        conversation_id, conversation_mode
                    );
                        if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                            log::error!("Failed to reset mode to 'daily': {}", e);
                        }
                        String::new()
                    }
                };

                if active_skill_id.is_empty() {
                    None
                } else {
                    match skill_registry.get(&active_skill_id).await {
                        None => {
                            log::error!(
                        "Active skill '{}' not found for conversation {}, falling back to daily mode",
                        active_skill_id, conversation_id
                    );
                            if let Err(e) = db.set_conversation_mode(&conversation_id, "daily") {
                                log::error!("Failed to reset mode to 'daily': {}", e);
                            }
                            None
                        }
                        Some(skill) => {
                            let db_step_state = orchestrator::get_step_state(&db, &conversation_id);
                            let step_num = db_step_state.as_ref().map(|s| s.step).unwrap_or(0);

                            let mut skill_state = SkillState::new(skill.id());
                            skill_state.current_step = Some(format!("step{}", step_num));
                            skill_state.has_files = has_files;

                            // Route based on step status
                            match db_step_state.as_ref().map(|s| &s.status) {
                                Some(&StepStatus::Paused) => {
                                    // Crash recovery: resume paused step
                                    log::info!(
                                        "Resuming paused step {} for conversation {}",
                                        step_num,
                                        conversation_id
                                    );

                                    if let Err(e) = orchestrator::advance_step(
                                        &db.clone(),
                                        &conversation_id,
                                        step_num,
                                        "in_progress",
                                    ) {
                                        log::error!(
                                            "Failed to mark step {} as in_progress: {}",
                                            step_num,
                                            e
                                        );
                                    }

                                    Some(
                                        build_config_from_skill(
                                            &*skill,
                                            &skill_state,
                                            &tool_registry,
                                        )
                                        .await,
                                    )
                                }

                                Some(&StepStatus::InProgress) => {
                                    // Edge case: user sent message while step is still running.
                                    let action = skill.on_step_complete(&mut skill_state, &content);

                                    if matches!(action, StepAction::Abort) {
                                        log::info!("User aborted analysis (in_progress) for conversation {}",
                            conversation_id);
                                        if let Err(e) =
                                            db.set_conversation_mode(&conversation_id, "daily")
                                        {
                                            log::error!("Failed to set mode to 'daily': {}", e);
                                        }
                                        if let Err(e) =
                                            db.finalize_analysis(&conversation_id, "aborted")
                                        {
                                            log::error!(
                                                "Failed to finalize analysis as aborted: {}",
                                                e
                                            );
                                        }
                                        clear_analysis_notes(
                                            &db,
                                            &conversation_id,
                                            &workspace_path,
                                        );
                                        None
                                    } else {
                                        log::warn!(
                            "User sent message while step {} is in_progress; re-running",
                            step_num
                        );
                                        if let Err(e) = orchestrator::advance_step(
                                            &db.clone(),
                                            &conversation_id,
                                            step_num,
                                            "in_progress",
                                        ) {
                                            log::error!(
                                                "Failed to mark step {} as in_progress: {}",
                                                step_num,
                                                e
                                            );
                                        }
                                        Some(
                                            build_config_from_skill(
                                                &*skill,
                                                &skill_state,
                                                &tool_registry,
                                            )
                                            .await,
                                        )
                                    }
                                }

                                Some(&StepStatus::Completed) | None => {
                                    let action = skill.on_step_complete(&mut skill_state, &content);
                                    log::info!(
                        "[step_routing] conv={} step={} current_step={:?} user_content='{}' => action={:?}",
                        conversation_id, step_num,
                        skill_state.current_step,
                        truncate_for_ui(&content, 50),
                        std::mem::discriminant(&action)
                    );

                                    match action {
                                        StepAction::Abort => {
                                            log::info!(
                                                "User aborted analysis for conversation {}",
                                                conversation_id
                                            );
                                            if let Err(e) =
                                                db.set_conversation_mode(&conversation_id, "daily")
                                            {
                                                log::error!("Failed to set mode to 'daily': {}", e);
                                            }
                                            if let Err(e) =
                                                db.finalize_analysis(&conversation_id, "aborted")
                                            {
                                                log::error!(
                                                    "Failed to finalize analysis as aborted: {}",
                                                    e
                                                );
                                            }
                                            clear_analysis_notes(
                                                &db,
                                                &conversation_id,
                                                &workspace_path,
                                            );
                                            None
                                        }

                                        StepAction::Finish => {
                                            log::info!(
                                                "Analysis complete for conversation {}",
                                                conversation_id
                                            );
                                            if let Err(e) =
                                                db.set_conversation_mode(&conversation_id, "daily")
                                            {
                                                log::error!("Failed to set mode to 'daily': {}", e);
                                            }
                                            if let Err(e) =
                                                db.finalize_analysis(&conversation_id, "completed")
                                            {
                                                log::error!("Failed to finalize analysis: {}", e);
                                            }
                                            clear_analysis_notes(
                                                &db,
                                                &conversation_id,
                                                &workspace_path,
                                            );
                                            None
                                        }

                                        StepAction::AdvanceToStep(next_step_id) => {
                                            let next_step_num = next_step_id
                                                .strip_prefix("step")
                                                .and_then(|n| n.parse::<u32>().ok());

                                            if next_step_num.is_none() {
                                                log::warn!(
                                    "Invalid step ID '{}' from skill '{}', expected 'stepN' format; falling back to step {}",
                                    next_step_id, active_skill_id, step_num + 1
                                );
                                            }
                                            let next_step_num =
                                                next_step_num.unwrap_or(step_num + 1);

                                            // Check that the current step saved its summary note (step >= 1).
                                            // This is a safety net: if the LLM forgot to call save_analysis_note,
                                            // the checkpoint extraction below will still capture context, but we
                                            // log a warning so this can be monitored.
                                            if step_num >= 1 {
                                                let note_key = format!(
                                                    "note:{}:step{}_summary",
                                                    conversation_id, step_num
                                                );
                                                match db.get_memory(&note_key) {
                                                    Ok(Some(_)) => {
                                                        log::info!("Step {} summary note found for conversation {}", step_num, conversation_id);
                                                    }
                                                    _ => {
                                                        log::warn!(
                                            "Step {} summary note MISSING for conversation {} — LLM did not call save_analysis_note. \
                                             Checkpoint extraction will attempt to compensate.",
                                            step_num, conversation_id
                                        );
                                                    }
                                                }
                                            }

                                            log::info!(
                                "[step_advance] conv={} from_step={} to_step={} next_step_id='{}'",
                                conversation_id, step_num, next_step_num, next_step_id
                            );

                                            // Save direction note for step 0 → step 1 transition
                                            if step_num == 0 {
                                                let direction = content.trim();
                                                if !direction.is_empty() {
                                                    let note_key = format!(
                                                        "note:{}:step0_analysis_direction",
                                                        conversation_id
                                                    );
                                                    match db.set_memory(
                                                        &note_key,
                                                        direction,
                                                        Some("step_0_direction"),
                                                    ) {
                                                        Ok(_) => log::info!(
                                                            "Saved analysis direction: '{}'",
                                                            direction
                                                        ),
                                                        Err(e) => log::warn!(
                                                            "Failed to save analysis direction: {}",
                                                            e
                                                        ),
                                                    }
                                                }
                                            }

                                            // Transition mode: confirming → analyzing (for step 0 → step 1)
                                            if conversation_mode == "confirming" {
                                                if let Err(e) = db.set_conversation_mode(
                                                    &conversation_id,
                                                    "analyzing",
                                                ) {
                                                    log::error!(
                                                        "Failed to set mode to 'analyzing': {}",
                                                        e
                                                    );
                                                }
                                            }

                                            // Mark current step completed, new step in_progress
                                            if let Err(e) = orchestrator::advance_step(
                                                &db.clone(),
                                                &conversation_id,
                                                step_num,
                                                "completed",
                                            ) {
                                                log::error!(
                                                    "Failed to mark step {} as completed: {}",
                                                    step_num,
                                                    e
                                                );
                                            }
                                            if let Err(e) = orchestrator::advance_step(
                                                &db.clone(),
                                                &conversation_id,
                                                next_step_num,
                                                "in_progress",
                                            ) {
                                                log::error!(
                                                    "Failed to mark step {} as in_progress: {}",
                                                    next_step_num,
                                                    e
                                                );
                                            }

                                            // Emit step-reset so the frontend:
                                            // 1. Resets the streaming content for the new step
                                            // 2. Updates the watchdog activity timer (prevents 30s timeout
                                            //    during checkpoint extraction which can take up to 30s)
                                            let _ = app.emit(
                                                "streaming:step-reset",
                                                serde_json::json!({
                                                    "conversationId": conversation_id,
                                                    "step": next_step_num,
                                                }),
                                            );

                                            // --- Checkpoint extraction (Layer 1) ---
                                            // Make a dedicated non-streaming LLM call to extract structured
                                            // step conclusions BEFORE wiping message history. Falls back to
                                            // auto_capture on any failure. Runs for ALL steps (including step 0
                                            // which carries critical file info and analysis direction).
                                            {
                                                let (base_ep, step_ep) = skill
                                                    .extract_prompt(&format!("step{}", step_num));
                                                let extract_prompt =
                                                    if base_ep.is_empty() && step_ep.is_empty() {
                                                        String::new()
                                                    } else {
                                                        format!("{}\n\n{}", base_ep, step_ep)
                                                    };
                                                let cp_result =
                                                    crate::llm::checkpoint::checkpoint_extract(
                                                        &gateway,
                                                        &settings,
                                                        &conversation_id,
                                                        step_num,
                                                        &chat_messages,
                                                        &db,
                                                        &extract_prompt,
                                                        &workspace_path,
                                                    )
                                                    .await;
                                                if cp_result.is_some() {
                                                    log::info!("[step_advance] Checkpoint extraction succeeded for step {}", step_num);
                                                } else {
                                                    log::warn!("[step_advance] Checkpoint extraction failed for step {}, auto_capture will serve as fallback", step_num);
                                                }

                                                // --- Auto-capture (Layer 3) --- always runs as fallback
                                                auto_capture_step_context(
                                                    &db,
                                                    &conversation_id,
                                                    step_num,
                                                    &chat_messages,
                                                );
                                            }

                                            // Archive plan file on step advance (P1)
                                            {
                                                let plan_path = workspace_path
                                                    .join("analysis")
                                                    .join(&conversation_id)
                                                    .join("_plan.md");
                                                if plan_path.exists() {
                                                    let archive = plan_path.with_file_name(
                                                        format!("_plan_step{}.md", step_num),
                                                    );
                                                    match std::fs::rename(&plan_path, &archive) {
                                                        Ok(_) => log::info!(
                                            "[plan_archive] Archived step {} plan to {:?}",
                                            step_num, archive
                                        ),
                                                        Err(e) => log::warn!(
                                            "[plan_archive] Failed to archive step {} plan: {}",
                                            step_num, e
                                        ),
                                                    }
                                                }
                                            }

                                            // Sub-agent context reset
                                            let original_user_msg = chat_messages
                                                .iter()
                                                .find(|m| m.role == "user")
                                                .cloned();
                                            let step_summary = if step_num == 0 {
                                                format!(
                                    "分析方向已确认。关键信息已保存到分析记录中。\n\n\
                                    请查看系统提示词中 [前序分析记录] 部分获取之前的完整数据。\n\
                                    所有数据分析必须使用 execute_python 基于实际数据执行，禁止凭空推断。\n\
                                    如需读取数据文件，请使用系统提示词中[本次会话的文件]部分提供的文件信息。\n\n\
                                    ⚠️ 直接开始执行第 {} 步的分析任务，立即调用工具，不要先做自我介绍或复述步骤说明。",
                                    next_step_num
                                )
                                            } else {
                                                format!(
                                    "前 {} 步分析已完成。关键结论和数据已保存到分析记录中。\n\n\
                                    请查看系统提示词中 [前序分析记录] 部分获取之前步骤的完整数据（包括字段映射、岗位归一化结果、职级框架等）。\n\
                                    所有数据分析必须使用 execute_python 基于实际数据执行，禁止凭空推断。\n\
                                    如需读取数据文件，请使用系统提示词中[本次会话的文件]部分提供的文件信息。\n\n\
                                    ⚠️ 直接开始执行第 {} 步的分析任务，立即调用工具（如 execute_python），不要先做自我介绍、复述步骤说明或寒暄。",
                                    step_num, next_step_num
                                )
                                            };
                                            chat_messages = Vec::new();
                                            if let Some(user_msg) = original_user_msg {
                                                chat_messages.push(user_msg);
                                            }
                                            // Use "user" role so the conversation ends with a user message.
                                            // Some providers (e.g. Aliyun) reject requests ending with assistant messages.
                                            chat_messages
                                                .push(ChatMessage::text("user", &step_summary));

                                            // Build config for the new step
                                            let mut new_state = SkillState::new(skill.id());
                                            new_state.current_step = Some(next_step_id);
                                            new_state.has_files = has_files;

                                            Some(
                                                build_config_from_skill(
                                                    &*skill,
                                                    &new_state,
                                                    &tool_registry,
                                                )
                                                .await,
                                            )
                                        }

                                        StepAction::WaitForUser => {
                                            log::info!("Re-running step {} with user feedback for conversation {}",
                                step_num, conversation_id);

                                            if let Err(e) = orchestrator::advance_step(
                                                &db.clone(),
                                                &conversation_id,
                                                step_num,
                                                "in_progress",
                                            ) {
                                                log::error!(
                                                    "Failed to mark step {} as in_progress: {}",
                                                    step_num,
                                                    e
                                                );
                                            }

                                            let mut config = build_config_from_skill(
                                                &*skill,
                                                &skill_state,
                                                &tool_registry,
                                            )
                                            .await;
                                            config.is_feedback = true;
                                            Some(config)
                                        }
                                    }
                                }
                            }
                        } // Some(skill)
                    } // match skill_registry.get
                } // else (active_skill_id not empty)
            } // else (workflow routing)
        }

        // Unknown mode — treat as daily
        _ => {
            log::warn!(
                "Unknown conversation mode '{}', treating as daily",
                conversation_mode
            );
            None
        }
    };

    // Build AgentContext for the background task
    let assistant_id = uuid::Uuid::new_v4().to_string();
    // Session-level root token: this is the top of the cancel hierarchy for this
    // legacy send_message path (session → turn → tool_call).  No parent exists above
    // the transport entry point, so CancellationToken::new() is intentional here.
    let cancel_token = CancellationToken::new();
    let agent_ctx = AgentContext {
        db: db.clone(),
        gateway: gateway.clone(),
        file_mgr: file_mgr.clone(),
        tool_registry: tool_registry.clone(),
        session_mgr: session_mgr.clone(),
        auth_manager: auth_manager.clone(),
        app: app.clone(),
        settings,
        workspace_path,
        conversation_id: conversation_id.clone(),
        run_id: run_id.clone(),
        authorized_workspace,
        assistant_id: assistant_id.clone(),
        masking_level,
        session_event_bus,
        cancel_token,
    };

    // 9. Spawn the agent loop in a background task with guard and timeout
    log::info!(
        "=== Spawning agent_loop === assistant_id={}, analysis_step={:?}",
        assistant_id,
        step_config.as_ref().map(|c| c.step)
    );

    // Gateway already marked busy (early claim at step 0 to prevent TOCTOU race).
    // Record in DB for crash recovery. If this fails, rollback the gateway busy state.
    if let Err(e) = db.insert_active_task(&conversation_id) {
        log::error!(
            "Failed to insert active task, rolling back gateway busy state: {}",
            e
        );
        gateway.clear_task(&conversation_id);
        return Err(e.to_string());
    }

    // Cognitive memory: trigger auto-distill if > 24h since last distill.
    // Runs synchronously but is fast (~ms for typical index sizes).
    if db.needs_cognitive_distill() {
        log::info!("[cognitive] Auto-distilling memories (>24h since last distill)");
        match db.distill_cognitive_memories(7, false) {
            Ok(report) => log::info!(
                "[cognitive] Auto-distill complete: promoted={} demoted={} archived={} core_lines={}",
                report.promoted, report.demoted, report.archived, report.core_lines,
            ),
            Err(e) => log::warn!("[cognitive] Auto-distill failed (non-fatal): {}", e),
        }
    }

    // Agent loop spawn: use the real upstream cancel source from RuntimeRunRegistry
    // (via LlmGateway) instead of a local token with no caller.
    tokio::spawn(async move {
        let conversation_id_clone = agent_ctx.conversation_id.clone();
        let app_clone = agent_ctx.app.clone();
        let cancel_token = agent_ctx.cancel_token.clone();

        // Early exit if cancelled before the loop starts
        if agent_ctx
            .gateway
            .is_conversation_busy(&conversation_id_clone)
        {
            log::info!(
                "[AgentGuard] agent_loop skipped (cancelled before start) for conversation {}",
                conversation_id_clone
            );
            cancel_token.cancel();
            return;
        }

        let mut guard = AgentGuard::new(
            agent_ctx.gateway.clone(),
            agent_ctx.db.clone(),
            agent_ctx.session_mgr.clone(),
            agent_ctx.app.clone(),
            agent_ctx.conversation_id.clone(),
            agent_ctx.run_id.clone(),
        );

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(AGENT_TIMEOUT_SECS),
            agent_loop(agent_ctx, chat_messages, step_config),
        )
        .await;

        match result {
            Ok(()) => {
                log::info!(
                    "[AgentGuard] agent_loop completed normally for conversation {}",
                    conversation_id_clone
                );
            }
            Err(_elapsed) => {
                log::error!(
                    "[AgentGuard] agent_loop TIMED OUT after {}s for conversation {}",
                    AGENT_TIMEOUT_SECS,
                    conversation_id_clone
                );
                cancel_token.cancel();
                guard
                    .gateway
                    .cancel_conversation(&conversation_id_clone)
                    .ok();
                let _ = app_clone.emit(
                    "streaming:error",
                    serde_json::json!({
                        "conversationId": conversation_id_clone,
                        "error": format!(
                            "处理超时（已运行 {} 分钟）。可能原因：\n\
                            1. 任务过于复杂\n\
                            2. 网络连接不稳定\n\n\
                            请尝试简化问题后重试。",
                            AGENT_TIMEOUT_SECS / 60
                        ),
                        "errorType": "agent_timeout",
                        "timeoutSeconds": AGENT_TIMEOUT_SECS,
                    }),
                );
            }
        }

        guard.clear().await;
    });

    Ok(())
}

/// Compress older messages into a summary to stay within context limits.
///
/// When total message content exceeds [`COMPRESS_THRESHOLD_CHARS`], the function:
/// 1. Splits messages into "old" (to compress) and "recent" (to keep verbatim).
/// 2. Makes a non-streaming LLM call to summarize the old messages.
/// 3. Replaces old messages with a single summary message.
///
/// Returns the (possibly compressed) message list. On compression failure,
/// returns the original messages unchanged (graceful degradation).
async fn compress_context_if_needed(
    messages: Vec<ChatMessage>,
    gateway: &LlmGateway,
    settings: &AppSettings,
) -> Vec<ChatMessage> {
    // Estimate total content size
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();

    if total_chars <= COMPRESS_THRESHOLD_CHARS || messages.len() <= COMPRESS_KEEP_RECENT + 2 {
        return messages; // Under threshold or too few messages to compress
    }

    let split_point = messages.len().saturating_sub(COMPRESS_KEEP_RECENT);

    log::info!(
        "[compress] Total content {}chars exceeds threshold {}chars, compressing {} of {} messages",
        total_chars,
        COMPRESS_THRESHOLD_CHARS,
        split_point,
        messages.len()
    );

    // Build a text representation of old messages for summarization
    let old_messages = &messages[..split_point];
    let mut conversation_text = String::new();
    for msg in old_messages {
        let role_label = match msg.role.as_str() {
            "user" => "用户",
            "assistant" => "助手",
            _ => &msg.role,
        };
        // Skip empty messages
        if msg.content.trim().is_empty() {
            continue;
        }
        // Truncate very long individual messages for the summary input
        let content = if msg.content.len() > 2000 {
            let end = truncate_at_char_boundary(&msg.content, 2000);
            format!("{}...", &msg.content[..end])
        } else {
            msg.content.clone()
        };
        conversation_text.push_str(&format!("{}：{}\n\n", role_label, content));
    }

    // Cap total input to the compression LLM call
    if conversation_text.len() > 12000 {
        let end = truncate_at_char_boundary(&conversation_text, 12000);
        conversation_text.truncate(end);
    }

    let compress_prompt = format!(
        "请将以下对话历史压缩为一段简洁的摘要（不超过 500 字）。\n\
        保留：关键话题、用户的核心需求、重要结论和数据、各文件的分析发现（列名、数据特征、关键结果）。\n\
        省略：寒暄、重复内容、工具调用细节。\n\
        用中文输出，格式为连贯的段落（不要用列表）。\n\n\
        ---\n{}\n---",
        conversation_text
    );

    let compress_messages = vec![ChatMessage::text("user", &compress_prompt)];

    // Non-streaming call with short timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        gateway.send_message(
            settings,
            compress_messages,
            MaskingLevel::Relaxed, // No masking needed for internal summarization
            Some("你是一个对话摘要助手。将对话历史压缩为简洁摘要。"),
            None, // no dynamic context
            None, // no tools
        ),
    )
    .await;

    match result {
        Ok(Ok(response)) if !response.content.trim().is_empty() => {
            let summary = response.content.trim().to_string();
            log::info!(
                "[compress] Summary generated: {} chars (compressed from {} chars, {} messages)",
                summary.len(),
                total_chars,
                split_point
            );

            // Rebuild messages: [summary] + [recent messages]
            let mut compressed = Vec::with_capacity(1 + messages.len() - split_point);
            compressed.push(ChatMessage::text(
                "assistant",
                &format!("[之前的对话摘要]\n{}", summary),
            ));
            compressed.extend_from_slice(&messages[split_point..]);
            compressed
        }
        Ok(Ok(_)) => {
            log::warn!("[compress] LLM returned empty summary, keeping original messages");
            messages
        }
        Ok(Err(e)) => {
            log::warn!(
                "[compress] Summarization LLM call failed: {}, keeping original messages",
                e
            );
            messages
        }
        Err(_) => {
            log::warn!("[compress] Summarization timed out (20s), keeping original messages");
            messages
        }
    }
}

// ---------------------------------------------------------------------------
// AgentContext — groups shared services passed through the agent loop
// ---------------------------------------------------------------------------

/// Shared services and configuration for a single agent loop invocation.
///
/// Replaces the 14-parameter `agent_loop` signature with a single struct,
/// making it easier to add new fields without touching every call site.
struct AgentContext {
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    tool_registry: Arc<ToolRegistry>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    auth_manager: Arc<crate::auth::AuthManager>,
    app: AppHandle,
    settings: AppSettings,
    workspace_path: std::path::PathBuf,
    conversation_id: String,
    run_id: RunId,
    authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
    assistant_id: String,
    masking_level: MaskingLevel,
    /// Session-level event bus from RuntimeChatTurnDriver.  When present, tool
    /// round events (ToolCallExecuting / ToolCallCompleted) are emitted through
    /// this bus instead of a locally-constructed one, so TauriEventAdapter and
    /// any test recording host on the session bus receive them.
    session_event_bus: Option<RuntimeEventBus>,
    /// Cancellation token for this turn.  Cancelled when the user triggers
    /// stop_streaming or the agent loop times out.  Passed to
    /// `PythonSessionManager` so LRU-evicted sessions skip checkpoint writes
    /// when the owning turn has already been cancelled.
    cancel_token: CancellationToken,
}

// ---------------------------------------------------------------------------
// Extracted helper functions for agent_loop setup
// ---------------------------------------------------------------------------

/// Build file context string for injection into the system prompt.
///
/// Lists uploaded files (with load status and variable mapping) and
/// generated files so the LLM always knows what's available.
fn build_file_context(db: &AppStorage, conversation_id: &str, loaded_scope_id: &str) -> String {
    let uploaded = db
        .get_uploaded_files_for_conversation(conversation_id)
        .unwrap_or_default();
    let generated = db
        .get_generated_files_for_conversation(conversation_id)
        .unwrap_or_default();

    if uploaded.is_empty() && generated.is_empty() {
        return String::new();
    }

    let mut ctx = String::new();

    if !uploaded.is_empty() {
        let loaded_prefix = format!("loaded:{}:", loaded_scope_id);
        let loaded_entries = db
            .get_memories_by_prefix(&loaded_prefix)
            .unwrap_or_default();
        let mut loaded_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (_key, value) in &loaded_entries {
            if let Ok(info) = serde_json::from_str::<serde_json::Value>(value) {
                let file_id = info.get("fileId").and_then(|v| v.as_str()).unwrap_or("");
                let loaded_as = info
                    .get("loadedAs")
                    .and_then(|v| v.as_str())
                    .unwrap_or("text");
                if !file_id.is_empty() {
                    loaded_map.insert(file_id.to_string(), loaded_as.to_string());
                }
            }
        }

        let df_count = loaded_map.values().filter(|v| *v == "dataframe").count();
        let text_count = loaded_map.values().filter(|v| *v == "text").count();

        ctx.push_str("\n\n[本次会话的上传文件]");
        for f in &uploaded {
            let name = f["originalName"].as_str().unwrap_or("unknown");
            let fid = f["id"].as_str().unwrap_or("");
            let ftype = f["fileType"].as_str().unwrap_or("unknown");

            if let Some(loaded_as) = loaded_map.get(fid) {
                let var_hint = match loaded_as.as_str() {
                    "dataframe" if df_count > 1 => format!("_dfs['{}']", fid),
                    "dataframe" => "_df".to_string(),
                    "text" if text_count > 1 => format!("_texts['{}']", fid),
                    _ if text_count > 1 => format!("_texts['{}']", fid),
                    _ => "_text".to_string(),
                };
                ctx.push_str(&format!(
                    "\n- {} (file_id: \"{}\", 类型: {}) ✅已加载 → {}",
                    name, fid, ftype, var_hint
                ));
            } else {
                ctx.push_str(&format!(
                    "\n- {} (file_id: \"{}\", 类型: {}) ⏳未加载；若要分析这个上传文件，必须先调用 load_file(file_id=\"{}\")",
                    name, fid, ftype, fid
                ));
            }
        }
    }

    if !generated.is_empty() {
        ctx.push_str("\n\n[本次会话生成的文件]");
        for f in &generated {
            let name = f["fileName"].as_str().unwrap_or("unknown");
            let fid = f["id"].as_str().unwrap_or("");
            let category = f["category"].as_str().unwrap_or("file");
            ctx.push_str(&format!(
                "\n- {} (file_id: \"{}\", 类别: {})",
                name, fid, category
            ));
        }
    }

    ctx
}

/// Build analysis notes context for injection into the system prompt.
///
/// Three-layer priority: checkpoint (structured) > summary (LLM-curated) > auto_context (fallback).
/// Checkpoints use field-level decay: summary/key_findings/next_step_input never truncated,
/// data_artifacts truncated for older steps.
fn build_analysis_notes_context(
    db: &AppStorage,
    conversation_id: &str,
    step_config: Option<&StepConfig>,
    workspace: &std::path::Path,
    model: &str,
) -> String {
    let notes_prefix = format!("note:{}:", conversation_id);
    let notes = match db.get_memories_by_prefix(&notes_prefix) {
        Ok(notes) if !notes.is_empty() => notes,
        _ => return String::new(),
    };

    let current_step = step_config.map(|c| c.step).unwrap_or(0);

    // Separate notes by type: checkpoint, step-grouped, non-step
    let mut checkpoints: std::collections::BTreeMap<u32, crate::llm::checkpoint::StepCheckpoint> =
        std::collections::BTreeMap::new();
    let mut step_notes: std::collections::BTreeMap<u32, Vec<(String, String)>> =
        std::collections::BTreeMap::new();
    let mut non_step_notes: Vec<(String, String)> = Vec::new();

    for (key, value) in &notes {
        let note_name = key.strip_prefix(&notes_prefix).unwrap_or(key);

        if note_name.starts_with("step") && note_name.ends_with("_checkpoint") {
            if let Some(step_str) = note_name.strip_prefix("step") {
                if let Some(num_str) = step_str.strip_suffix("_checkpoint") {
                    if let Ok(sn) = num_str.parse::<u32>() {
                        if let Ok(cp) =
                            serde_json::from_str::<crate::llm::checkpoint::StepCheckpoint>(value)
                        {
                            checkpoints.insert(sn, cp);
                            continue;
                        }
                    }
                }
            }
        }

        if note_name.starts_with("step") {
            if let Some(step_str) = note_name.strip_prefix("step") {
                if let Some(sn) = step_str
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u32>()
                    .ok()
                {
                    step_notes
                        .entry(sn)
                        .or_default()
                        .push((note_name.to_string(), value.clone()));
                    continue;
                }
            }
        }
        non_step_notes.push((note_name.to_string(), value.clone()));
    }

    let mut ctx = String::from("\n\n[前序分析记录]\n");
    ctx.push_str("⚠️ 重要：以下是之前步骤保存的分析结论和关键数据，是当前步骤的唯一数据来源。\n");
    ctx.push_str("· 当前步骤必须基于这些记录继续分析\n");
    ctx.push_str("· 所有数据必须来自 execute_python 实际执行，禁止凭空推断\n");
    ctx.push_str(
        "· 当前步骤结束前必须调用 save_analysis_note 保存关键结论，否则下一步将丢失数据\n\n",
    );

    for (name, value) in &non_step_notes {
        ctx.push_str(&format!("### {}\n{}\n\n", name, value));
    }

    let all_steps: std::collections::BTreeSet<u32> = checkpoints
        .keys()
        .chain(step_notes.keys())
        .copied()
        .collect();

    const OLDER_STEP_MAX_CHARS: usize = 3000;
    const RECENT_STEP_MAX_CHARS: usize = 6000;

    for &sn in &all_steps {
        let is_recent = current_step == 0 || sn >= current_step.saturating_sub(1);

        if let Some(cp) = checkpoints.get(&sn) {
            let display_name = step_config
                .and_then(|c| c.step_display_names.iter().find(|(n, _)| *n == sn))
                .map(|(_, name)| name.as_str())
                .unwrap_or("未知步骤");
            ctx.push_str(&crate::llm::checkpoint::format_checkpoint_for_injection(
                cp,
                sn,
                display_name,
                is_recent,
            ));
        } else if let Some(notes_for_step) = step_notes.get(&sn) {
            ctx.push_str(&format!("## 第 {} 步记录\n", sn));

            for (name, value) in notes_for_step {
                let is_summary = name.contains("_summary");

                if is_summary {
                    ctx.push_str(&format!("### {}\n{}\n\n", name, value));
                } else if is_recent {
                    let truncated = if value.len() > RECENT_STEP_MAX_CHARS {
                        let end = truncate_at_char_boundary(value, RECENT_STEP_MAX_CHARS);
                        format!("{}...(truncated)", &value[..end])
                    } else {
                        value.clone()
                    };
                    ctx.push_str(&format!("### {}\n{}\n\n", name, truncated));
                } else {
                    let truncated = if value.len() > OLDER_STEP_MAX_CHARS {
                        let end = truncate_at_char_boundary(value, OLDER_STEP_MAX_CHARS);
                        format!("{}...(truncated)", &value[..end])
                    } else {
                        value.clone()
                    };
                    ctx.push_str(&format!("### {}\n{}\n\n", name, truncated));
                }
            }
        }
    }

    log::info!(
        "[notes_injection] Injected {} notes ({} checkpoints + {} step groups + {} non-step) for conversation {}, current_step={}",
        notes.len(), checkpoints.len(), step_notes.len(), non_step_notes.len(), conversation_id, current_step
    );

    // M4-A: Memory quality — injection stats
    {
        let auto_context_count = step_notes
            .values()
            .flat_map(|v| v.iter())
            .filter(|(name, _)| name.contains("auto_context"))
            .count();
        let oldest_step = all_steps.iter().next().copied().unwrap_or(0);
        let newest_step = all_steps.iter().next_back().copied().unwrap_or(0);
        log::info!(
            "[METRICS:memory] conv={} step={} | checkpoints={} summaries={} auto_contexts={} | injected_chars={} | oldest_step={} newest_step={}",
            conversation_id, current_step,
            checkpoints.len(),
            step_notes.values().flat_map(|v| v.iter()).filter(|(n, _)| n.contains("summary")).count(),
            auto_context_count,
            ctx.len(),
            oldest_step, newest_step,
        );
        crate::telemetry::record(
            "memory",
            workspace,
            &[
                ("conv", conversation_id),
                ("step", &current_step.to_string()),
                ("checkpoints", &checkpoints.len().to_string()),
                (
                    "summaries",
                    &step_notes
                        .values()
                        .flat_map(|v| v.iter())
                        .filter(|(n, _)| n.contains("summary"))
                        .count()
                        .to_string(),
                ),
                ("auto_contexts", &auto_context_count.to_string()),
                ("injected_chars", &ctx.len().to_string()),
                ("oldest_step", &oldest_step.to_string()),
                ("newest_step", &newest_step.to_string()),
                ("model", model),
            ],
        );
    }

    ctx
}

/// The core agent loop — stream, execute tools, re-stream until done.
///
/// When `step_config` is `Some`, the loop operates in analysis mode:
/// - Uses the step's system prompt
/// - Filters tools to only the step's relevant tools
/// - Respects the step's max_iterations limit
/// - Auto-advances to the next step when the current step completes
///
/// When `step_config` is `None`, operates in daily consultation mode:
/// - Uses the daily system prompt
/// - All tools available
/// - Standard MAX_TOOL_ITERATIONS limit
async fn agent_loop(
    ctx: AgentContext,
    initial_messages: Vec<ChatMessage>,
    step_config: Option<StepConfig>,
) {
    // Destructure for convenience (avoids `ctx.` prefix on hot-path variables)
    let AgentContext {
        db,
        gateway,
        file_mgr,
        tool_registry,
        session_mgr,
        auth_manager,
        app,
        settings,
        workspace_path,
        conversation_id,
        run_id,
        authorized_workspace,
        assistant_id,
        masking_level,
        session_event_bus,
        cancel_token,
    } = ctx;

    let tavily_api_key = if settings.tavily_api_key.is_empty() {
        None
    } else {
        Some(settings.tavily_api_key.clone())
    };
    let bocha_api_key = if settings.bocha_api_key.is_empty() {
        None
    } else {
        Some(settings.bocha_api_key.clone())
    };

    // Build workspace/file context and analysis notes for the system prompt
    let workspace_context = build_workspace_context(authorized_workspace.as_ref());
    let file_context = build_file_context(&db, &conversation_id, run_id.as_str());

    let current_step_config = step_config;
    let mut messages = initial_messages;
    let current_assistant_id = assistant_id;

    // In daily mode, filter out tool call/result messages from analysis history
    // to maximize useful context within the token budget. Analysis steps produce
    // many tool call + tool result messages that are not useful for daily chat.
    if current_step_config.is_none() {
        messages.retain(|m| {
            // Keep user and system messages
            if m.role == "user" || m.role == "system" {
                return true;
            }
            // Keep assistant messages that have text content (not pure tool-call wrappers)
            if m.role == "assistant" {
                return !m.content.is_empty()
                    && m.tool_calls.as_ref().map_or(true, |tc| tc.is_empty());
            }
            // Filter out tool result messages
            if m.tool_call_id.is_some() {
                return false;
            }
            true
        });

        // Auto-compress long conversation history to preserve early context.
        // Instead of hard-truncating at MAX_HISTORY_MESSAGES (losing everything before),
        // older messages are summarized into a compact recap by a non-streaming LLM call.
        messages = compress_context_if_needed(messages, &gateway, &settings).await;
    }

    // Determine system prompt, tool filter, and token budget based on mode.
    // Daily mode: only DAILY_ALLOWED_TOOLS are visible to the LLM (Primitive + Power +
    // selected Composite tools). Composite tools not in DAILY_ALLOWED_TOOLS are hidden.
    // Analysis mode: uses all tool schemas for KV cache prefix stability;
    // runtime guard enforces per-step allowed_tool_names at execution time.

    let authorized_workspace_present = authorized_workspace.is_some();

    let all_tool_defs = build_visible_tool_defs(&tool_registry, authorized_workspace_present).await;
    let (system_prompt, tool_defs_override, mut max_iterations, token_budget, chunk_timeout_secs) =
        match &current_step_config {
            Some(config) => {
                log::info!("Agent loop in ANALYSIS mode: step={}, tools={}, max_iter={}, token_budget={}, has_precompute={}",
                config.step,
                config.tool_defs.len(),
                config.max_iterations,
                config.token_budget,
                config.precompute.is_some());
                if let Some(ref allowed) = config.allowed_tool_names {
                    log::info!("  allowed_tools: {:?}", allowed);
                }
                (
                    config.system_prompt.clone(),
                    Some(config.tool_defs.clone()),
                    config.max_iterations,
                    config.token_budget,
                    180u64, // analysis mode: tools take time, LLM thinks longer
                )
            }
            None => {
                // Apply DAILY_ALLOWED_TOOLS filter: only expose Primitive + Power tools
                // (and a subset of Composite tools) to the LLM in daily mode.
                // Composite tools not in DAILY_ALLOWED_TOOLS (browse_and_extract,
                // generate_slides, etc.) are hidden from the default tool surface.
                let daily_allowed_set: std::collections::HashSet<&str> =
                    DAILY_ALLOWED_TOOLS.iter().copied().collect();
                let daily_tool_defs: Vec<_> = all_tool_defs
                    .into_iter()
                    .filter(|td| daily_allowed_set.contains(td.name.as_str()))
                    .collect();
                log::info!(
                    "Agent loop in DAILY CONSULTATION mode ({} tools after DAILY_ALLOWED_TOOLS filter)",
                    daily_tool_defs.len()
                );
                let persona = db.get_active_persona().ok();
                let product_name: Option<String> = auth_manager
                    .get_auth_info()
                    .await
                    .tenant
                    .and_then(|t| t.product_name.filter(|n| !n.is_empty()));
                (
                    prompts::get_system_prompt(None, persona.as_ref(), product_name.as_deref()),
                    Some(daily_tool_defs), // filtered by DAILY_ALLOWED_TOOLS
                    MAX_TOOL_ITERATIONS,
                    8192u32, // daily consultation: needs headroom for generate_report JSON
                    CHUNK_TIMEOUT_SECS, // daily mode: 90s
                )
            }
        };

    // --- System prompt is now STATIC (stable KV cache prefix) ---
    // Dynamic content (file context, analysis notes, analysis profile) is
    // injected as a separate context_message parameter to stream_message().
    // This ensures the system prompt prefix never changes between iterations,
    // allowing LLM providers to reuse their KV cache.

    // Build analysis notes context ONCE per step (doesn't change within a step)
    let analysis_notes_context = build_analysis_notes_context(
        &db,
        &conversation_id,
        current_step_config.as_ref(),
        &workspace_path,
        &settings.primary_model,
    );

    // --- Precompute: execute deterministic Python before LLM starts ---
    // If the step has a precompute script, run it now and cache the result.
    // The cached JSON will be injected into the LLM dynamic context.
    let mut precompute_file_ids: Vec<String> = Vec::new();
    let precompute_context: Option<String> = if let Some(ref config) = current_step_config {
        if let Some(ref precompute) = config.precompute {
            log::info!(
                "[PRECOMPUTE] Executing precompute for step {} (cache_key='{}', code_len={}) conv={}",
                config.step, precompute.cache_key, precompute.python_code.len(), conversation_id
            );
            let timeout = std::time::Duration::from_secs(120);
            let sandbox = build_precompute_sandbox(&workspace_path, authorized_workspace.as_ref());

            // Auto-load uploaded files that haven't been loaded yet.
            // This handles the case where step0 was entered without files (user uploaded later),
            // so load_file was never called and _df would be undefined in precompute scripts.
            {
                let uploaded_files = db
                    .get_uploaded_files_for_conversation(&conversation_id)
                    .unwrap_or_default();
                let auto_load_params =
                    crate::llm::tool_executor::file_load::LoadFileParams {
                        storage: &db,
                        file_manager: &file_mgr,
                        workspace_path: workspace_path.as_path(),
                        conversation_id: &conversation_id,
                        run_id: Some(&run_id),
                        app_handle: Some(&app),
                    };
                for file in &uploaded_files {
                    let file_id = file.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if file_id.is_empty() {
                        continue;
                    }
                    // Use run_id as scope to match LoadFileParams::loaded_scope_id() behavior:
                    // handle_load_file_core writes "loaded:{run_id}:{file_id}" when run_id is Some.
                    let scope_id = run_id.as_str();
                    let loaded_key = file_loaded_key(scope_id, file_id);
                    let failed_key = file_load_failed_key(scope_id, file_id);
                    if db.get_memory(&loaded_key).ok().flatten().is_none()
                        && db.get_memory(&failed_key).ok().flatten().is_none()
                    {
                        log::info!(
                            "[PRECOMPUTE] Auto-loading file '{}' before precompute for conv={}",
                            file_id,
                            conversation_id
                        );
                        let load_args = serde_json::json!({"file_id": file_id});
                        if let Err(e) =
                            crate::llm::tool_executor::file_load::handle_load_file_core(
                                &auto_load_params,
                                &load_args,
                            )
                            .await
                        {
                            log::warn!("[PRECOMPUTE] Auto-load failed for '{}': {}", file_id, e);
                            let _ = db.set_memory(
                                &failed_key,
                                &e.to_string(),
                                Some("auto_load_failed"),
                            );
                        }
                    }
                }
            }

            // Build full code: file preamble (injects _df) + analysis preamble
            // (injects _ANALYSIS_DIR, snapshots, utils) + precompute script
            let file_preamble = crate::llm::tool_executor::file_load::build_loaded_files_preamble(
                &db,
                run_id.as_str(),
                &workspace_path,
            );
            let analysis_preamble = crate::llm::tool_executor::file_load::build_analysis_preamble(
                &conversation_id,
                config.step,
                &workspace_path,
            );
            let knowledge_preamble = precompute
                .knowledge_dir
                .as_ref()
                .map(|dir| build_knowledge_preamble(dir))
                .unwrap_or_default();
            let full_code = format!(
                "{}\n{}\n{}\n{}",
                file_preamble, analysis_preamble, knowledge_preamble, precompute.python_code
            );

            // NOTE: No sandbox.validate_code() for precompute scripts.
            // The preamble uses exec() to load _analysis_utils.py, which would
            // fail the standalone-call check. Precompute scripts + preamble are
            // developer-authored trusted code, not user input. Runtime sandbox
            // (timeout, memory limit, path restriction) still applies via session_mgr.execute().

            match session_mgr
                .execute_for_run(
                    &run_id,
                    &full_code,
                    timeout,
                    &sandbox,
                )
                .await
            {
                Ok(result) if result.result.exit_code == 0 => {
                    log::info!(
                        "[PRECOMPUTE] Success for step {} conv={} (exit_code=0, stdout_len={})",
                        config.step,
                        conversation_id,
                        result.result.stdout.len()
                    );

                    // Parse __GENERATED_FILE__ markers from precompute stdout
                    // (same logic as python.rs:214-278)
                    let workspace_canonical = workspace_path
                        .canonicalize()
                        .unwrap_or_else(|_| workspace_path.clone());
                    for line in result.result.stdout.lines() {
                        if let Some(json_str) = line.strip_prefix("__GENERATED_FILE__:") {
                            if let Ok(file_meta) =
                                serde_json::from_str::<serde_json::Value>(json_str)
                            {
                                let rel_path =
                                    file_meta.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                let filename = file_meta
                                    .get("filename")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let title = file_meta
                                    .get("title")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let fmt = file_meta
                                    .get("format")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("excel");

                                let full_path = workspace_path.join(rel_path);
                                let canonical = match full_path.canonicalize() {
                                    Ok(p) => p,
                                    Err(_) => {
                                        log::warn!("[PRECOMPUTE] Skipping __GENERATED_FILE__: path does not exist: {:?}", full_path);
                                        continue;
                                    }
                                };
                                if !canonical.starts_with(&workspace_canonical) {
                                    log::error!(
                                        "[PRECOMPUTE] Path traversal blocked: {:?}",
                                        canonical
                                    );
                                    continue;
                                }
                                let file_size = std::fs::metadata(&canonical)
                                    .map(|m| m.len() as i64)
                                    .unwrap_or(0);
                                let file_id = uuid::Uuid::new_v4().to_string();

                                if let Err(e) = db.insert_generated_file(
                                    &file_id,
                                    &conversation_id,
                                    None,
                                    filename,
                                    rel_path,
                                    fmt,
                                    file_size,
                                    "data",
                                    Some(title),
                                    1,
                                    true,
                                    None,
                                    None,
                                    None,
                                ) {
                                    log::error!(
                                        "[PRECOMPUTE] Failed to register file '{}': {}",
                                        filename,
                                        e
                                    );
                                } else {
                                    log::info!(
                                        "[PRECOMPUTE] Registered file: {} ({})",
                                        filename,
                                        file_id
                                    );
                                    precompute_file_ids.push(file_id);
                                }
                            }
                        }
                    }

                    // Read cached result JSON
                    let cache_path = workspace_path
                        .join("analysis")
                        .join(&conversation_id)
                        .join(format!("{}.json", precompute.cache_key));
                    match std::fs::read_to_string(&cache_path) {
                        Ok(content) => {
                            log::info!(
                                "[PRECOMPUTE] Loaded cached result from {:?} ({} chars)",
                                cache_path,
                                content.len()
                            );
                            Some(content)
                        }
                        Err(e) => {
                            log::warn!(
                                "[PRECOMPUTE] Cache file not found at {:?}: {}. Using stdout as fallback.",
                                cache_path, e
                            );
                            // Fallback: use stdout output directly
                            if !result.result.stdout.trim().is_empty() {
                                Some(result.result.stdout.clone())
                            } else {
                                None
                            }
                        }
                    }
                }
                Ok(result) => {
                    log::warn!(
                        "[PRECOMPUTE] Failed for step {} conv={} (exit_code={}, stderr='{}'). Falling back to agent mode.",
                        config.step, conversation_id, result.result.exit_code,
                        truncate_for_ui(&result.result.stderr, 500)
                    );
                    None
                }
                Err(e) => {
                    log::error!(
                        "[PRECOMPUTE] Execution error for step {} conv={}: {}. Falling back to agent mode.",
                        config.step, conversation_id, e
                    );
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // P2: Load structured analysis context (file profiles + findings).
    // Only active in analysis mode — provides persistent file metadata so the LLM
    // doesn't need to re-discover file structure through tool output on every iteration.
    let is_analysis = current_step_config.is_some();
    let mut analysis_ctx = if is_analysis {
        AnalysisContext::load_or_default(&workspace_path, &conversation_id)
    } else {
        AnalysisContext::default()
    };

    // Build allowed-tool set for execution-time enforcement.
    // Analysis mode: uses StepConfig.allowed_tool_names from the skill's workflow.toml.
    // Daily mode: blocks analysis-only tools that shouldn't be used in casual chat.
    // The LLM sees all tool schemas (for KV cache stability), but calling a
    // tool not in the allowed set will be blocked with an error message.
    //
    // Precompute degradation: if precompute was expected but failed, expand allowed
    // tools to include the feedback set so the LLM can do the work itself.
    let allowed_tools: Option<std::collections::HashSet<String>> = if let Some(ref config) =
        current_step_config
    {
        if (config.precompute.is_some() && precompute_context.is_none()) || config.is_feedback {
            // Precompute failed OR user is providing feedback on a completed step — use feedback mode tools
            log::warn!(
                "[PRECOMPUTE] Degrading step {} to feedback mode (precompute_failed={}, is_feedback={}) conv={}",
                config.step, precompute_context.is_none(), config.is_feedback, conversation_id
            );
            config
                .feedback_config
                .as_ref()
                .map(|fc| {
                    fc.tools
                        .iter()
                        .cloned()
                        .collect::<std::collections::HashSet<_>>()
                })
                .or_else(|| config.allowed_tool_names.clone())
        } else {
            // Normal: use skill-provided allowed list
            config.allowed_tool_names.clone()
        }
    } else {
        // Daily mode: block analysis-only tools at runtime
        let daily_blocked: std::collections::HashSet<String> = [
            "hypothesis_test",
            "detect_anomalies",
            "save_analysis_note",
            "update_progress",
            "update_plan",
            // Browser tools are handled by browse_data sub-agent, not directly by daily mode
            "browse_navigate",
            "browse_and_extract",
            "read_page_content",
            "page_execute_js",
            "extract_table_data",
            "extract_with_pagination",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let daily_allowed: std::collections::HashSet<String> = tool_defs_override
            .as_ref()
            .map(|defs| {
                defs.iter()
                    .map(|td| td.name.clone())
                    .filter(|name| !daily_blocked.contains(name))
                    .collect()
            })
            .unwrap_or_default();
        Some(daily_allowed)
    };

    // Precompute degradation / feedback mode: adjust max_iterations
    if let Some(ref config) = current_step_config {
        if config.precompute.is_some() && precompute_context.is_none() {
            // Precompute failed: use 15 iterations (v1 baseline) as fallback
            let fallback_iters = 15;
            log::warn!(
                "[PRECOMPUTE] Increasing max_iterations from {} to {} for degraded step {} conv={}",
                max_iterations,
                fallback_iters,
                config.step,
                conversation_id
            );
            max_iterations = fallback_iters;
        } else if config.is_feedback {
            // User feedback mode: use feedback_config.max_iterations if available
            if let Some(ref fc) = config.feedback_config {
                log::info!(
                    "[FEEDBACK] Using feedback max_iterations {} for step {} conv={}",
                    fc.max_iterations,
                    config.step,
                    conversation_id
                );
                max_iterations = fc.max_iterations;
            }
        }
    }

    let mut full_content = String::new();
    let mut combined_mask_ctx: Option<MaskingContext> = None;
    let mut generated_file_ids: Vec<String> = precompute_file_ids;
    let mut all_file_metas: Vec<FileMeta> = Vec::new();
    let mut iteration_count = 0usize;
    let mut stream_cancelled = false;
    let mut step_tokens_in: u32 = 0;
    let mut step_tokens_out: u32 = 0;
    let mut safeguard_phase1_injected = false;
    let mut force_no_tools = false;
    let step_start = std::time::Instant::now();
    let mut phase = PhaseTracker::new(
        settings.enable_taor_tracking,
        conversation_id.clone(),
        app.clone(),
        max_iterations,
    );

    // --- Context metrics: baseline snapshot before iteration loop ---
    {
        let msg_total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        let tool_result_chars: usize = messages
            .iter()
            .filter(|m| m.tool_call_id.is_some())
            .map(|m| m.content.len())
            .sum();
        let tool_result_count = messages.iter().filter(|m| m.tool_call_id.is_some()).count();
        let initial_ctx_prompt = if is_analysis {
            analysis_ctx.format_for_prompt()
        } else {
            String::new()
        };
        log::info!(
            "[CTX_METRICS] conversation={} mode={} step={:?} | \
             system_prompt={}chars (STATIC) | messages={} (tool_results={}) | \
             msg_total={}chars (tool_results={}chars) | \
             file_context={}chars | notes_context={}chars | \
             analysis_ctx={}chars",
            conversation_id,
            if is_analysis { "analysis" } else { "daily" },
            current_step_config.as_ref().map(|c| c.step),
            system_prompt.len(),
            messages.len(),
            tool_result_count,
            msg_total_chars,
            tool_result_chars,
            file_context.len(),
            analysis_notes_context.len(),
            initial_ctx_prompt.len(),
        );
    }

    let mut stream_retry_count: u32 = 0;
    let mut stream_needs_retry: bool = false;

    for iteration in 0..max_iterations {
        iteration_count = iteration + 1;
        phase.next_iteration(iteration);
        phase.think();
        log::info!(
            "=== [AGENT] iteration={}/{} conversation={} step={:?} messages={} full_content_len={} ===",
            iteration, max_iterations, conversation_id,
            current_step_config.as_ref().map(|c| c.step),
            messages.len(), full_content.len()
        );
        // Log each message role + content length for debugging
        for (i, m) in messages.iter().enumerate() {
            let has_tc = m.tool_calls.as_ref().map_or(0, |v| v.len());
            let tc_id = m.tool_call_id.as_deref().unwrap_or("-");
            log::debug!(
                "[AGENT] msg[{}] role={} len={} tool_call_id={} tool_calls={}",
                i,
                m.role,
                m.content.len(),
                tc_id,
                has_tc
            );
        }

        // Build dynamic context message (changes each iteration as AnalysisContext updates)
        // This is separate from system_prompt to preserve KV cache prefix stability.

        // Pre-compute connector context (async, must be done before sync block)
        let connector_ctx =
            if let Some(ce) = app.try_state::<Arc<crate::connector::ConnectorEngine>>() {
                ce.reset_request_counters().await;
                let ctx_str = ce.build_context().await;
                if ctx_str.is_empty() {
                    None
                } else {
                    Some(ctx_str)
                }
            } else {
                None
            };

        let dynamic_ctx = {
            let mut ctx = String::from("[动态上下文 — 请勿回复此消息]\n");

            // Cognitive core memory — always loaded (cross-session knowledge base)
            let core_mem = db.load_core_memory();
            if !core_mem.is_empty() {
                ctx.push_str("\n[核心记忆]\n");
                ctx.push_str(&core_mem);
                ctx.push_str("\n");
            }

            if !workspace_context.is_empty() {
                ctx.push_str(&workspace_context);
            }
            if !file_context.is_empty() {
                ctx.push_str(&file_context);
            }
            if !analysis_notes_context.is_empty() {
                ctx.push_str(&analysis_notes_context);
            }
            // Inject precompute result into LLM context
            if let Some(ref pc_result) = precompute_context {
                ctx.push_str("\n\n[precompute_result]\n");
                ctx.push_str("以下是系统自动计算的结果，请基于这些数据向用户展示分析结论。\n");
                ctx.push_str(pc_result);
                ctx.push_str("\n[/precompute_result]");
            }
            // Inject internal system connector context (V4: open browsing + legacy apps)
            if let Some(ref connector) = connector_ctx {
                ctx.push_str("\n\n[内部系统浏览]\n");
                ctx.push_str("你可以通过 browse_navigate 工具直接打开浏览器访问任意内部系统 URL，用户在 Chrome 中登录后即可读取数据。\n\n");
                ctx.push_str(connector);
                ctx.push_str("\n[/内部系统浏览]");
            }
            if is_analysis {
                let ctx_prompt = analysis_ctx.format_for_prompt();
                if !ctx_prompt.is_empty() {
                    ctx.push_str(&ctx_prompt);
                }

                // Inject step plan content (P1)
                let plan_path = workspace_path
                    .join("analysis")
                    .join(&conversation_id)
                    .join("_plan.md");
                if plan_path.exists() {
                    if let Ok(plan) = std::fs::read_to_string(&plan_path) {
                        let plan = plan.trim();
                        if !plan.is_empty() {
                            let plan_display = if plan.len() > 4000 {
                                let boundary = plan.floor_char_boundary(4000);
                                format!("{}...(truncated)", &plan[..boundary])
                            } else {
                                plan.to_string()
                            };
                            ctx.push_str("\n\n[当前步骤计划]\n");
                            ctx.push_str(&plan_display);
                        }
                    }
                }
            }
            ctx
        };
        let dynamic_ctx_ref = if dynamic_ctx.len() > "[动态上下文 — 请勿回复此消息]\n".len()
        {
            Some(dynamic_ctx.as_str())
        } else {
            None // No dynamic content to inject
        };

        // Stream from LLM with system prompt and tool filter
        // P1: Apply context decay to reduce older tool outputs before sending to LLM.
        // Non-destructive — original `messages` vec is preserved for checkpoint/auto_capture.
        let decayed_messages = context_decay::apply_decay(&messages, is_analysis);

        // --- Context metrics: per-iteration snapshot ---
        {
            let orig_chars: usize = messages.iter().map(|m| m.content.len()).sum();
            let decayed_chars: usize = decayed_messages.iter().map(|m| m.content.len()).sum();
            let tool_chars: usize = messages
                .iter()
                .filter(|m| m.tool_call_id.is_some())
                .map(|m| m.content.len())
                .sum();
            let decayed_tool_chars: usize = decayed_messages
                .iter()
                .filter(|m| m.tool_call_id.is_some())
                .map(|m| m.content.len())
                .sum();
            let saved = orig_chars.saturating_sub(decayed_chars);
            log::info!(
                "[CTX_METRICS] iter={}/{} conv={} | \
                 messages={} orig={}chars decayed={}chars saved={}chars | \
                 tool_results: orig={}chars decayed={}chars | \
                 system_prompt={}chars (STATIC) dynamic_ctx={}chars",
                iteration,
                max_iterations,
                conversation_id,
                messages.len(),
                orig_chars,
                decayed_chars,
                saved,
                tool_chars,
                decayed_tool_chars,
                system_prompt.len(),
                dynamic_ctx_ref.map_or(0, |s| s.len()),
            );
        }

        let stream_start = std::time::Instant::now();
        // Phase 3 hard cutoff: send empty tool_defs so LLM physically cannot call tools
        let effective_tools = if force_no_tools {
            log::info!(
                "[AGENT] force_no_tools=true, sending empty tool_defs to LLM (conversation={})",
                conversation_id
            );
            Some(vec![])
        } else {
            tool_defs_override.clone()
        };
        log::info!("[AGENT] Calling gateway.stream_message() model={} system_prompt_len={} dynamic_ctx_len={} messages={} (decayed={}) tools={} force_no_tools={}",
            settings.primary_model, system_prompt.len(), dynamic_ctx_ref.map_or(0, |s| s.len()),
            messages.len(), decayed_messages.len(),
            effective_tools.as_ref().map_or(0, |t| t.len()),
            force_no_tools);
        let stream_result = gateway
            .stream_message(
                &settings,
                decayed_messages,
                masking_level.clone(),
                Some(&system_prompt),
                dynamic_ctx_ref,
                effective_tools,
                token_budget,
                Some(&conversation_id),
            )
            .await;

        let (_task_id, mut stream, mask_ctx, mut cancel_rx) = match stream_result {
            Ok(r) => {
                log::info!("gateway.stream_message() OK, task_id={}", r.0);
                r
            }
            Err(e) => {
                let err_str = e.to_string();
                log::error!(
                    "gateway.stream_message() FAILED at iteration {}/{}: {}",
                    iteration_count,
                    max_iterations,
                    err_str
                );

                // Retry transient errors within the iteration loop
                if stream_retry_count < MAX_STREAM_RETRIES && is_retryable_stream_error(&err_str) {
                    stream_retry_count += 1;
                    log::warn!(
                        "[AGENT] Gateway error is retryable, retrying in {}s (attempt {}/{}) conv={}",
                        STREAM_RETRY_DELAY_SECS, stream_retry_count, MAX_STREAM_RETRIES, conversation_id
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(STREAM_RETRY_DELAY_SECS))
                        .await;
                    continue;
                }

                let user_error = classify_llm_error(&err_str);
                let _ = app.emit(
                    "streaming:error",
                    serde_json::json!({
                        "conversationId": conversation_id,
                        "error": user_error,
                        "errorType": "gateway_error",
                        "rawError": truncate_for_ui(&err_str, 200),
                        "iteration": iteration_count,
                        "partialContent": !full_content.is_empty(),
                    }),
                );

                // Save partial content if any was generated in previous iterations
                if !full_content.trim().is_empty() {
                    log::info!(
                        "[AGENT] Saving partial content ({} chars) before gateway error exit",
                        full_content.len()
                    );
                    finish_agent(
                        &db,
                        &app,
                        &current_assistant_id,
                        &conversation_id,
                        &full_content,
                        combined_mask_ctx.as_ref(),
                        &generated_file_ids,
                        &workspace_path,
                        &all_file_metas,
                    );
                }

                // streaming:done is emitted by AgentGuard::clear() after agent_loop returns
                return;
            }
        };

        // Merge this iteration's masking context into the combined context.
        // Each iteration may discover new PII in tool results; merging ensures
        // the final unmask() covers all mappings from all iterations.
        match combined_mask_ctx.as_mut() {
            Some(existing) => existing.merge(mask_ctx),
            None => {
                combined_mask_ctx = Some(mask_ctx);
            }
        }

        // Collect this iteration's content and tool calls
        let mut iter_content = String::new();
        let mut tool_calls = Vec::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut delta_count: u32 = 0;

        loop {
            let chunk_timeout =
                tokio::time::sleep(std::time::Duration::from_secs(chunk_timeout_secs));
            tokio::select! {
                // Cancel signal — fires immediately even if HTTP is stuck
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow() {
                        log::info!("[AGENT] Cancel signal received for conversation {}", conversation_id);
                        cancel_token.cancel();
                        stream_cancelled = true;
                        break;
                    }
                }
                // No data for chunk_timeout_secs — treat as stalled
                _ = chunk_timeout => {
                    log::error!("[AGENT] Chunk timeout ({}s) for conversation {} at iteration {}/{}",
                        chunk_timeout_secs, conversation_id, iteration_count, max_iterations);

                    // Retry chunk timeout as a transient error
                    if stream_retry_count < MAX_STREAM_RETRIES {
                        stream_retry_count += 1;
                        log::warn!(
                            "[AGENT] Chunk timeout is retryable, will retry iteration (attempt {}/{}) conv={}",
                            stream_retry_count, MAX_STREAM_RETRIES, conversation_id
                        );
                        iter_content.clear();
                        tool_calls.clear();
                        stream_needs_retry = true;
                        break;
                    }

                    // All retries exhausted — emit error and exit
                    let error_msg = if is_analysis {
                        format!(
                            "响应超时（{}秒无数据）。可能原因：\n\
                            1. 网络连接不稳定\n\
                            2. AI 服务响应缓慢\n\
                            3. 当前步骤计算量较大\n\n\
                            已保存当前进度，请稍后重试。",
                            chunk_timeout_secs
                        )
                    } else {
                        format!(
                            "响应超时（{}秒无数据）。请检查网络连接后重试。",
                            chunk_timeout_secs
                        )
                    };

                    let _ = app.emit("streaming:error", serde_json::json!({
                        "conversationId": conversation_id,
                        "error": error_msg,
                        "errorType": "chunk_timeout",
                        "timeoutSeconds": chunk_timeout_secs,
                        "iteration": iteration_count,
                        "maxIterations": max_iterations,
                        "partialContent": !full_content.is_empty(),
                    }));

                    // Save partial content if any was generated before timeout
                    if !full_content.trim().is_empty() {
                        log::info!("[AGENT] Saving partial content ({} chars) before timeout exit", full_content.len());
                        finish_agent(
                            &db, &app, &current_assistant_id, &conversation_id,
                            &full_content, combined_mask_ctx.as_ref(),
                            &generated_file_ids, &workspace_path, &all_file_metas,
                        );
                    }

                    // streaming:done is emitted by AgentGuard::clear() after agent_loop returns
                    return;
                }
                // Normal stream event
                event = stream.next() => {
                    match event {
                        Some(StreamEvent::ContentDelta { delta }) => {
                            let clean = strip_thinking_markers(&delta);
                            if !clean.is_empty() {
                                delta_count += 1;
                                iter_content.push_str(&clean);
                                full_content.push_str(&clean);

                                // Leak detection: check current iteration content only
                                // (full_content spans all iterations — causes false positives
                                // when LLM legitimately mentions internal names across steps)
                                if delta_count % 5 == 0 || iter_content.len() > 200 {
                                    if let prompt_guard::LeakCheckResult::Leaked { matched_count, .. } =
                                        prompt_guard::check_for_leak(&iter_content)
                                    {
                                        log::warn!(
                                            "[AGENT] Prompt leak detected mid-stream for {} ({} fingerprints)",
                                            conversation_id, matched_count
                                        );
                                        full_content.clear();
                                        full_content.push_str(prompt_guard::LEAK_REFUSAL);
                                        iter_content.clear();
                                        break;
                                    }
                                }

                                let _ = app.emit(
                                    "streaming:delta",
                                    serde_json::json!({
                                        "conversationId": conversation_id,
                                        "delta": clean,
                                    }),
                                );
                            }
                        }
                        Some(StreamEvent::ThinkingDelta { .. }) => {
                            // ThinkingDelta contains internal model reasoning (e.g. DeepSeek R1
                            // <think> tokens). We intentionally drop these because:
                            // 1. They bypass strip_thinking_markers() and prompt_guard::check_for_leak
                            // 2. They would pollute the user-visible streamingContent in the frontend
                            // 3. They are implementation-internal, not meant for the user
                        }
                        Some(StreamEvent::ToolCallStart { tool_call }) => {
                            log::info!(
                                "[AGENT] Tool call received: name='{}' id='{}' args={}",
                                tool_call.name,
                                tool_call.id,
                                serde_json::to_string(&tool_call.arguments)
                                    .unwrap_or_else(|_| "??".into())
                            );
                            tool_calls.push(tool_call);
                        }
                        Some(StreamEvent::Done {
                            stop_reason: reason,
                            usage,
                            ..
                        }) => {
                            stop_reason = reason;
                            let stream_elapsed = stream_start.elapsed();
                            log::info!(
                                "[AGENT] Stream done: stop_reason={:?}, usage=({} in / {} out), deltas={}, content_len={}, tool_calls={}, elapsed={:?}",
                                stop_reason, usage.input_tokens, usage.output_tokens,
                                delta_count, iter_content.len(), tool_calls.len(), stream_elapsed
                            );

                            // M5: Token utilization
                            {
                                let total = usage.input_tokens + usage.output_tokens;
                                let util = if token_budget > 0 {
                                    (total as f64 / token_budget as f64) * 100.0
                                } else {
                                    0.0
                                };
                                log::info!(
                                    "[METRICS:tokens] conv={} iter={} model={} | budget={} in={} out={} total={} util={:.0}%",
                                    conversation_id, iteration_count, settings.primary_model,
                                    token_budget, usage.input_tokens, usage.output_tokens, total, util,
                                );
                                crate::telemetry::record("tokens", &workspace_path, &[
                                    ("conv", &conversation_id),
                                    ("iter", &iteration_count.to_string()),
                                    ("model", &settings.primary_model),
                                    ("budget", &token_budget.to_string()),
                                    ("in", &usage.input_tokens.to_string()),
                                    ("out", &usage.output_tokens.to_string()),
                                    ("total", &total.to_string()),
                                    ("util", &format!("{:.0}%", util)),
                                ]);
                            }

                            // Accumulate tokens for M3 step lifecycle
                            step_tokens_in += usage.input_tokens;
                            step_tokens_out += usage.output_tokens;

                            break;
                        }
                        Some(StreamEvent::Error { error }) => {
                            log::error!("[AGENT] Stream error at iteration {}/{}: {}", iteration_count, max_iterations, error);

                            // Retry transient stream errors (5xx, timeout, connection)
                            if stream_retry_count < MAX_STREAM_RETRIES && is_retryable_stream_error(&error) {
                                stream_retry_count += 1;
                                log::warn!(
                                    "[AGENT] Stream error is retryable, will retry iteration (attempt {}/{}) conv={}",
                                    stream_retry_count, MAX_STREAM_RETRIES, conversation_id
                                );
                                // Discard partial content from this failed iteration
                                // (full_content already has prior iterations' content — safe)
                                iter_content.clear();
                                tool_calls.clear();
                                // Break inner stream loop to trigger retry via `continue` in outer loop
                                stream_needs_retry = true;
                                break;
                            }

                            let user_error = classify_llm_error(&error);
                            let _ = app.emit(
                                "streaming:error",
                                serde_json::json!({
                                    "conversationId": conversation_id,
                                    "error": user_error,
                                    "errorType": "stream_error",
                                    "rawError": truncate_for_ui(&error, 200),
                                    "partialContent": !full_content.is_empty(),
                                }),
                            );

                            // Save partial content if any was generated before error
                            if !full_content.trim().is_empty() {
                                log::info!("[AGENT] Saving partial content ({} chars) before stream error exit", full_content.len());
                                finish_agent(
                                    &db, &app, &current_assistant_id, &conversation_id,
                                    &full_content, combined_mask_ctx.as_ref(),
                                    &generated_file_ids, &workspace_path, &all_file_metas,
                                );
                            }

                            // streaming:done is emitted by AgentGuard::clear() after agent_loop returns
                            return;
                        }
                        None => {
                            log::info!("[AGENT] Stream ended (None) for conversation {}", conversation_id);
                            break;
                        }
                    }
                }
            }
        }

        // If cancelled, break out of the iteration loop
        if stream_cancelled {
            break;
        }

        // If stream error triggered a retry, skip tool processing and re-enter the iteration loop.
        if stream_needs_retry {
            stream_needs_retry = false;
            log::info!(
                "[AGENT] Stream retry: re-entering iteration loop after {}s delay (retry {}/{})",
                STREAM_RETRY_DELAY_SECS,
                stream_retry_count,
                MAX_STREAM_RETRIES
            );
            tokio::time::sleep(std::time::Duration::from_secs(STREAM_RETRY_DELAY_SECS)).await;
            continue;
        }

        // Reset stream retry counter on successful iteration
        stream_retry_count = 0;

        // If no tool calls, finish this step — unless ghost calls were dropped
        if tool_calls.is_empty() {
            // Ghost call recovery: if stop_reason=ToolUse but all tool calls were
            // dropped (empty args from SSE chunk loss), inject a retry prompt instead
            // of exiting. This prevents "模型未能生成回复" when the LLM tried to call
            // a tool but SSE transmission lost all argument data.
            if stop_reason == StopReason::ToolUse && iter_content.is_empty() {
                log::warn!(
                    "[AGENT] Ghost call recovery: stop_reason=ToolUse but 0 tool calls survived \
                     (SSE chunk loss dropped all args). Injecting retry prompt. conv={} step={:?} iter={}/{}",
                    conversation_id,
                    current_step_config.as_ref().map(|c| c.step),
                    iteration + 1, max_iterations
                );
                messages.push(ChatMessage::text(
                    "user",
                    "⚠️ 系统提示：你刚才的工具调用因网络传输问题丢失了，请重新调用工具继续执行。",
                ));
                continue;
            }

            log::info!(
                "[AGENT] Finishing step (no tool calls): stop_reason={:?}, total_content_len={}, iter_content_preview='{}'",
                stop_reason, full_content.len(),
                truncate_for_ui(&iter_content, 100)
            );
            // Warn if analysis mode exits on iteration 0 with text-only response.
            // This likely means the LLM is "acknowledging" the step instruction
            // instead of directly calling tools — a prompt effectiveness issue.
            if is_analysis && iteration_count <= 1 && !iter_content.is_empty() {
                log::warn!(
                    "[AGENT] ⚠️ Analysis step exited on first iteration with text-only response \
                     (no tool calls). This suggests the step prompt is not compelling enough \
                     to trigger immediate tool use. conv={} step={:?} text='{}'",
                    conversation_id,
                    current_step_config.as_ref().map(|c| c.step),
                    truncate_for_ui(&iter_content, 200)
                );
            }
            break;
        }
        // Tool calls present but stop_reason doesn't match ToolUse.
        // This can happen when an SSE JSON chunk is malformed and the
        // finish_reason field is lost. Proceed with tool execution anyway
        // to avoid silently dropping tool calls.
        if stop_reason != StopReason::ToolUse {
            log::warn!(
                "[AGENT] stop_reason={:?} but {} tool calls received — proceeding with tool execution \
                 (possible SSE chunk loss, conversation={}). Tool names: {:?}",
                stop_reason, tool_calls.len(), conversation_id,
                tool_calls.iter().map(|tc| tc.name.as_str()).collect::<Vec<_>>()
            );
        }

        log::info!(
            "[AGENT] Continuing to tool execution: iter={}/{} tool_calls={} names={:?} iter_content_len={} (conversation={})",
            iteration + 1, max_iterations,
            tool_calls.len(),
            tool_calls.iter().map(|tc| tc.name.as_str()).collect::<Vec<_>>(),
            iter_content.len(),
            conversation_id
        );

        // --- Tool execution phase ---
        phase.act(tool_calls.iter().map(|tc| tc.name.clone()).collect());
        messages.push(ChatMessage::assistant_with_tool_calls(
            iter_content,
            tool_calls.clone(),
        ));

        let plugin_ctx = PluginContext {
            storage: db.clone(),
            file_manager: file_mgr.clone(),
            workspace_path: workspace_path.clone(),
            conversation_id: conversation_id.clone(),
            session_id: SessionId::new(conversation_id.clone()),
            run_id: Some(run_id.clone()),
            agent_id: None,
            tavily_api_key: tavily_api_key.clone(),
            bocha_api_key: bocha_api_key.clone(),
            app_handle: Some(app.clone()),
            session_manager: session_mgr.clone(),
            auth_manager: Some(auth_manager.clone()),
            connector_engine: app
                .try_state::<Arc<crate::connector::ConnectorEngine>>()
                .map(|s| s.inner().clone()),
            use_cloud: settings.use_cloud,
            model: settings.primary_model.clone(),
            gateway: Some(Arc::clone(&gateway)),
            tool_registry: Some(Arc::clone(&tool_registry)),
            app_settings: Some(Arc::new(settings.clone())),
            agent_runtime: app
                .try_state::<Arc<crate::runtime::agent::AgentRuntime>>()
                .map(|s| s.inner().clone()),
            event_bus: None,
            authorized_workspace: authorized_workspace.clone(),
        };

        // --- Runtime-owned tool dispatch (Phase 1+2+3) ---
        // Build runtime dispatcher from the legacy tool registry so all tools
        // (RuntimeTool and LegacyToolAdapter) are available through the runtime path.
        let dispatcher = tool_registry.to_runtime_dispatcher(plugin_ctx).await;
        let browser_available = app
            .try_state::<Arc<crate::connector::ConnectorEngine>>()
            .is_some();
        let query_engine = QueryEngine::with_dispatcher(dispatcher)
            .with_workspace_path(workspace_path.clone())
            .with_authorized_workspace(authorized_workspace.clone())
            .with_browser_available(browser_available)
            .with_file_ops(Arc::new(
                crate::runtime::tools::capability::DefaultFileOperations {
                    storage: db.clone(),
                    file_manager: file_mgr.clone(),
                    workspace_path: workspace_path.clone(),
                    conversation_id: conversation_id.clone(),
                    run_id: Some(run_id.clone()),
                },
            ));

        // Create or reuse event bus for tool round.
        // When a session-level bus is available, tool events flow through the
        // RuntimeChatTurnDriver's bus (and its TauriEventAdapter subscriber),
        // making them visible to recording hosts in tests and to the session
        // event log.  When no session bus is present (standalone agent_loop),
        // fall back to a locally-constructed bus wired to a TauriRuntimeHost.
        let round_bus = if let Some(ref sbus) = session_event_bus {
            sbus.clone()
        } else {
            let round_host: Arc<dyn crate::transport::runtime_host::RuntimeHost> =
                Arc::new(crate::transport::tauri_runtime_host::TauriRuntimeHost::new(app.clone()));
            let local_bus = RuntimeEventBus::new();
            local_bus.subscribe(Arc::new(TauriEventAdapter::new(round_host)));
            local_bus
        };

        // Build a TurnState for this tool round (identity from conversation_id + run_id).
        let round_mapping = crate::runtime::identity::IdentityMapping::from_legacy_conversation_id(
            conversation_id.clone(),
        );
        let round_turn = crate::runtime::state::TurnState::new(
            round_mapping,
            run_id.clone(),
            String::new(), // user_input not relevant for tool round
        )
        .with_cancellation(cancel_token.child_token());

        // Convert LLM tool_calls to RuntimeToolCallRequest.
        let requests: Vec<crate::runtime::chat::tool_round_types::RuntimeToolCallRequest> =
            tool_calls
                .iter()
                .map(|tc| {
                    log::info!(
                        "[AGENT] Executing tool '{}' (id={}) with args: {}",
                        tc.name,
                        tc.id,
                        serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "??".into())
                    );
                    crate::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        args: tc.arguments.clone(),
                        purpose: tc.arguments
                            .get("purpose")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    }
                })
                .collect();

        // Execute through ToolRoundDriver (handles allowed_tools filtering,
        // single/concurrent dispatch, and runtime event emission).
        let round_start = std::time::Instant::now();
        let driver = crate::runtime::chat::tool_round_driver::ToolRoundDriver::new(query_engine)
            .with_allowed_tools_opt(allowed_tools.as_ref().map(|s| s.iter().cloned().collect()));
        let round_results = driver.execute_round(&round_turn, &round_bus, requests).await;
        let round_elapsed = round_start.elapsed();

        // --- Phase 3: Process results sequentially (masking, messages) ---
        // tool:executing and tool:completed events have already been emitted by the
        // runtime bus (for permitted tools) or skipped (for blocked tools).
        let tool_result_count = round_results.len();
        let mut tool_success_count = 0usize;
        let mut tool_error_count = 0usize;
        let mut tool_names: Vec<String> = Vec::new();
        let tool_total_elapsed = round_elapsed;
        for round_result in &round_results {
            // Unpack the result into common fields for downstream processing.
            // AskRequired outcomes use the helper methods which synthesise the
            // "permission ask required" content string for the LLM.
            let (tr_id, tr_name, tr_content, tr_is_error) = match round_result {
                crate::runtime::ToolRoundResult::Ok(outcome) => (
                    outcome.tool_call_id(),
                    outcome.tool_name(),
                    outcome.content(),
                    outcome.is_error(),
                ),
                crate::runtime::ToolRoundResult::Blocked(blocked) => (
                    blocked.tool_call_id.as_str(),
                    blocked.tool_name.as_str(),
                    blocked.reason.as_str(),
                    true, // blocked tools are treated as errors for stats
                ),
            };

            // Collect file_meta from successful tool outcomes into all_file_metas
            // so verify_file_claims and file card semantics work correctly.
            if let crate::runtime::ToolRoundResult::Ok(outcome) = round_result {
                if let Some(meta) = outcome.file_meta() {
                    all_file_metas.push(meta.clone());
                }
            }

            log::info!(
                "[AGENT] Tool '{}' result: is_error={}, content_len={}, preview='{}'",
                tr_name,
                tr_is_error,
                tr_content.len(),
                truncate_for_ui(tr_content, 300),
            );

            // M1: accumulate per-tool stats
            if tr_is_error {
                tool_error_count += 1;
            } else {
                tool_success_count += 1;
            }
            tool_names.push(tr_name.to_string());

            // Collect fileId from tool results
            if !tr_is_error {
                let parsed_json = serde_json::from_str::<serde_json::Value>(tr_content)
                    .ok()
                    .or_else(|| {
                        tr_content.find('{').and_then(|pos| {
                            serde_json::from_str::<serde_json::Value>(&tr_content[pos..]).ok()
                        })
                    });
                if let Some(parsed) = parsed_json {
                    if let Some(file_id) = parsed.get("fileId").and_then(|v| v.as_str()) {
                        generated_file_ids.push(file_id.to_string());
                    }
                }
                for line in tr_content.lines() {
                    if let Some(pos) = line.find("fileId:") {
                        let after = &line[pos + 7..].trim_start();
                        let id: String = after
                            .chars()
                            .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
                            .collect();
                        if id.len() >= 36 && !generated_file_ids.contains(&id) {
                            generated_file_ids.push(id);
                        }
                    }
                }
            }

            let masked_result = match combined_mask_ctx.as_mut() {
                Some(ctx) => ctx.mask_text(tr_content),
                None => tr_content.to_string(),
            };
            const MAX_TOOL_RESULT_CHARS: usize = 8000;
            let truncated_result = if masked_result.len() > MAX_TOOL_RESULT_CHARS {
                let end = truncate_at_char_boundary(&masked_result, MAX_TOOL_RESULT_CHARS);
                log::info!(
                    "[CTX_METRICS] tool='{}' truncated: {}→{}chars",
                    tr_name,
                    masked_result.len(),
                    end
                );
                format!(
                    "{}...\n[output truncated — {} chars total]",
                    &masked_result[..end],
                    masked_result.len()
                )
            } else {
                masked_result
            };
            messages.push(ChatMessage::tool_result(tr_id, tr_name, truncated_result));

            // P2: Update analysis context from tool results (analysis mode only)
            if is_analysis {
                match tr_name {
                    "load_file" => {
                        // Extract file info from tool result for AnalysisContext.
                        // load_file returns JSON: {"status":"loaded","fileId":"...","originalName":"...","columns":[...],...}
                        let parsed = serde_json::from_str::<serde_json::Value>(tr_content)
                            .ok()
                            .or_else(|| {
                                // Fallback: try to find JSON object in the content
                                tr_content.find('{').and_then(|pos| {
                                    serde_json::from_str::<serde_json::Value>(&tr_content[pos..])
                                        .ok()
                                })
                            });
                        if let Some(ref json) = parsed {
                            let file_id = json
                                .get("fileId")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            let original_name = json
                                .get("originalName")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            let var_hint = if tr_content.contains("_dfs[") {
                                format!("_dfs['{}']", file_id)
                            } else {
                                "_df".to_string()
                            };
                            if !file_id.is_empty() {
                                analysis_ctx.update_from_load_file(
                                    file_id,
                                    original_name,
                                    &var_hint,
                                    tr_content,
                                );
                            } else {
                                log::warn!(
                                    "[P2] load_file result JSON missing 'fileId' for conversation {} — \
                                     AnalysisContext file profile will not be created. Keys: {:?}",
                                    conversation_id,
                                    json.as_object().map(|o| o.keys().collect::<Vec<_>>()),
                                );
                            }
                        } else {
                            log::warn!(
                                "[P2] Could not parse load_file result as JSON for conversation {} — \
                                 AnalysisContext file profile will not be created. Preview: '{}'",
                                conversation_id,
                                truncate_for_ui(tr_content, 200),
                            );
                        }
                    }
                    "execute_python" => {
                        analysis_ctx.update_from_python_output(tr_content);
                    }
                    _ => {}
                }
            }
        }

        // P2: Save analysis context to disk after each iteration (crash-safe)
        if is_analysis {
            analysis_ctx.save(&workspace_path, &conversation_id);
        }

        // M1: Tool execution quality — per-iteration summary
        {
            let blocked = tool_calls.len() - tool_result_count;
            log::info!(
                "[METRICS:tool] conv={} step={:?} iter={} | total={} success={} error={} blocked={} | names={:?} | total_elapsed_ms={}",
                conversation_id,
                current_step_config.as_ref().map(|c| c.step),
                iteration_count,
                tool_calls.len(), tool_success_count, tool_error_count, blocked,
                tool_names, tool_total_elapsed.as_millis(),
            );
            crate::telemetry::record(
                "tool",
                &workspace_path,
                &[
                    ("conv", &conversation_id),
                    (
                        "step",
                        &format!("{:?}", current_step_config.as_ref().map(|c| c.step)),
                    ),
                    ("iter", &iteration_count.to_string()),
                    ("total", &tool_calls.len().to_string()),
                    ("success", &tool_success_count.to_string()),
                    ("error", &tool_error_count.to_string()),
                    ("blocked", &blocked.to_string()),
                    ("names", &format!("{:?}", tool_names)),
                    (
                        "total_elapsed_ms",
                        &tool_total_elapsed.as_millis().to_string(),
                    ),
                    ("model", &settings.primary_model),
                ],
            );
        }

        phase.observe();

        // Daily mode safeguard: if we've consumed most iterations without any
        // text output (LLM keeps calling tools), inject a user message to force
        // the LLM to summarize and respond in the next iteration.
        if !is_analysis && full_content.is_empty() && iteration >= max_iterations.saturating_sub(3)
        {
            log::warn!(
                "[AGENT] Daily mode safeguard: {}/{} iterations with no text output, injecting reply prompt (conversation={})",
                iteration + 1, max_iterations, conversation_id
            );
            messages.push(ChatMessage::text(
                "user",
                "请停止调用工具，直接用文字向用户总结目前的分析结果。",
            ));
        }

        // Analysis mode safeguard: when approaching iteration limit, escalate
        // through three phases to ensure the LLM saves notes and produces text.
        //   Phase 1 (once): inject save prompt
        //   Phase 2 (once): inject text-only prompt + arm force_no_tools
        //   Phase 3 (auto): empty tool_defs in next stream_message call
        // Only applies to steps with max_iterations >= 8 (to avoid triggering on short steps like Step 0)
        if is_analysis && max_iterations >= 8 && iteration >= max_iterations.saturating_sub(6) {
            let has_saved_note = messages
                .iter()
                .any(|m| m.role == "tool" && m.name.as_deref() == Some("save_analysis_note"));
            let remaining = max_iterations.saturating_sub(iteration + 1);
            log::info!(
                "[AGENT] Safeguard check: iter={}/{} remaining={} has_saved_note={} has_text={} phase1={} force_no_tools={} (conv={})",
                iteration + 1, max_iterations, remaining, has_saved_note, !full_content.is_empty(),
                safeguard_phase1_injected, force_no_tools, conversation_id
            );

            if !has_saved_note && !safeguard_phase1_injected {
                // Phase 1 (inject once): save notes + summarize
                safeguard_phase1_injected = true;
                log::warn!(
                    "[AGENT] Safeguard Phase 1: {}/{} iters, no save_analysis_note → injecting save prompt (conv={})",
                    iteration + 1, max_iterations, conversation_id
                );
                messages.push(ChatMessage::text(
                    "user",
                    "⚠️ 你即将达到本步骤的迭代上限。请立即：\n\
                     1. 调用 save_analysis_note 保存当前分析结论\n\
                     2. 用文字向用户总结本步骤的分析结果\n\
                     不要再调用 execute_python，直接保存和总结。",
                ));
            } else if full_content.is_empty() && remaining <= 3 && !force_no_tools {
                // Phase 2 (inject once + arm hard cutoff): no text output approaching limit
                // Use remaining <= 3 (not 4) to give LLM at least 1 extra iteration
                // after Phase 1 to call save_analysis_note before we force text output.
                force_no_tools = true;
                log::warn!(
                    "[AGENT] Safeguard Phase 2: no text output at iter {}/{}, arming force_no_tools for next iteration (conv={})",
                    iteration + 1, max_iterations, conversation_id
                );
                messages.push(ChatMessage::text(
                    "user",
                    "⚠️ 迭代即将用完且尚无文字输出。下一次迭代将禁用所有工具。\n\
                     请立即停止调用工具，直接用文字向用户总结本步骤的分析结果。",
                ));
                // Phase 3 is automatic: force_no_tools=true causes empty tool_defs
                // in the next stream_message call, physically preventing tool use.
            }
        }

        // Check cancel signal after tool batch completes
        if *cancel_rx.borrow() {
            log::info!(
                "[AGENT] Cancel signal detected after tool execution for conversation {}",
                conversation_id
            );
            cancel_token.cancel();
            stream_cancelled = true;
        }
    }

    // --- Context metrics: end-of-step summary ---
    {
        let final_msg_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        let final_tool_chars: usize = messages
            .iter()
            .filter(|m| m.tool_call_id.is_some())
            .map(|m| m.content.len())
            .sum();
        let final_tool_count = messages.iter().filter(|m| m.tool_call_id.is_some()).count();
        log::info!(
            "[CTX_METRICS] STEP_DONE conv={} step={:?} iterations={} | \
             final_messages={} (tool_results={}) | \
             final_total={}chars (tool_results={}chars) | \
             output={}chars",
            conversation_id,
            current_step_config.as_ref().map(|c| c.step),
            iteration_count,
            messages.len(),
            final_tool_count,
            final_msg_chars,
            final_tool_chars,
            full_content.len(),
        );
    }

    // M3: Step lifecycle — end-of-step summary
    {
        let status = if stream_cancelled {
            "cancelled"
        } else if iteration_count >= max_iterations {
            "max_iter"
        } else {
            "completed"
        };
        let duration_ms = step_start.elapsed().as_millis();

        // Determine what notes were saved for this step
        let note_saved = if let Some(ref config) = current_step_config {
            let cp_key = format!("note:{}:step{}_checkpoint", conversation_id, config.step);
            let ac_key = format!("note:{}:step{}_auto_context", conversation_id, config.step);
            let sm_key = format!("note:{}:step{}_summary", conversation_id, config.step);
            let has_cp = db
                .get_memory(&cp_key)
                .ok()
                .flatten()
                .map_or(false, |v| !v.is_empty());
            let has_ac = db
                .get_memory(&ac_key)
                .ok()
                .flatten()
                .map_or(false, |v| !v.is_empty());
            let has_sm = db
                .get_memory(&sm_key)
                .ok()
                .flatten()
                .map_or(false, |v| !v.is_empty());
            match (has_cp, has_ac, has_sm) {
                (true, _, _) => "checkpoint",
                (false, true, true) => "summary+auto",
                (false, false, true) => "summary",
                (false, true, false) => "auto_capture",
                (false, false, false) => "none",
            }
        } else {
            "n/a"
        };

        log::info!(
            "[METRICS:step] conv={} step={:?} status={} | iterations={}/{} duration_ms={} | tokens_in={} tokens_out={} | note_saved={} | text_output={}chars",
            conversation_id,
            current_step_config.as_ref().map(|c| c.step),
            status,
            iteration_count, max_iterations, duration_ms,
            step_tokens_in, step_tokens_out,
            note_saved,
            full_content.len(),
        );
        crate::telemetry::record(
            "step",
            &workspace_path,
            &[
                ("conv", &conversation_id),
                (
                    "step",
                    &format!("{:?}", current_step_config.as_ref().map(|c| c.step)),
                ),
                ("status", status),
                ("iterations", &iteration_count.to_string()),
                ("duration_ms", &duration_ms.to_string()),
                ("tokens_in", &step_tokens_in.to_string()),
                ("tokens_out", &step_tokens_out.to_string()),
                ("note_saved", note_saved),
                ("model", &settings.primary_model),
            ],
        );
    }

    // --- Step completion: save assistant message and check auto-advance ---

    // If we hit max_iterations without naturally finishing, append a notice
    if iteration_count >= max_iterations {
        log::warn!(
            "[AGENT] Hit max_iterations ({}) for conversation {} step {:?}",
            max_iterations,
            conversation_id,
            current_step_config.as_ref().map(|c| c.step)
        );
        if current_step_config.is_some() {
            let notice = format!(
                "\n\n---\n⚠️ 本步分析较为复杂，已达处理上限（{} 次迭代）。以上是当前阶段的分析结果。\n\
                如需补充分析，请回复具体要求；如结果已满足需要，请确认继续下一步。",
                max_iterations
            );
            full_content.push_str(&notice);
        }
    }

    // If the LLM returned absolutely nothing (empty response due to content
    // filtering, provider error, or all-thinking-token output), provide a
    // fallback message so the user is not left staring at silence.
    if full_content.trim().is_empty() && !stream_cancelled {
        log::warn!(
            "[AGENT] LLM returned empty content for conversation {} (step={:?}, iterations={})",
            conversation_id,
            current_step_config.as_ref().map(|c| c.step),
            iteration_count
        );
        full_content = "抱歉，模型未能生成回复。可能原因：内容限制、网络问题或服务暂时不可用。请尝试换一种方式提问。".to_string();
    }

    // Strip hallucinated XML function-call blocks before saving
    full_content = strip_hallucinated_xml(&full_content);

    // Post-stream verification: check if LLM falsely claims a format that wasn't produced (Layer 4)
    if let Some(correction) = verify_file_claims(&full_content, &all_file_metas, &workspace_path) {
        full_content.push_str(&correction);
    }

    // Save current step's assistant message
    finish_agent(
        &db,
        &app,
        &current_assistant_id,
        &conversation_id,
        &full_content,
        combined_mask_ctx.as_ref(),
        &generated_file_ids,
        &workspace_path,
        &all_file_metas,
    );

    // --- Step completion: mark as completed and wait for user confirmation ---
    //
    // All analysis steps require user confirmation before advancing.
    // The next user message will trigger send_message() → next_action() →
    // AdvanceStep/RerunStep/FinishAnalysis based on whether the user confirms
    // or gives feedback.
    if let Some(ref config) = current_step_config {
        let completed_step = config.step;

        // Auto-capture step context as fallback before checking notes.
        // This ensures every completed step has at least auto_context saved,
        // even if the LLM didn't call save_analysis_note.
        // Include full_content as a synthetic assistant message since it hasn't
        // been added to `messages` yet (finish_agent saves to DB, not messages vec).
        let mut capture_messages = messages.clone();
        if !full_content.trim().is_empty() {
            capture_messages.push(ChatMessage::text("assistant", &full_content));
        }
        auto_capture_step_context(&db, &conversation_id, completed_step, &capture_messages);

        // Validate that the step has analysis notes (either LLM-saved summary or auto-captured context).
        // Auto-capture always saves step{N}_auto_context, so this is a sanity check.
        // Still warn if the LLM didn't save a curated summary (auto_context is less structured).
        // Include step 0: it carries critical file info and analysis direction.
        {
            let note_prefix = format!("note:{}:step{}_", conversation_id, completed_step);
            let has_any_note = db
                .get_memories_by_prefix(&note_prefix)
                .map(|notes| !notes.is_empty())
                .unwrap_or(false);

            let summary_key = format!("note:{}:step{}_summary", conversation_id, completed_step);
            let has_summary = db
                .get_memory(&summary_key)
                .ok()
                .flatten()
                .map(|v| !v.is_empty())
                .unwrap_or(false);

            if !has_any_note {
                log::warn!(
                    "[AGENT] Step {} completed without ANY notes (prefix '{}') — auto_capture should have saved context as fallback",
                    completed_step, note_prefix
                );
                // NOTE: Do NOT emit streaming:error here. The auto_capture system
                // saves step context as fallback. Emitting streaming:error causes
                // the frontend to clear streaming state, which can interfere with
                // the next step's streaming when the step auto-advances.
            } else if !has_summary {
                log::info!(
                    "[AGENT] Step {} has auto-captured context but no LLM-curated summary. Auto-capture will serve as fallback.",
                    completed_step
                );
            }
        }

        log::info!(
            "[AGENT] Step {} finished for conversation {}, marking as completed",
            completed_step,
            conversation_id
        );

        // Mark this step as completed in DB so next_action() can route correctly.
        if let Err(e) =
            orchestrator::advance_step(&db, &conversation_id, completed_step, "completed")
        {
            log::error!("Failed to mark step {} as completed: {}", completed_step, e);
        }

        if completed_step >= 5 {
            log::info!("[AGENT] Final step (5) completed for conversation {}, awaiting user confirmation to finish", conversation_id);
        }
    }
}

/// Finalize the agent response — save to DB and emit events.
///
/// When `full_content` is empty (LLM produced only tool calls without
/// text output), we skip saving an empty message and skip
/// `message:updated`, but still emit `streaming:done` so the frontend
/// clears its streaming state. This prevents empty assistant bubbles
/// from appearing in the chat.
fn finish_agent(
    db: &Arc<AppStorage>,
    app: &AppHandle,
    assistant_id: &str,
    conversation_id: &str,
    full_content: &str,
    mask_ctx: Option<&MaskingContext>,
    generated_file_ids: &[String],
    workspace_path: &std::path::Path,
    file_metas: &[FileMeta],
) {
    // Unmask PII placeholders before saving to DB
    let unmasked_content = match mask_ctx {
        Some(ctx) => ctx.unmask(full_content),
        None => full_content.to_string(),
    };

    // Leak detection at persistence boundary
    let (unmasked_content, was_leaked) = prompt_guard::filter_leaked_content(&unmasked_content);
    if was_leaked {
        log::warn!(
            "[finish_agent] Prompt leak caught at persistence for conv={}",
            conversation_id
        );
    }

    let unmasked_trimmed = unmasked_content.trim();
    let created_at = chrono::Utc::now().to_rfc3339();

    // Only save and emit a message when there is actual content.
    // During analysis mode the LLM may produce tool-call-only iterations
    // (no visible text); saving those would create empty assistant bubbles.
    if !unmasked_trimmed.is_empty() {
        // Check if conversation still exists (might have been deleted while agent was running)
        if db.get_conversation_mode(conversation_id).is_err() {
            log::warn!(
                "Conversation {} was deleted during agent run, skipping message save",
                conversation_id
            );
            return;
        }

        // Build content JSON, including generated files if any
        let content_value = if !generated_file_ids.is_empty() {
            match db.get_generated_files_by_ids(generated_file_ids) {
                Ok(file_records) if !file_records.is_empty() => {
                    let gen_files: Vec<serde_json::Value> = file_records
                        .iter()
                        .map(|f| {
                            let stored_path = f["storedPath"].as_str().unwrap_or("");
                            let full_path = workspace_path.join(stored_path);
                            let file_id_str = f["id"].as_str().unwrap_or("");
                            // Look up FileMeta to inject degradation info
                            let matching_meta =
                                file_metas.iter().find(|m| m.file_id == file_id_str);
                            let mut file_json = serde_json::json!({
                                "id": f["id"],
                                "fileName": f["fileName"],
                                "filePath": full_path.to_string_lossy(),
                                "fileType": f["fileType"],
                                "fileSize": f["fileSize"],
                                "category": f["category"],
                                "version": f["version"],
                                "isLatest": f["isLatest"],
                                "createdAt": f["createdAt"],
                                "createdByStep": f["createdByStep"],
                                "description": f["description"],
                                "actions": [],
                            });
                            if let Some(meta) = matching_meta {
                                if meta.requested_format != meta.actual_format {
                                    file_json["isDegraded"] = serde_json::json!(true);
                                    file_json["requestedFormat"] =
                                        serde_json::json!(meta.requested_format);
                                    file_json["degradationNotice"] = serde_json::json!(format!(
                                        "{} 转换失败，已降级为 {} 格式",
                                        meta.requested_format.to_uppercase(),
                                        meta.actual_format.to_uppercase()
                                    ));
                                }
                            }
                            file_json
                        })
                        .collect();
                    log::info!(
                        "[AGENT] Attaching {} generated files to message {}",
                        gen_files.len(),
                        assistant_id
                    );
                    serde_json::json!({
                        "text": unmasked_content,
                        "generatedFiles": gen_files,
                    })
                }
                Ok(_) => serde_json::json!({ "text": unmasked_content }),
                Err(e) => {
                    log::error!("Failed to query generated files: {:#}", e);
                    serde_json::json!({ "text": unmasked_content })
                }
            }
        } else {
            serde_json::json!({ "text": unmasked_content })
        };

        // Save assistant message to DB (with unmasked content)
        let content_json = content_value.to_string();
        log::info!(
            "[finish_agent] Saving message id={} conv={} content_len={}",
            assistant_id,
            conversation_id,
            content_json.len()
        );
        match db.insert_message(assistant_id, conversation_id, "assistant", &content_json) {
            Ok(_) => {
                log::info!(
                    "[finish_agent] Message saved, emitting message:updated for {}",
                    assistant_id
                );
                // Emit full message ONLY after DB write succeeds.
                // This prevents the UI from showing a message that would disappear on refresh.
                if let Err(e) = app.emit(
                    "message:updated",
                    serde_json::json!({
                        "id": assistant_id,
                        "conversationId": conversation_id,
                        "role": "assistant",
                        "content": content_value,
                        "createdAt": created_at,
                    }),
                ) {
                    log::warn!("Failed to emit message:updated for {}: {}", assistant_id, e);
                }
            }
            Err(e) => {
                log::error!("Failed to save assistant message to DB: {:#}", e);
                // Do NOT emit message:updated — the message didn't persist
            }
        }
    } else {
        log::info!(
            "[AGENT] Skipping empty assistant message for conversation {}, assistant_id={}",
            conversation_id,
            assistant_id
        );
    }

    // NOTE: streaming:done is NOT emitted here. It is emitted exclusively by
    // AgentGuard::clear() when the entire agent_loop ends (all steps done or
    // confirmation checkpoint reached). This prevents a premature streaming:done
    // during auto-advance between analysis steps, which would cause the frontend
    // to set isStreaming=false while the next step's stream is about to begin.

    // Auto-generate conversation title from first assistant response
    if !unmasked_trimmed.is_empty() {
        if let Ok(msgs) = db.get_messages(conversation_id) {
            let assistant_count = msgs
                .iter()
                .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
                .count();
            if assistant_count <= 1 {
                let clean = strip_hallucinated_xml(unmasked_trimmed);
                let title: String = clean.chars().take(30).collect();
                let title = title
                    .split('\n')
                    .next()
                    .unwrap_or(&title)
                    .trim()
                    .to_string();
                let title = if title.len() < clean.len() {
                    format!("{}...", title)
                } else {
                    title
                };
                if let Err(e) = db.update_conversation_title(conversation_id, &title) {
                    log::warn!("Failed to auto-update conversation title: {}", e);
                }
                // Notify frontend of the title change
                let _ = app.emit(
                    "conversation:title-updated",
                    serde_json::json!({
                        "conversationId": conversation_id,
                        "title": title,
                    }),
                );
            }
        }
    }
}

/// Verify LLM's file format claims against actual file metadata.
///
/// Conservative strategy: only flags when the LLM explicitly mentions a format
/// (e.g. "PDF 已生成") that doesn't match any actual file produced.
/// Uses proximity checking (format keyword within 60 chars of an action word)
/// and excludes negative contexts (失败/不可用/failed etc.) to avoid false positives.
/// Returns a correction footnote to append, or None if no mismatch found.
fn verify_file_claims(
    llm_text: &str,
    file_metas: &[FileMeta],
    workspace_path: &std::path::Path,
) -> Option<String> {
    // Format keywords paired with their format family key
    let format_keywords: &[(&str, &str)] = &[
        ("PDF", "pdf"),
        ("DOCX", "docx"),
        ("Word", "docx"),
        ("Excel", "excel"),
        ("XLS", "excel"),
        ("XLSX", "excel"),
        ("PPT", "pptx"),
        ("PPTX", "pptx"),
        ("PowerPoint", "pptx"),
        ("演示文稿", "pptx"),
    ];

    let action_words_zh = [
        "已生成",
        "已导出",
        "已保存",
        "已创建",
        "生成了",
        "导出了",
        "保存了",
        "创建了",
    ];
    let action_words_en = ["generated", "exported", "saved", "created"];
    let negation_words = [
        "失败",
        "不可用",
        "无法",
        "不支持",
        "未能",
        "没有",
        "not",
        "failed",
        "unavailable",
        "unable",
        "cannot",
    ];

    let actual_formats: Vec<&str> = file_metas
        .iter()
        .map(|m| m.actual_format.as_str())
        .collect();
    let llm_lower = llm_text.to_lowercase();

    let mut false_claims: Vec<(&str, &str)> = Vec::new();
    let mut seen_format_keys: Vec<&str> = Vec::new();

    /// Check if any action word appears within `radius` chars of `pos` in the text.
    /// All searches done in the same text used for position finding to avoid byte offset mismatches.
    fn has_nearby_action(
        text: &str,
        pos: usize,
        radius: usize,
        action_zh: &[&str],
        action_en: &[&str],
    ) -> bool {
        let start = pos.saturating_sub(radius);
        let end = (pos + radius).min(text.len());
        // Ensure we slice on char boundaries
        let start = if text.is_char_boundary(start) {
            start
        } else {
            (start..text.len())
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(start)
        };
        let end = if text.is_char_boundary(end) {
            end
        } else {
            (0..end)
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(end)
        };
        let window = &text[start..end];
        action_zh.iter().any(|aw| window.contains(aw))
            || action_en.iter().any(|aw| window.contains(aw))
    }

    /// Check if any negation word appears within `radius` chars of `pos`.
    fn has_nearby_negation(text: &str, pos: usize, radius: usize, neg_words: &[&str]) -> bool {
        let start = pos.saturating_sub(radius);
        let end = (pos + radius).min(text.len());
        let start = if text.is_char_boundary(start) {
            start
        } else {
            (start..text.len())
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(start)
        };
        let end = if text.is_char_boundary(end) {
            end
        } else {
            (0..end)
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(end)
        };
        let window = &text[start..end];
        neg_words.iter().any(|nw| window.contains(nw))
    }

    // Check 1: files registered but physically missing or empty
    let mut missing_files: Vec<&str> = Vec::new();
    for meta in file_metas {
        let full = workspace_path.join(&meta.stored_path);
        if !full.exists() || meta.file_size == 0 {
            missing_files.push(&meta.file_name);
        }
    }

    // Check 2: LLM claims export/save but NO files were registered at all
    let claims_export_no_files = if file_metas.is_empty() {
        // Look for export claim patterns in the LLM text
        let export_path_patterns = ["exports/", "reports/", "charts/", "presentations/"];
        let has_path_mention = export_path_patterns.iter().any(|p| llm_lower.contains(p));
        let has_action = action_words_zh.iter().any(|w| llm_text.contains(w))
            || action_words_en.iter().any(|w| llm_lower.contains(w));
        has_path_mention && has_action
    } else {
        false
    };

    // Check 3: original format mismatch check
    for (display, format_key) in format_keywords {
        if seen_format_keys.contains(format_key) {
            continue;
        }

        // Search for format keywords using case-insensitive matching (all in llm_lower)
        let display_lower = display.to_lowercase();
        let mut found_positive_claim = false;
        let mut search_from = 0;
        while let Some(pos) = llm_lower[search_from..].find(&display_lower) {
            let abs_pos = search_from + pos;
            search_from = abs_pos + display_lower.len();

            // Proximity: action word must be within 60 chars (search in original text for zh, llm_lower for en)
            if !has_nearby_action(llm_text, abs_pos, 60, &action_words_zh, &[])
                && !has_nearby_action(&llm_lower, abs_pos, 60, &[], &action_words_en)
            {
                continue;
            }
            // Exclude negative context: skip if negation word is nearby
            if has_nearby_negation(llm_text, abs_pos, 40, &negation_words)
                || has_nearby_negation(&llm_lower, abs_pos, 40, &negation_words)
            {
                continue;
            }
            found_positive_claim = true;
            break;
        }

        if !found_positive_claim {
            continue;
        }

        // Check if this format was NOT actually produced
        let was_produced = actual_formats.iter().any(|af| af == format_key);
        if !was_produced {
            if let Some(meta) = file_metas
                .iter()
                .find(|m| m.requested_format == *format_key)
            {
                false_claims.push((display, &meta.actual_format));
                seen_format_keys.push(format_key);
            }
        }
    }

    // Build correction message
    let mut corrections: Vec<String> = Vec::new();

    if !missing_files.is_empty() {
        corrections.push(format!(
            "以下文件未能成功生成：{}。请重试导出操作。",
            missing_files.join("、")
        ));
    }

    if claims_export_no_files {
        corrections.push("文件导出未成功完成，请重试。".to_string());
    }

    for (claimed, actual) in &false_claims {
        corrections.push(format!(
            "实际生成的文件格式为 **{}**（非 {}）。",
            actual.to_uppercase(),
            claimed
        ));
    }

    if corrections.is_empty() {
        return None;
    }

    let mut correction = String::from("\n\n---\n> **更正**：");
    correction.push_str(&corrections.join(" "));
    if !false_claims.is_empty() {
        correction.push_str("请以文件卡片中显示的格式为准。");
    }

    Some(correction)
}

/// Tag a tool result with structured metadata so the LLM can correctly report
/// the actual file format (especially when degraded from the requested format).
#[allow(dead_code)]
fn tag_file_tool_result(
    original_content: &str,
    file_meta: &FileMeta,
    is_degraded: bool,
    notice: Option<&str>,
) -> String {
    let mut tagged = String::new();

    if is_degraded {
        tagged.push_str("[TOOL_STATUS: degraded]\n");
        tagged.push_str(&format!(
            "[REQUESTED_FORMAT: {}]\n",
            file_meta.requested_format
        ));
        tagged.push_str(&format!("[ACTUAL_FORMAT: {}]\n", file_meta.actual_format));
        tagged.push_str(&format!("[FILE: {}]\n", file_meta.file_name));
        if let Some(n) = notice {
            tagged.push_str(&format!("[NOTICE: {}]\n", n));
        }
        tagged.push('\n');
    } else {
        tagged.push_str("[TOOL_STATUS: success]\n");
        tagged.push_str(&format!("[FORMAT: {}]\n", file_meta.actual_format));
        tagged.push_str(&format!("[FILE: {}]\n", file_meta.file_name));
        tagged.push('\n');
    }

    tagged.push_str(original_content);
    tagged
}

/// Truncate text for UI display purposes.
fn truncate_for_ui(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let mut truncated = text.chars().take(max_len).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

/// Classify an LLM error into a user-friendly message.
fn classify_llm_error(error: &str) -> String {
    let lower = error.to_lowercase();
    if lower.contains("429") || lower.contains("rate limit") {
        "AI 服务请求频率超限，请稍等片刻后重试。".to_string()
    } else if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("authentication")
    {
        "API 密钥无效或已过期，请在设置中检查 API Key 配置。".to_string()
    } else if lower.contains("402") || lower.contains("insufficient") || lower.contains("quota") {
        "API 额度不足，请检查账户余额。".to_string()
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "AI 服务连接超时，请检查网络连接后重试。".to_string()
    } else if lower.contains("connection") || lower.contains("network") {
        "网络连接异常，请检查网络后重试。".to_string()
    } else if lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        "AI 服务暂时不可用，请稍后重试。".to_string()
    } else {
        format!("服务异常：{}。请重试。", truncate_for_ui(error, 100))
    }
}

/// Check if an LLM error is retryable (transient server/network errors).
fn is_retryable_stream_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        || lower.contains("network")
        || lower.contains("429")
        || lower.contains("rate limit")
}

/// Attempt to decrypt an API key. Falls back to returning the raw value
/// if it's not in encrypted format or decryption fails.
/// If decryption fails for an encrypted value (contains ':'), return empty
/// string so the caller can fall back to defaults.
fn decrypt_key(ss: &SecureStorage, value: &str) -> String {
    if value.is_empty() || !value.contains(':') {
        log::info!(
            "decrypt_key: value has no colon (len={}), returning as-is",
            value.len()
        );
        return value.to_string();
    }
    match ss.decrypt(value) {
        Ok(plaintext) => {
            log::info!(
                "decrypt_key: decryption OK, plaintext len={}",
                plaintext.len()
            );
            plaintext
        }
        Err(e) => {
            log::warn!(
                "Failed to decrypt API key (err={}), returning empty to trigger default fallback",
                e
            );
            String::new()
        }
    }
}

/// Strip DeepSeek internal thinking markers from content deltas.
///
/// DeepSeek models sometimes leak `<｜end▁of▁thinking｜>` and similar
/// markers into the regular content stream. These should not be shown
/// to users.
fn strip_thinking_markers(text: &str) -> String {
    text.replace("<｜end▁of▁thinking｜>", "")
        .replace("<｜begin▁of▁thinking｜>", "")
        .replace("<|end▁of▁thinking|>", "")
        .replace("<|begin▁of▁thinking|>", "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::tools::register_builtin_tools;
    use crate::plugin::registry::ToolRegistry;
    use crate::runtime::store::AuthorizedWorkspaceRef;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn test_build_precompute_sandbox_with_authorized_workspace() {
        let workspace = PathBuf::from("/tmp/lotus");
        let aw = AuthorizedWorkspaceRef {
            id: "test".to_string(),
            root_path: PathBuf::from("/tmp/test_data"),
            display_name: "test".to_string(),
        };
        let config = build_precompute_sandbox(&workspace, Some(&aw));
        assert!(config.allowed_read_paths.contains(&PathBuf::from("/tmp/test_data")));
        assert!(!config.allowed_write_paths.contains(&PathBuf::from("/tmp/test_data")));
    }

    #[test]
    fn test_build_precompute_sandbox_without_authorized_workspace() {
        let workspace = PathBuf::from("/tmp/lotus");
        let config = build_precompute_sandbox(&workspace, None);
        assert_eq!(config.allowed_read_paths.len(), 7);
        assert!(!config.allowed_read_paths.iter().any(|p| p == &PathBuf::from("/tmp/test_data")));
    }

    #[tokio::test]
    async fn test_build_visible_tool_defs_with_authorized_workspace() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;

        let defs = build_visible_tool_defs(&registry, true).await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        for tool_name in WORKSPACE_TOOL_NAMES {
            assert!(
                names.contains(tool_name),
                "workspace tool '{}' should be visible when authorized, got {:?}",
                tool_name,
                names
            );
        }
    }

    #[tokio::test]
    async fn test_build_visible_tool_defs_without_authorized_workspace() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;

        let defs = build_visible_tool_defs(&registry, false).await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        for tool_name in WORKSPACE_TOOL_NAMES {
            assert!(
                !names.contains(tool_name),
                "workspace tool '{}' should be hidden without authorization, got {:?}",
                tool_name,
                names
            );
        }

        assert!(
            names.contains(&"execute_python"),
            "non-workspace tools should remain visible without authorization"
        );
    }

    #[test]
    fn test_build_workspace_context_with_authorized_workspace_prefers_workspace_tools() {
        let authorized = AuthorizedWorkspaceRef {
            id: "aw-1".to_string(),
            root_path: PathBuf::from("/tmp/customer-data"),
            display_name: "customer-data".to_string(),
        };

        let ctx = build_workspace_context(Some(&authorized));

        assert!(ctx.contains("[已连接本地目录]"));
        assert!(ctx.contains("customer-data"));
        assert!(ctx.contains("list_directory / read_workspace_file / search_files / get_file_info"));
        assert!(ctx.contains("不需要先上传或复制文件"));
        assert!(ctx.contains("只有处理用户上传的附件时，才使用 load_file"));
    }

    #[test]
    fn test_build_workspace_context_without_authorization_is_empty() {
        assert!(build_workspace_context(None).is_empty());
    }

    #[test]
    fn test_build_llm_content_with_authorized_workspace_scopes_load_file_to_uploads() {
        let attachments = vec![json!({
            "id": "file-1",
            "originalName": "sales.xlsx",
            "fileType": "xlsx"
        })];

        let content = build_llm_content("请分析这个目录里的销售数据", &attachments, true);

        assert!(content.contains("[已上传文件]"));
        assert!(content.contains("对这些已上传文件请调用 load_file(file_id)"));
        assert!(content.contains("对已连接本地目录请优先使用 list_directory / read_workspace_file / search_files / get_file_info"));
        assert!(!content.contains("提示：先调用 load_file(file_id) 加载文件数据"));
    }
}
