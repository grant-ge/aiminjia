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
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Keep the legacy app data dir only as migration input; runtime data lives in ~/.renlijia/.
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;

            let aijia_home = Arc::new(storage::AiJiaHome::from_home());
            aijia_home
                .ensure_dirs()
                .expect("Failed to create ~/.renlijia dirs");
            if let Err(e) = storage::migration::migrate_if_needed(&app_data_dir, aijia_home.root()) {
                log::warn!("[setup] migration warning (non-fatal): {}", e);
            }
            app.manage(aijia_home.clone());

            // Initialize prompt store from external .md files
            let resource_dir = app
                .path()
                .resource_dir()
                .unwrap_or_else(|_| aijia_home.root().to_path_buf());
            llm::prompts::init_prompts(&resource_dir, aijia_home.root());

            // Initialize file-based storage
            let db = Arc::new(
                storage::file_store::AppStorage::new(aijia_home.root())
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
                let default_ws = aijia_home.root().to_path_buf();
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
            let agent_store_path = aijia_home.agent_invocations_path();
            let subagent_transcript_store_dir = aijia_home.subagent_transcripts_dir();
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

            // Initialize cloud auth manager
            let auth_manager = Arc::new(auth::AuthManager::new(db.clone(), secure_storage.clone()));
            // Restore persisted auth state
            tauri::async_runtime::block_on(auth_manager.restore());

            // Initialize LLM gateway (with auth_manager for cloud session_key injection)
            let gateway = Arc::new(
                llm::gateway::LlmGateway::new_with_registry(db.clone(), run_registry.clone())
                    .with_auth_manager(auth_manager.clone()),
            );

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
            let permission_store = Arc::new(runtime::store::PermissionStore::with_layer_files(
                Some(
                    file_mgr
                        .workspace_path()
                        .join(".aijia")
                        .join("permissions.json"),
                ),
                Some(aijia_home.permissions_path()),
            ));
            tauri::async_runtime::block_on(
                tool_registry.set_permission_store(permission_store.clone()),
            );
            let mcp_server_manager =
                Arc::new(runtime::mcp::McpServerManager::new(tool_registry.clone()));
            let mcp_config_store = Arc::new(storage::mcp_config_store::McpConfigStore::new(
                aijia_home.mcp_config_path(),
            ));

            let persisted_mcp_configs = mcp_config_store.load().unwrap_or_else(|err| {
                log::warn!("Failed to load MCP configs from disk: {}", err);
                Vec::new()
            });

            tauri::async_runtime::block_on(async {
                for config in persisted_mcp_configs {
                    let connection = match runtime::mcp::build_mcp_connection(&config) {
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

            // Register builtin tools and skills
            tauri::async_runtime::block_on(async {
                plugin::builtin::tools::register_builtin_tools(&tool_registry).await;
                plugin::builtin::skills::register_builtin_skills(
                    &skill_registry,
                    db.clone(),
                    auth_manager.clone(),
                    None,
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
                let skills_dir = aijia_home.skills_dir();
                if skills_dir.is_dir() {
                    log::info!("Scanning custom skills from: {:?}", skills_dir);
                    scan_external_plugins(
                        &skills_dir,
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

            // Initialize runtime repository facade (routes settings/persona/file/export
            // commands through domain traits instead of direct AppStorage access).
            // IMPORTANT: facade must be managed before TauriChatCommandAdapter::new() is
            // called, because new() calls try_state::<RuntimeRepositoryFacade>() to wire
            // authorized_workspace_store. Registering it here ensures try_state succeeds.
            let facade = Arc::new(storage::file_store::RuntimeRepositoryFacade::from_storage(
                db.clone(),
            ));
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
                    skill_registry.clone(),
                    session_mgr.clone(),
                    auth_manager.clone(),
                    permission_store.clone(),
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
            app.manage(task_store);
            app.manage(secure_storage);
            app.manage(auth_manager);
            app.manage(connector_engine);
            app.manage(tool_registry);
            app.manage(mcp_server_manager);
            app.manage(mcp_config_store);
            app.manage(permission_store);
            app.manage(skill_registry);
            app.manage(session_mgr);
            app.manage(agent_runtime);
            app.manage(chat_adapter);

            runtime::schedule_runner::spawn_schedule_runner(
                aijia_home.clone(),
                app.state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                    .inner()
                    .clone(),
            );

            // Skill-smith: cleanup expired drafts on startup (non-blocking).
            // Draft files older than 7 days are removed to keep _drafts/ tidy.
            let cleanup_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match commands::skill_smith::cleanup_expired_drafts(cleanup_handle).await {
                    Ok(n) if n > 0 => log::info!("skill-smith: cleaned up {} expired draft(s)", n),
                    Ok(_) => {}
                    Err(e) => log::warn!("skill-smith: draft cleanup failed: {}", e),
                }
            });

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
            chat::get_archived_conversations,
            chat::get_conversations,
            chat::get_tasks,
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
            workspace::get_default_folder,
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
            // Project memory commands
            commands::project_memory::save_project_memory,
            commands::project_memory::distill_project_memory,
            // Schedule commands
            commands::schedules::list_schedules,
            commands::schedules::create_schedule,
            commands::schedules::delete_schedule,
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
            // Skill-smith (conversational skill creation) — draft file system (T2)
            commands::skill_smith::create_skill_draft,
            commands::skill_smith::write_skill_draft_file,
            commands::skill_smith::read_skill_draft_file,
            commands::skill_smith::list_skill_draft_files,
            commands::skill_smith::list_skill_drafts,
            commands::skill_smith::discard_skill_draft,
            commands::skill_smith::cleanup_expired_drafts,
            // Skill-smith schema validation (T3)
            commands::skill_smith::validation::validate_skill_draft,
            // Skill-smith commit + export (T4)
            commands::skill_smith::commit::commit_skill_draft,
            commands::skill_smith::commit::commit_skill_draft_force,
            commands::skill_smith::commit::export_skill_draft,
            // Skill-smith dry-run (T7)
            commands::skill_smith::dry_run::dry_run_skill_draft,
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

        let manifest = match plugin::manifest::read_manifest_from_skill_dir(&path) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Invalid skill manifest in {:?}: {}", path, e);
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
                        .register(std::sync::Arc::new(skill), source)
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
                log::warn!("Unknown plugin type '{}' in {:?}", other, path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scan_external_plugins;
    use crate::plugin::{SkillRegistry, ToolRegistry};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_scan_external_plugins_keeps_source_label_for_skill_registration() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("md-skill");
        std::fs::create_dir_all(plugin_dir.join("prompts")).unwrap();
        std::fs::write(
            plugin_dir.join("SKILL.md"),
            r#"---
id: "md-skill"
name: "Markdown Skill"
description: "desc"
keywords:
  - "分析"
include_app_base: false
---
# Markdown Skill
"#,
        )
        .unwrap();
        std::fs::write(plugin_dir.join("prompts/base.md"), "base prompt").unwrap();

        let storage = Arc::new(
            crate::storage::file_store::AppStorage::new(tmp.path()).expect("test storage"),
        );
        let tool_registry = ToolRegistry::new();
        let skill_registry = SkillRegistry::new("daily-assistant");
        skill_registry
            .register(
                Arc::new(crate::plugin::builtin::skills::daily_assistant::DailyAssistantSkill::new(
                    storage.clone(),
                    Arc::new(crate::auth::AuthManager::new(storage.clone(), None)),
                )),
                "builtin",
            )
            .await;

        scan_external_plugins(
            tmp.path(),
            &tool_registry,
            &skill_registry,
            tmp.path(),
            "custom",
        )
        .await;

        let skills = skill_registry.list().await;
        let md_skill = skills
            .into_iter()
            .find(|skill| skill.id == "md-skill")
            .expect("SKILL.md-only directory should be scanned and registered");
        assert_eq!(md_skill.source, "custom");
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
