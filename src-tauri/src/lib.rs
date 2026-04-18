pub mod auth;
pub mod commands;
pub mod connector;
pub mod llm;
pub mod models;
pub mod plugin;
pub mod python;
pub mod runtime;
pub mod runtime_audit;
pub mod search;
pub mod storage;
pub mod telemetry;
pub mod transport;

use commands::chat;
use commands::export;
use commands::file;
use commands::settings;
use commands::workspace;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Initialize app data directory
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let app_config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| app_data_dir.clone());
            std::fs::create_dir_all(&app_config_dir)?;

            // Initialize prompt store from external .md files
            let resource_dir = app
                .path()
                .resource_dir()
                .unwrap_or_else(|_| app_data_dir.clone());
            llm::prompts::init_prompts(&resource_dir, &app_data_dir);

            // Initialize file-based storage
            let db = Arc::new(
                storage::file_store::AppStorage::new(&app_data_dir)
                    .expect("Failed to initialize file storage"),
            );

            // Initialize file manager
            let workspace_path = db
                .get_setting("workspacePath")
                .ok()
                .flatten()
                .unwrap_or_default();
            let fm_path = if workspace_path.is_empty() {
                // Default workspace: ~/.renlijia
                let default_ws = dirs::home_dir()
                    .map(|h| h.join(".renlijia"))
                    .unwrap_or_else(|| app_data_dir.clone());
                std::fs::create_dir_all(&default_ws).ok();
                default_ws
            } else {
                let p = std::path::PathBuf::from(&workspace_path);
                std::fs::create_dir_all(&p).ok();
                p
            };
            let file_mgr = Arc::new(storage::file_manager::FileManager::new(fm_path.clone()));

            // Configure logging — write to workspace/logs/ for both debug and release
            let logs_dir = fm_path.join("logs");
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

            // Auto-cleanup old log files (> 7 days)
            cleanup_old_logs(&logs_dir, 7);

            // Cleanup stale temp files from previous sessions (code_*.py)
            cleanup_temp_dir(&fm_path.join("temp"));

            // Initialize secure storage for API key encryption
            let secure_storage: Option<Arc<storage::crypto::SecureStorage>> =
                match storage::crypto::SecureStorage::new(&app_data_dir) {
                    Ok(ss) => {
                        log::info!("SecureStorage initialized (key file in app data dir)");
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
            let agent_store_path = app_data_dir.join("agent_invocations.json");
            let subagent_transcript_store_dir = app_data_dir.join("subagent_transcripts");
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

            // Initialize LLM gateway
            let gateway = Arc::new(llm::gateway::LlmGateway::new_with_registry(
                db.clone(),
                run_registry.clone(),
            ));

            // Initialize cloud auth manager
            let auth_manager = Arc::new(auth::AuthManager::new(db.clone(), secure_storage.clone()));
            // Restore persisted auth state
            tauri::async_runtime::block_on(auth_manager.restore());

            // Set window title from persisted branding (before WebView renders)
            {
                // Title bar is rendered by HTML TitleBar component (titleBarStyle: Overlay)
                // Set native window title to empty to avoid duplicate text
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_title(" ");
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

            // Initialize plugin registries
            let tool_registry = Arc::new(plugin::ToolRegistry::new());
            let skill_registry = Arc::new(plugin::SkillRegistry::new("daily-assistant"));
            let mcp_server_manager = Arc::new(runtime::mcp::McpServerManager::new(
                tool_registry.clone(),
            ));
            let mcp_config_store = Arc::new(storage::mcp_config_store::McpConfigStore::new(
                app_config_dir.join("mcp_servers.json"),
            ));

            let persisted_mcp_configs = mcp_config_store.load().unwrap_or_else(|err| {
                log::warn!("Failed to load MCP configs from disk: {}", err);
                Vec::new()
            });

            tauri::async_runtime::block_on(async {
                for config in persisted_mcp_configs {
                    if let Err(err) = mcp_server_manager
                        .register(Arc::new(runtime::mcp::PendingMcpConnection::new(
                            config.clone(),
                        )))
                        .await
                    {
                        log::warn!(
                            "Failed to pre-register persisted MCP server '{}': {}",
                            config.name,
                            err
                        );
                    }
                }
            });

            // Register builtin tools and skills
            tauri::async_runtime::block_on(async {
                plugin::builtin::tools::register_builtin_tools(&tool_registry).await;
                plugin::builtin::skills::register_builtin_skills(
                    &skill_registry,
                    db.clone(),
                    auth_manager.clone(),
                )
                .await;

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
                        file_mgr.workspace_path(),
                        "builtin",
                    )
                    .await;
                }

                // Scan user-installed custom plugins
                let custom_plugins_dir = app_data_dir.join("custom_plugins");
                if custom_plugins_dir.is_dir() {
                    log::info!("Scanning custom plugins from: {:?}", custom_plugins_dir);
                    scan_external_plugins(
                        &custom_plugins_dir,
                        &tool_registry,
                        &skill_registry,
                        file_mgr.workspace_path(),
                        "custom",
                    )
                    .await;
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
                        db.reset_stuck_analysis_state(conv_id).ok();
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

            // Initialize runtime repository facade (routes settings/persona/file/export
            // commands through domain traits instead of direct AppStorage access).
            // IMPORTANT: facade must be managed before TauriChatCommandAdapter::new() is
            // called, because new() calls try_state::<RuntimeRepositoryFacade>() to wire
            // authorized_workspace_store. Registering it here ensures try_state succeeds.
            let facade = Arc::new(
                storage::file_store::RuntimeRepositoryFacade::from_storage(db.clone()),
            );
            app.manage(facade);

            // Initialize Python session manager for persistent REPL sessions
            let session_mgr = Arc::new(python::session::PythonSessionManager::new(
                fm_path.clone(),
                Some(app.handle()),
            ));

            let chat_adapter = Arc::new(
                transport::tauri_commands::chat::TauriChatCommandAdapter::new(
                    db.clone(),
                    gateway.clone(),
                    file_mgr.clone(),
                    secure_storage.clone(),
                    tool_registry.clone(),
                    session_mgr.clone(),
                    auth_manager.clone(),
                    app.handle().clone(),
                ),
            );

            // Start idle session reaper (every 5 minutes)
            {
                let session_mgr_clone = session_mgr.clone();
                tauri::async_runtime::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                    loop {
                        interval.tick().await;
                        session_mgr_clone.reap_idle().await;
                    }
                });
            }

            // Register managed state
            app.manage(db);
            app.manage(file_mgr);
            app.manage(gateway);
            app.manage(run_registry);
            app.manage(secure_storage);
            app.manage(auth_manager);
            app.manage(connector_engine);
            app.manage(tool_registry);
            app.manage(mcp_server_manager);
            app.manage(mcp_config_store);
            app.manage(skill_registry);
            app.manage(session_mgr);
            app.manage(agent_runtime);
            app.manage(chat_adapter);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Chat commands
            chat::send_message,
            chat::stop_streaming,
            chat::approve_permission_request,
            chat::deny_permission_request,
            chat::cancel_permission_request,
            chat::get_messages,
            chat::create_conversation,
            chat::delete_conversation,
            chat::rename_conversation,
            chat::get_conversations,
            chat::is_agent_busy,
            // File commands
            file::upload_file,
            file::open_generated_file,
            file::reveal_file_in_folder,
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
            workspace::authorize_local_directory,
            workspace::get_authorized_workspace,
            workspace::revoke_authorized_workspace,
            // Export commands
            export::export_conversation,
            // Plugin commands
            commands::plugin::list_tools,
            commands::plugin::list_skills,
            commands::plugin::get_plugin_info,
            // MCP server management commands
            transport::tauri_commands::mcp::list_mcp_servers,
            transport::tauri_commands::mcp::add_mcp_server,
            transport::tauri_commands::mcp::remove_mcp_server,
            transport::tauri_commands::mcp::connect_mcp_server,
            transport::tauri_commands::mcp::disconnect_mcp_server,
            // Persona commands
            commands::persona::list_personas,
            commands::persona::get_persona,
            commands::persona::save_persona,
            commands::persona::delete_persona,
            commands::persona::set_active_persona,
            commands::persona::get_active_persona,
            commands::persona::export_personas,
            commands::persona::import_personas,
            // Auth commands
            commands::auth::cloud_login,
            commands::auth::cloud_logout,
            commands::auth::get_cloud_auth,
            commands::auth::get_cloud_models,
            commands::auth::cloud_change_password,
            commands::auth::get_branding,
            // Skill management commands
            commands::skill_management::list_custom_skills,
            commands::skill_management::install_custom_skill,
            commands::skill_management::uninstall_custom_skill,
            commands::skill_management::init_skill_template,
            commands::skill_management::pack_skill,
            commands::skill_management::reload_skill,
            commands::skill_management::start_skill_watch,
            commands::skill_management::stop_skill_watch,
            // Marketplace commands
            commands::skill_management::list_marketplace_skills,
            commands::skill_management::install_marketplace_skill,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Graceful shutdown: checkpoint all Python sessions before exit.
                // block_on is safe here — the event loop is already shutting down.
                let session_mgr = app_handle.state::<Arc<python::session::PythonSessionManager>>();
                tauri::async_runtime::block_on(session_mgr.shutdown_all());

                // Shutdown CDP browser (kill Chromium process) via connector engine
                let engine = app_handle.state::<Arc<connector::ConnectorEngine>>();
                tauri::async_runtime::block_on(engine.shutdown_cdp());
            }
        });
}

/// Scan a plugin directory for external plugins.
/// `source` identifies the origin: "builtin" for bundled, "custom" for user-installed.
async fn scan_external_plugins(
    plugins_dir: &std::path::Path,
    tool_registry: &plugin::ToolRegistry,
    skill_registry: &plugin::SkillRegistry,
    workspace_path: &std::path::Path,
    source: &str,
) {
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("Failed to read plugins directory: {}", e);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Skip directories starting with '_' (disabled plugins)
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('_') {
                continue;
            }
        }

        let manifest_path = path.join("plugin.toml");
        if !manifest_path.exists() {
            continue;
        }

        let manifest_content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to read {:?}: {}", manifest_path, e);
                continue;
            }
        };

        let manifest = match plugin::manifest::parse_plugin_manifest(&manifest_content) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Invalid plugin.toml in {:?}: {}", path, e);
                continue;
            }
        };

        match manifest.plugin.plugin_type.as_str() {
            "tool" => {
                if manifest.plugin.runtime.as_deref() == Some("python") {
                    match plugin::python_bridge::PythonToolBridge::from_manifest(
                        &manifest,
                        path.clone(),
                    ) {
                        Ok(mut bridge) => {
                            if let Err(e) = bridge.load_schema(workspace_path).await {
                                log::warn!(
                                    "Failed to load schema for plugin '{}': {}",
                                    manifest.plugin.id,
                                    e
                                );
                                continue;
                            }
                            tool_registry
                                .register(std::sync::Arc::new(bridge), source)
                                .await;
                            log::info!("Loaded Python tool plugin: {}", manifest.plugin.id);
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to create Python tool bridge for '{}': {}",
                                manifest.plugin.id,
                                e
                            );
                        }
                    }
                }
            }
            "skill" => match plugin::declarative_skill::DeclarativeSkill::load(&manifest, &path) {
                Ok(skill) => {
                    skill_registry
                        .register(std::sync::Arc::new(skill), "plugin")
                        .await;
                    log::info!("Loaded declarative skill plugin: {}", manifest.plugin.id);
                }
                Err(e) => {
                    log::warn!(
                        "Failed to load skill plugin '{}': {}",
                        manifest.plugin.id,
                        e
                    );
                }
            },
            other => {
                log::warn!("Unknown plugin type '{}' in {:?}", other, manifest_path);
            }
        }
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
