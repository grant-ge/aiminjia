pub mod auth;
pub mod commands;
pub mod connector;
pub mod llm;
pub mod models;
pub mod plugin;
pub mod runtime;
pub mod runtime_audit;
pub mod search;
pub mod storage;
pub mod telemetry;
pub mod transport;

use commands::chat;
use commands::file;
use commands::settings;
use commands::workspace;
use std::sync::Arc;
use storage::UserScopedPathResolver;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Keep the legacy app data dir only as migration input; runtime data lives in ~/.renlijia/.
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;

            let aijia_home = Arc::new(storage::AiJiaHome::from_home());
            aijia_home
                .ensure_dirs()
                .expect("Failed to create ~/.renlijia dirs");
            aijia_home
                .ensure_global_dirs()
                .expect("Failed to create global dirs");
            telemetry::set_diagnostics_workspace(aijia_home.root().to_path_buf());
            commands::file::cleanup_workspace_clipboard_staging(&aijia_home.tmp_clipboard_dir(), 7);
            if let Err(e) = storage::migration::migrate_if_needed(&app_data_dir, aijia_home.root())
            {
                log::warn!("[setup] migration warning (non-fatal): {}", e);
            }
            if let Err(e) = storage::migration::reconcile_legacy_conversations_if_needed(
                &app_data_dir,
                aijia_home.root(),
            ) {
                log::warn!(
                    "[setup] legacy conversation migration warning (non-fatal): {}",
                    e
                );
            }
            if let Err(e) = storage::migration::migrate_message_shards_to_single_file_if_needed(
                aijia_home.root(),
            ) {
                log::warn!("[setup] message shard migration warning (non-fatal): {}", e);
            }
            app.manage(aijia_home.clone());
            let runtime_paths = runtime::dependencies::RuntimePaths::new(
                aijia_home.runtimes_dir(),
                "renlijia-primary-runtime",
            )
            .expect("Failed to initialize managed runtime paths");
            let platform = runtime::dependencies::RuntimePlatform::current()
                .expect("Failed to identify managed runtime platform");
            let manifest_url = runtime::dependencies::configured_runtime_manifest_url();
            let runtime_manager: runtime::dependencies::ManagedRuntimeManager = Arc::new(
                runtime::dependencies::RuntimeManager::new(
                    runtime_paths.clone(),
                    env!("CARGO_PKG_VERSION"),
                )
                .with_manifest_source(
                    runtime::dependencies::RuntimeManifestSource::Url(manifest_url),
                    "primary",
                    platform,
                ),
            );
            let runtime_resolver: runtime::dependencies::ManagedRuntimeResolver =
                runtime_manager.clone();
            {
                let runtime_manager = runtime_manager.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = runtime_manager.ensure_managed().await {
                        log::warn!("[runtime] background ensure failed: {}", error);
                    }
                });
            }
            app.manage(runtime_manager.clone());
            app.manage(runtime_resolver.clone());

            // Initialize prompt store from external .md files
            let resource_dir = app
                .path()
                .resource_dir()
                .unwrap_or_else(|_| aijia_home.root().to_path_buf());
            llm::prompts::init_prompts(&resource_dir, aijia_home.root());

            // Initialize fallback root storage; user-scoped storage replaces it after auth restore.
            let root_db = Arc::new(
                storage::file_store::AppStorage::new(aijia_home.root())
                    .expect("Failed to initialize file storage"),
            );

            // Initialize file manager with root as default; will be updated after user scope activates.
            let file_mgr = Arc::new(storage::file_manager::FileManager::new(aijia_home.root()));

            // Configure logging — write to root logs/ initially (before user scope is known)
            let logs_dir = aijia_home.root().join("logs");
            std::fs::create_dir_all(&logs_dir).ok();
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                    .target(tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::Folder {
                            path: logs_dir.clone(),
                            file_name: Some("renlijia".into()),
                        },
                    ))
                    .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                    .max_file_size(5_000_000) // 5MB per file
                    .build(),
            )?;

            // Install global panic hook — captures panic message + backtrace into the log
            // file BEFORE the default abort handler runs (panic = "abort" in release).
            // Must be installed after the logger plugin is registered so log::error! writes
            // to the file.
            std::panic::set_hook(Box::new(|info| {
                let location = info
                    .location()
                    .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                    .unwrap_or_else(|| "unknown".to_string());
                let msg = info
                    .payload()
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| info.payload().downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic>".to_string());
                let bt = std::backtrace::Backtrace::force_capture();
                log::error!("[PANIC] at {}: {}\nBacktrace:\n{}", location, msg, bt);
                eprintln!("[PANIC] at {}: {}\nBacktrace:\n{}", location, msg, bt);
                // Brief sleep so async log writers can flush before the abort signal fires.
                std::thread::sleep(std::time::Duration::from_millis(200));
            }));

            // Auto-cleanup old log files (> 7 days)
            cleanup_old_logs(&logs_dir, 7);

            // Cleanup stale temp files from previous sessions (code_*.py)
            cleanup_temp_dir(&aijia_home.root().join("temp"));

            // Initialize secure storage for API key encryption
            let secure_storage: Option<Arc<storage::crypto::SecureStorage>> =
                match storage::crypto::SecureStorage::new(&aijia_home.crypto_dir()) {
                    Ok(ss) => {
                        log::info!("SecureStorage initialized (key file in ~/.renlijia/crypto)");
                        Some(Arc::new(ss))
                    }
                    Err(e) => {
                        log::warn!(
                            "SecureStorage unavailable (API keys stored as plaintext): {}",
                            e
                        );
                        None
                    }
                };

            // Initialize runtime orchestration state
            let run_registry = Arc::new(runtime::RuntimeRunRegistry::new());
            let task_store = Arc::new(runtime::store::InMemoryTaskStore::new());

            // Initialize cloud auth manager
            let global_store = Arc::new(storage::GlobalConfigStore::new(aijia_home.global_dir()));
            if let Err(e) = storage::migration_user_scope::bootstrap_cloud_auth_if_needed(
                aijia_home.root(),
                &aijia_home.global_dir(),
            ) {
                log::warn!("[setup] cloud_auth bootstrap warning: {}", e);
            }
            let auth_manager = Arc::new(auth::AuthManager::new(
                global_store.clone(),
                secure_storage.clone(),
            ));
            // Restore persisted auth state
            tauri::async_runtime::block_on(auth_manager.restore());

            let current_user_storage =
                Arc::new(storage::CurrentUserStorage::new(aijia_home.clone()));
            let user_scope: Option<storage::UserScope> = {
                let info = tauri::async_runtime::block_on(auth_manager.get_auth_info());
                if info.logged_in {
                    info.user
                        .as_ref()
                        .zip(info.tenant.as_ref())
                        .map(|(u, t)| storage::UserScope::new(t.id, u.id))
                } else {
                    None
                }
            };
            if let Some(ref scope) = user_scope {
                let user_dir = aijia_home.user_dir(scope);
                if let Err(e) =
                    storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
                        aijia_home.root(),
                        &user_dir,
                        &scope.key(),
                        &aijia_home.global_state_path(),
                    )
                {
                    log::warn!("[setup] user-scope migration warning: {}", e);
                }
                if let Err(e) = storage::migration_user_scope::migrate_legacy_config_if_needed(
                    aijia_home.root(),
                    &user_dir,
                    &aijia_home.global_dir(),
                ) {
                    log::warn!("[setup] config split warning: {}", e);
                }
                current_user_storage
                    .activate_scope(scope.clone())
                    .expect("Failed to activate user storage");

                // Update FileManager with user-scoped workspacePath
                let workspace_path = current_user_storage
                    .get()
                    .and_then(|db| db.get_setting("workspacePath").ok().flatten())
                    .unwrap_or_default();
                if !workspace_path.is_empty() {
                    let p = std::path::PathBuf::from(&workspace_path);
                    std::fs::create_dir_all(&p).ok();
                    file_mgr.update_workspace_path(&p);
                } else {
                    file_mgr.update_workspace_path(aijia_home.root());
                }
            }

            let (agent_store_path, subagent_transcript_store_dir) = current_user_storage
                .resolve_paths()
                .map(|paths| {
                    (
                        paths.agent_invocations_path(),
                        paths.subagent_transcripts_dir(),
                    )
                })
                .unwrap_or_else(|| {
                    (
                        aijia_home.agent_invocations_path(),
                        aijia_home.subagent_transcripts_dir(),
                    )
                });
            let agent_runtime = Arc::new(
                runtime::agent::AgentRuntime::from_storage(
                    agent_store_path,
                    subagent_transcript_store_dir,
                )
                .unwrap_or_else(|e| {
                    log::warn!(
                        "Failed to create FileAgentInvocationStore: {e}, falling back to in-memory"
                    );
                    runtime::agent::AgentRuntime::for_test()
                }),
            );

            let db = current_user_storage
                .get()
                .unwrap_or_else(|| root_db.clone());

            // Initialize LLM gateway (with auth_manager for cloud session_key injection)
            let gateway = Arc::new(
                llm::gateway::LlmGateway::new_with_registry(db.clone(), run_registry.clone())
                    .with_auth_manager(auth_manager.clone()),
            );

            // Set window title from persisted branding (before WebView renders)
            // Window setup: custom titlebar on all platforms
            {
                if let Some(win) = app.get_webview_window("main") {
                    // Title bar is rendered by HTML TitleBar component
                    let _ = win.set_title(" ");

                    // Windows: disable native decorations to avoid double titlebar.
                    // macOS uses titleBarStyle: Overlay (set in tauri.conf.json) which
                    // keeps the traffic light buttons overlaid on content.
                    #[cfg(target_os = "windows")]
                    {
                        let _ = win.set_decorations(false);
                    }
                }
            }

            // Initialize Playwright browser — primary browser automation
            let playwright_browser = Arc::new(
                connector::playwright_browser::PlaywrightBrowser::new(app.handle().clone()),
            );

            // Initialize connector engine (browser automation only)
            let connector_engine = Arc::new(connector::ConnectorEngine::new());
            tauri::async_runtime::block_on(async {
                connector_engine
                    .set_playwright_browser(playwright_browser.clone())
                    .await;
            });

            // Initialize DingTalk bridge (dws CLI sidecar)
            let dingtalk_bridge = Arc::new(connector::dingtalk::DingtalkBridge::new(
                app.handle().clone(),
            ));
            // Restore DingTalk auth status from dws persisted token (non-blocking)
            {
                let dt = dingtalk_bridge.clone();
                tauri::async_runtime::spawn(async move {
                    match dt.refresh_status().await {
                        Ok(info) if info.connected => {
                            log::info!(
                                "DingTalk: restored session — {} @ {}",
                                info.user_name.as_deref().unwrap_or("?"),
                                info.corp_name.as_deref().unwrap_or("?")
                            );
                        }
                        _ => log::info!("DingTalk: no active session"),
                    }
                });
            }

            // Initialize plugin registries
            let tool_registry = Arc::new(plugin::ToolRegistry::new());
            // Load SKILL.md-based skills from user and global roots
            let global_skills_dir = aijia_home.skills_dir();
            let user_skills_dir = current_user_storage
                .resolve_paths()
                .map(|paths| paths.skills_dir());
            let skill_roots: Vec<std::path::PathBuf> = match user_skills_dir {
                Some(user) => vec![user, global_skills_dir],
                None => vec![global_skills_dir],
            };
            let loaded_skills = plugin::skill::loader::load_skill_roots(&skill_roots)
                .unwrap_or_else(|e| {
                    log::warn!("[setup] Failed to load skills from roots: {}", e);
                    Default::default()
                });
            let disk_skill_registry = Arc::new(std::sync::Mutex::new(
                plugin::skill::registry::SkillRegistry::from_skills(
                    loaded_skills.into_values().collect(),
                ),
            ));
            app.manage(disk_skill_registry.clone());
            // Builtin skill sync moved to AuthGate post-login (see sync_builtin_skills command).
            // Persist the sync config so the IPC command can rebuild it.
            let global_skill_sync_config =
                plugin::skill::global_sync::GlobalSkillSyncConfig::for_home(
                    aijia_home.root(),
                    skill_roots.clone(),
                );
            app.manage(global_skill_sync_config);

            // Build agent registry: builtins + user-scope agents/*.md (if logged in).
            let user_agents_dir = current_user_storage
                .resolve_paths()
                .map(|paths| paths.agents_dir());
            let agent_registry = Arc::new(
                runtime::agent::registry_loader::load_registry_with_user_dir(
                    user_agents_dir.as_deref(),
                    None,
                ),
            );
            app.manage(agent_registry.clone());
            let async_agent_task_store = Arc::new(runtime::agent::async_task_store::AsyncAgentTaskStore::new());
            let task_notification_queue = Arc::new(runtime::agent::task_notification::TaskNotificationQueue::new());
            // Managed before TauriChatCommandAdapter::new() so the SessionRuntime can
            // pull it from app state and inject async sub-agent completion notices.
            app.manage(task_notification_queue.clone());

            let skill_registry = Arc::new(plugin::SkillRegistry::new("daily-assistant"));
            let permission_store = Arc::new(runtime::store::PermissionStore::with_layer_files(
                Some(
                    file_mgr
                        .workspace_path()
                        .join(".aijia")
                        .join("permissions.json"),
                ),
                current_user_storage
                    .resolve_paths()
                    .map(|paths| paths.permissions_path())
                    .or_else(|| Some(aijia_home.permissions_path())),
            ));
            tauri::async_runtime::block_on(
                tool_registry.set_permission_store(permission_store.clone()),
            );
            let mcp_server_manager =
                Arc::new(runtime::mcp::McpServerManager::new(tool_registry.clone()));
            let mcp_config_path = current_user_storage
                .resolve_paths()
                .map(|paths| paths.mcp_config_path())
                .unwrap_or_else(|| aijia_home.mcp_config_path());
            let mcp_config_store = Arc::new(storage::mcp_config_store::McpConfigStore::new(
                mcp_config_path,
            ));

            let persisted_mcp_configs = mcp_config_store.load().unwrap_or_else(|err| {
                log::warn!("Failed to load MCP configs from disk: {}", err);
                Vec::new()
            });

            tauri::async_runtime::block_on(async {
                for config in persisted_mcp_configs {
                    let connection = match runtime::mcp::build_mcp_connection(
                        &config,
                        Some(runtime_resolver.clone()),
                    ) {
                        Ok(connection) => connection,
                        Err(err) => {
                            log::warn!(
                                "Failed to build persisted MCP server '{}': {}",
                                config.name,
                                err
                            );
                            continue;
                        }
                    };
                    if let Err(err) = mcp_server_manager.register(connection).await {
                        log::warn!(
                            "Failed to pre-register persisted MCP server '{}': {}",
                            config.name,
                            err
                        );
                    }
                }
            });

            // Register builtin tools
            tauri::async_runtime::block_on(async {
                plugin::builtin::tools::register_builtin_tools(&tool_registry).await;
                // Note: builtin skills (daily_assistant) removed in Phase B Task 6.
                // SKILL.md disk loading implemented in Phase C/D via plugin::skill module.

                // Scan bundled plugin directory for external plugins
                let plugins_dir = resource_dir.join("plugins");
                log::info!(
                    "Scanning plugins from: {:?} (exists={})",
                    plugins_dir,
                    plugins_dir.exists()
                );
                if plugins_dir.exists() {
                    scan_external_plugins(
                        &plugins_dir,
                        &tool_registry,
                        &skill_registry,
                        &file_mgr.workspace_path(),
                        "builtin",
                    )
                    .await;
                }

                // Scan user-installed custom plugins
                if let Some(paths) = current_user_storage.resolve_paths() {
                    let skills_dir = paths.skills_dir();
                    if skills_dir.is_dir() {
                        log::info!("Scanning custom skills from: {:?}", skills_dir);
                        scan_external_plugins(
                            &skills_dir,
                            &tool_registry,
                            &skill_registry,
                            &file_mgr.workspace_path(),
                            "custom",
                        )
                        .await;
                    }
                }
            });

            log::info!("Plugin system initialized");

            // Crash recovery: clean up any tasks that were running when app crashed
            match db.cleanup_orphaned_tasks() {
                Ok(orphaned) => {
                    for conv_id in &orphaned {
                        log::warn!(
                            "Cleaning up orphaned agent task for conversation: {}",
                            conv_id
                        );
                    }
                    if !orphaned.is_empty() {
                        log::info!(
                            "Cleaned up {} orphaned agent tasks from previous crash",
                            orphaned.len()
                        );
                    }
                }
                Err(e) => {
                    log::warn!("Failed to cleanup orphaned tasks: {}", e);
                }
            }

            match storage::upload_gc::gc_orphan_upload_files(&db, &file_mgr) {
                Ok(deleted) => {
                    if deleted > 0 {
                        log::info!("Cleaned up {} orphaned upload files", deleted);
                    }
                }
                Err(err) => {
                    log::warn!("Failed to cleanup orphaned upload files: {}", err);
                }
            }

            match storage::skill_draft_store::gc_all_users(&aijia_home, 7) {
                Ok(removed) if removed > 0 => {
                    log::info!("Cleaned up {} stale skill drafts (>7d)", removed);
                }
                Ok(_) => {}
                Err(err) => {
                    log::warn!("Failed to gc skill drafts: {}", err);
                }
            }

            // Initialize runtime repository facade (routes settings/persona/file/export
            // commands through domain traits instead of direct AppStorage access).
            // IMPORTANT: facade must be managed before TauriChatCommandAdapter::new() is
            // called, because new() calls try_state::<RuntimeRepositoryFacade>() to wire
            // authorized_workspace_store. Registering it here ensures try_state succeeds.
            let facade = Arc::new(storage::file_store::RuntimeRepositoryFacade::from_storage(
                db.clone(),
            ));
            app.manage(facade);

            // Shared registry: ChannelManager worker inserts new session ids here;
            // IMAskCoordinator reads from it to decide if an event belongs to an IM session.
            // Created unconditionally before TauriChatCommandAdapter so the event adapter
            // can use it to suppress desktop-dialog forwarding for IM-channel sessions.
            let channel_session_ids: Arc<std::sync::RwLock<std::collections::HashSet<String>>> =
                Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()));

            let chat_adapter = Arc::new(
                transport::tauri_commands::chat::TauriChatCommandAdapter::new_with_channel_sessions(
                    current_user_storage.clone(),
                    root_db.clone(),
                    gateway.clone(),
                    file_mgr.clone(),
                    secure_storage.clone(),
                    tool_registry.clone(),
                    disk_skill_registry.clone(),
                    auth_manager.clone(),
                    permission_store.clone(),
                    app.handle().clone(),
                    Some(channel_session_ids.clone()
                        as Arc<dyn connector::channel::ask_coordinator::ChannelSessionRegistry>),
                ),
            );

            // Register managed state
            app.manage(db);
            app.manage(file_mgr);
            app.manage(gateway);
            app.manage(run_registry);
            app.manage(task_store);
            app.manage(secure_storage);
            app.manage(global_store);
            app.manage(current_user_storage.clone());
            app.manage(current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>);
            app.manage(auth_manager);
            app.manage(connector_engine);
            app.manage(dingtalk_bridge);
            app.manage(tool_registry);
            app.manage(mcp_server_manager);
            app.manage(mcp_config_store);
            app.manage(permission_store);
            app.manage(skill_registry);
            app.manage(agent_runtime);
            app.manage(chat_adapter);
            app.manage(async_agent_task_store);
            app.manage(std::sync::Arc::new(
                crate::runtime::employee::EmployeeActiveRuns::new(),
            ));

            // Initialize ChannelManager for IM channel integration
            if let Some(paths) = current_user_storage.resolve_paths() {
                let chat_adapter_ref = app
                    .state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                    .inner()
                    .clone();
                let gateway_ref = app
                    .state::<Arc<llm::gateway::LlmGateway>>()
                    .inner()
                    .clone();

                // reply_manager is shared between ChannelManager and the coordinator (as AskOutputSink)
                let reply_manager = Arc::new(connector::channel::DingtalkReplyManager::new());

                let judge = Arc::new(connector::channel::ask_coordinator::GatewayAskReplyJudge::new(
                    gateway_ref,
                    models::settings::AppSettings::default(),
                ));
                let ask_coordinator = Arc::new(
                    connector::channel::ask_coordinator::IMAskCoordinator::new(
                        channel_session_ids.clone()
                            as Arc<dyn connector::channel::ask_coordinator::ChannelSessionRegistry>,
                        reply_manager.clone()
                            as Arc<dyn connector::channel::ask_coordinator::AskOutputSink>,
                        chat_adapter_ref.permission_control_plane(),
                        chat_adapter_ref.interaction_control_plane(),
                        judge,
                    ),
                );

                let channel_manager = Arc::new(connector::channel::ChannelManager::new(
                    app.handle().clone(),
                    chat_adapter_ref,
                    app.state::<Arc<storage::file_store::RuntimeRepositoryFacade>>()
                        .inner()
                        .conversation_store_arc(),
                    app.state::<Option<Arc<storage::crypto::SecureStorage>>>()
                        .inner()
                        .clone(),
                    paths.channels_dir(),
                    Some(ask_coordinator),
                    reply_manager,
                    channel_session_ids,
                ));
                let cm = channel_manager.clone();
                tauri::async_runtime::spawn(async move {
                    cm.hydrate_conversations().await;
                    cm.auto_connect_if_configured().await;
                });
                app.manage(channel_manager);
            }

            runtime::schedule_runner::spawn_schedule_runner(
                current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>,
                app.state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                    .inner()
                    .clone(),
            );

            runtime::employee::runner::spawn_employee_scheduler(
                current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>,
                app.state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                    .inner()
                    .clone(),
            );

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Chat commands
            chat::send_message,
            chat::stop_streaming,
            chat::approve_permission_request,
            chat::deny_permission_request,
            chat::cancel_permission_request,
            chat::submit_user_interaction,
            chat::cancel_user_interaction,
            chat::get_messages,
            chat::get_subagent_transcript,
            chat::create_conversation,
            chat::get_conversation_model_override,
            chat::set_conversation_model_override,
            chat::delete_conversation,
            chat::rename_conversation,
            chat::archive_conversation,
            chat::restore_conversation,
            chat::get_archived_conversations,
            chat::get_conversations,
            chat::get_tasks,
            chat::is_agent_busy,
            // File commands
            file::upload_file,
            file::read_clipboard_file_paths,
            file::save_clipboard_image_to_tmp_dir,
            file::save_clipboard_image_to_workspace_staging,
            file::open_generated_file,
            file::reveal_file_in_folder,
            file::get_file_preview,
            file::get_local_file_preview,
            file::open_local_file,
            file::preview_file,
            file::delete_file,
            file::open_file_by_name,
            file::reveal_file_by_name,
            // Settings commands
            settings::get_settings,
            settings::update_settings,
            settings::validate_api_key,
            settings::get_configured_providers,
            settings::switch_provider,
            settings::get_all_provider_keys,
            settings::update_all_provider_keys,
            // Workspace commands
            workspace::select_workspace,
            workspace::get_workspace_info,
            workspace::pick_local_directory,
            workspace::open_logs_directory,
            workspace::open_workspace_directory,
            workspace::export_metrics,
            workspace::clear_metrics,
            workspace::get_metrics_info,
            workspace::record_frontend_diagnostic,
            workspace::authorize_local_directory,
            workspace::get_authorized_workspace,
            workspace::revoke_authorized_workspace,
            workspace::get_default_folder,
            commands::diagnostics::upload_diagnostic_logs,
            // Plugin commands
            commands::plugin::list_tools,
            commands::plugin::list_skills,
            commands::plugin::get_plugin_info,
            // Agent commands
            transport::tauri_commands::agents::list_agents,
            // MCP server management commands
            transport::tauri_commands::mcp::list_mcp_servers,
            transport::tauri_commands::mcp::add_mcp_server,
            transport::tauri_commands::mcp::remove_mcp_server,
            transport::tauri_commands::mcp::connect_mcp_server,
            transport::tauri_commands::mcp::disconnect_mcp_server,
            transport::tauri_commands::runtime::runtime_get_health,
            transport::tauri_commands::runtime::runtime_ensure,
            transport::tauri_commands::runtime::runtime_reinstall,
            transport::tauri_commands::runtime::runtime_cleanup_old_versions,
            transport::tauri_commands::runtime::runtime_cancel_operation,
            // Persona commands
            commands::persona::list_personas,
            commands::persona::get_persona,
            commands::persona::save_persona,
            commands::persona::delete_persona,
            commands::persona::set_active_persona,
            commands::persona::get_active_persona,
            commands::persona::export_personas,
            commands::persona::import_personas,
            // Project memory commands
            commands::project_memory::save_project_memory,
            commands::project_memory::distill_project_memory,
            // Schedule commands
            commands::schedules::list_schedules,
            commands::schedules::create_schedule,
            commands::schedules::delete_schedule,
            // Employee commands
            commands::employees::employee_list,
            commands::employees::employee_get,
            commands::employees::employee_create,
            commands::employees::employee_update,
            commands::employees::employee_delete,
            commands::employees::employee_restore,
            commands::employees::employee_purge,
            commands::employees::employee_trigger,
            commands::employees::employee_stop_run,
            commands::employees::employee_active_run,
            commands::employees::employee_index_knowledge_async,
            commands::employees::employee_template_catalog,
            commands::employees::employee_template_refresh,
            commands::employees::inbox_list,
            commands::employees::inbox_mark_read,
            commands::employees::inbox_mark_all_read,
            commands::employees::inbox_unread_count,
            // DingTalk commands
            commands::dingtalk::dingtalk_login,
            commands::dingtalk::dingtalk_logout,
            commands::dingtalk::dingtalk_status,
            commands::dingtalk::dingtalk_refresh_status,
            // Auth commands
            commands::auth::cloud_login,
            commands::auth::cloud_logout,
            commands::auth::get_cloud_auth,
            commands::auth::get_cloud_models,
            commands::auth::cloud_change_password,
            // Skill management commands
            commands::skill_management::list_custom_skills,
            commands::skill_management::install_custom_skill,
            commands::skill_management::uninstall_custom_skill,
            commands::skill_management::init_skill_template,
            commands::skill_management::pack_skill,
            // Skill-Smith (小程) draft commands
            commands::skill_draft::list_skill_drafts,
            commands::skill_draft::discard_skill_draft,
            commands::skill_draft::get_skill_draft_meta,
            commands::skill_draft::import_skill_package,
            commands::skill_draft::export_installed_skill,
            commands::skill_management::reload_skill,
            commands::skill_management::start_skill_watch,
            commands::skill_management::stop_skill_watch,
            crate::plugin::skill::sync_command::sync_builtin_skills,
            // Marketplace commands
            commands::skill_management::list_marketplace_skills,
            commands::skill_management::install_marketplace_skill,
            // Channel commands
            commands::channel::channel_get_platforms,
            commands::channel::channel_get_platform,
            commands::channel::channel_get_conversations,
            commands::channel::channel_begin_registration,
            commands::channel::channel_poll_registration,
            commands::channel::channel_set_enabled,
            commands::channel::channel_remove_platform,
            commands::channel::channel_reveal_secret,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(chat_adapter) = app_handle
                    .try_state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                {
                    if let Err(err) = chat_adapter.flush_pending_message_writes() {
                        log::warn!(
                            "Failed to flush pending assistant message writes on exit: {err}"
                        );
                    }
                }

                // Shutdown CDP browser (kill Chromium process) via connector engine
                let engine = app_handle.state::<Arc<connector::ConnectorEngine>>();
                tauri::async_runtime::block_on(engine.shutdown_cdp());
            }
        });
}

/// Scan a plugin directory for external plugins.
/// `source` identifies the origin: "builtin" for bundled, "custom" for user-installed.
async fn scan_external_plugins(
    _plugins_dir: &std::path::Path,
    _tool_registry: &plugin::ToolRegistry,
    _skill_registry: &plugin::SkillRegistry,
    _workspace_path: &std::path::Path,
    _source: &str,
) {
    // SKILL.md disk loading is implemented in Phase C/D via plugin::skill module.
    // This legacy entrypoint is intentionally a no-op.
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_external_plugins_is_intentional_noop() {
        // scan_external_plugins is a no-op in Phase B.
        // SKILL.md disk loading is implemented in Phase C/D via plugin::skill module.
        // This test documents the intentional state.
    }
}

/// Remove log files older than `retention_days` days from the logs directory.
fn cleanup_old_logs(logs_dir: &std::path::Path, retention_days: u64) {
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(retention_days * 86400));
    let cutoff = match cutoff {
        Some(c) => c,
        None => return,
    };

    let entries = match std::fs::read_dir(logs_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(meta) = path.metadata() {
            if let Ok(modified) = meta.modified() {
                if modified < cutoff {
                    if std::fs::remove_file(&path).is_ok() {
                        // Log may not be available yet during startup, use eprintln
                        eprintln!("Cleaned up old log file: {:?}", path);
                    }
                }
            }
        }
    }
}

/// Remove stale Python temp files (code_*.py) from the workspace temp directory.
///
/// These files are normally cleaned up after each execution, but if the app
/// crashes or is force-quit during Python execution, temp files may be left behind.
fn cleanup_temp_dir(temp_dir: &std::path::Path) {
    let entries = match std::fs::read_dir(temp_dir) {
        Ok(e) => e,
        Err(_) => return, // Directory doesn't exist yet — fine
    };

    let mut count = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("code_") && name.ends_with(".py") {
                    if std::fs::remove_file(&path).is_ok() {
                        count += 1;
                    }
                }
            }
        }
    }
    if count > 0 {
        eprintln!("Cleaned up {} stale temp Python files", count);
    }
}
