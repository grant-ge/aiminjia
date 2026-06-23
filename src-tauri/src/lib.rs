pub mod auth;
pub mod cli;
pub mod commands;
pub mod connector;
pub mod environment;
pub mod llm;
pub mod log_context;
pub mod log_level;
pub mod models;
pub mod plugin;
pub mod runtime;
pub mod runtime_audit;
pub mod search;
pub mod storage;
pub mod telemetry;
pub mod tracing_setup;
pub mod transport;
pub mod updater;

use commands::chat;
use commands::file;
use commands::settings;
use commands::workspace;
use std::sync::Arc;
use storage::UserScopedPathResolver;
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

const APP_LOG_RETENTION_DAYS: u64 = 3;
const NAVIGATION_MENU_EVENT: &str = "navigation:menu-command";
const NAVIGATION_BACK_MENU_ID: &str = "navigation.back";
const NAVIGATION_FORWARD_MENU_ID: &str = "navigation.forward";
const TRAY_ICON_ID: &str = "main";
const TRAY_SHOW_MENU_ID: &str = "tray.show";
const TRAY_HIDE_MENU_ID: &str = "tray.hide";
const TRAY_QUIT_MENU_ID: &str = "tray.quit";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AppMenuLabels {
    file: &'static str,
    edit: &'static str,
    view: &'static str,
    window: &'static str,
    help: &'static str,
    navigation: &'static str,
    back: &'static str,
    forward: &'static str,
    tray_tooltip: &'static str,
    tray_show: &'static str,
    tray_hide: &'static str,
    tray_quit: &'static str,
}

fn app_menu_labels(language: &str) -> AppMenuLabels {
    if language.to_ascii_lowercase().starts_with("en") {
        AppMenuLabels {
            file: "File",
            edit: "Edit",
            view: "View",
            window: "Window",
            help: "Help",
            navigation: "Navigation",
            back: "Back",
            forward: "Forward",
            tray_tooltip: "AIjia",
            tray_show: "Show AIjia",
            tray_hide: "Hide to Tray",
            tray_quit: "Quit AIjia",
        }
    } else {
        AppMenuLabels {
            file: "文件",
            edit: "编辑",
            view: "显示",
            window: "窗口",
            help: "帮助",
            navigation: "导航",
            back: "后退",
            forward: "前进",
            tray_tooltip: "AI 小家",
            tray_show: "显示主窗口",
            tray_hide: "隐藏到托盘",
            tray_quit: "退出 AI 小家",
        }
    }
}

fn app_about_metadata<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> AboutMetadata<'static> {
    let pkg_info = app_handle.package_info();
    let config = app_handle.config();
    AboutMetadata {
        name: Some(pkg_info.name.clone()),
        version: Some(pkg_info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config
            .bundle
            .publisher
            .clone()
            .map(|publisher| vec![publisher]),
        ..Default::default()
    }
}

fn handle_window_close_requested<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    event: &tauri::WindowEvent,
) {
    if window.label() != "main" {
        return;
    }

    #[cfg(target_os = "macos")]
    {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Err(err) = window.hide() {
                log::warn!("Failed to hide main window on macOS close request: {err}");
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Err(err) = window.hide() {
                log::warn!("Failed to hide main window on Windows close request: {err}");
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = event;
}

fn build_localized_app_menu<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    language: &str,
) -> tauri::Result<Menu<R>> {
    let labels = app_menu_labels(language);
    let pkg_info = app_handle.package_info();
    let about_metadata = app_about_metadata(app_handle);

    let navigation_menu = SubmenuBuilder::new(app_handle, labels.navigation)
        .text(NAVIGATION_BACK_MENU_ID, labels.back)
        .text(NAVIGATION_FORWARD_MENU_ID, labels.forward)
        .build()?;

    let window_menu = Submenu::with_id_and_items(
        app_handle,
        tauri::menu::WINDOW_SUBMENU_ID,
        labels.window,
        true,
        &[
            &PredefinedMenuItem::minimize(app_handle, None)?,
            &PredefinedMenuItem::maximize(app_handle, None)?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app_handle)?,
            &PredefinedMenuItem::close_window(app_handle, None)?,
        ],
    )?;

    let help_menu = Submenu::with_id_and_items(
        app_handle,
        tauri::menu::HELP_SUBMENU_ID,
        labels.help,
        true,
        &[
            #[cfg(not(target_os = "macos"))]
            &PredefinedMenuItem::about(app_handle, None, Some(about_metadata))?,
        ],
    )?;

    Menu::with_items(
        app_handle,
        &[
            #[cfg(target_os = "macos")]
            &Submenu::with_items(
                app_handle,
                pkg_info.name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app_handle, None, Some(about_metadata))?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::services(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::hide(app_handle, None)?,
                    &PredefinedMenuItem::hide_others(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::quit(app_handle, None)?,
                ],
            )?,
            #[cfg(not(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            )))]
            &Submenu::with_items(
                app_handle,
                labels.file,
                true,
                &[
                    &PredefinedMenuItem::close_window(app_handle, None)?,
                    #[cfg(not(target_os = "macos"))]
                    &PredefinedMenuItem::quit(app_handle, None)?,
                ],
            )?,
            &Submenu::with_items(
                app_handle,
                labels.edit,
                true,
                &[
                    &PredefinedMenuItem::undo(app_handle, None)?,
                    &PredefinedMenuItem::redo(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::cut(app_handle, None)?,
                    &PredefinedMenuItem::copy(app_handle, None)?,
                    &PredefinedMenuItem::paste(app_handle, None)?,
                    &PredefinedMenuItem::select_all(app_handle, None)?,
                ],
            )?,
            #[cfg(target_os = "macos")]
            &Submenu::with_items(
                app_handle,
                labels.view,
                true,
                &[&PredefinedMenuItem::fullscreen(app_handle, None)?],
            )?,
            &window_menu,
            &help_menu,
            &navigation_menu,
        ],
    )
}

fn set_app_navigation_menu<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    language: &str,
) -> tauri::Result<()> {
    let menu = build_localized_app_menu(app_handle, language)?;
    app_handle.set_menu(menu)?;
    Ok(())
}

fn install_app_navigation_menu<R: tauri::Runtime>(app: &mut tauri::App<R>) -> tauri::Result<()> {
    set_app_navigation_menu(app.handle(), "zh-CN")?;
    app.on_menu_event(|app_handle, event| match event.id().0.as_str() {
        NAVIGATION_BACK_MENU_ID => {
            if let Err(err) = app_handle.emit(NAVIGATION_MENU_EVENT, "back") {
                log::warn!("[app-menu] failed to emit navigation back event: {err}");
            }
        }
        NAVIGATION_FORWARD_MENU_ID => {
            if let Err(err) = app_handle.emit(NAVIGATION_MENU_EVENT, "forward") {
                log::warn!("[app-menu] failed to emit navigation forward event: {err}");
            }
        }
        _ => {}
    });
    Ok(())
}

#[tauri::command]
fn set_app_menu_language(app: tauri::AppHandle, language: String) -> Result<(), String> {
    set_app_navigation_menu(&app, &language).map_err(|err| err.to_string())?;
    set_app_tray_language(&app, &language).map_err(|err| err.to_string())
}

fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn hide_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn toggle_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    if visible && !minimized {
        let _ = window.hide();
    } else {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray_menu<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    labels: AppMenuLabels,
) -> tauri::Result<Menu<R>> {
    Menu::with_items(
        app_handle,
        &[
            &MenuItem::with_id(
                app_handle,
                TRAY_SHOW_MENU_ID,
                labels.tray_show,
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(
                app_handle,
                TRAY_HIDE_MENU_ID,
                labels.tray_hide,
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app_handle)?,
            &MenuItem::with_id(
                app_handle,
                TRAY_QUIT_MENU_ID,
                labels.tray_quit,
                true,
                None::<&str>,
            )?,
        ],
    )
}

fn set_app_tray_language<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    language: &str,
) -> tauri::Result<()> {
    let labels = app_menu_labels(language);
    if let Some(tray) = app_handle.tray_by_id(TRAY_ICON_ID) {
        let menu = build_tray_menu(app_handle, labels)?;
        tray.set_menu(Some(menu))?;
        tray.set_tooltip(Some(labels.tray_tooltip))?;
    }
    Ok(())
}

fn install_app_tray<R: tauri::Runtime>(app: &mut tauri::App<R>) -> tauri::Result<()> {
    let app_handle = app.handle().clone();
    let labels = app_menu_labels("zh-CN");
    let menu = build_tray_menu(&app_handle, labels)?;
    let icon = app_handle
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default_window_icon".into()))?;

    TrayIconBuilder::with_id(TRAY_ICON_ID)
        .icon(icon)
        .tooltip(labels.tray_tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_SHOW_MENU_ID => show_main_window(app),
            TRAY_HIDE_MENU_ID => hide_main_window(app),
            TRAY_QUIT_MENU_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        })
        .build(&app_handle)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Initialize tracing BEFORE the Tauri builder so we call log::set_logger()
    // first — Tauri's devtools feature also tries to set a global logger and
    // would otherwise win the race, causing SetLoggerError on LogTracer::init().
    {
        let renlijia_root = dirs::home_dir().unwrap_or_default().join(".renlijia");
        let logs_dir = renlijia_root.join("logs");
        std::fs::create_dir_all(&logs_dir).ok();
        crate::tracing_setup::init(&logs_dir);
        let level = crate::log_level::read_persisted(&renlijia_root.join("global"));
        log::set_max_level(crate::log_level::to_log_filter(&level));
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init());

    #[cfg(feature = "e2e")]
    let builder = builder.plugin(tauri_plugin_pilot::init());

    builder
        .on_window_event(|window, event| {
            handle_window_close_requested(window, event);
        })
        .setup(|app| {
            install_app_navigation_menu(app)?;
            install_app_tray(app)?;

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
            let global_store = Arc::new(storage::GlobalConfigStore::new(aijia_home.global_dir()));
            telemetry::set_diagnostics_workspace(aijia_home.root().to_path_buf());
            commands::file::cleanup_workspace_clipboard_staging(&aijia_home.tmp_clipboard_dir(), 7);
            // GC expired legacy-root archives (30d retention).  Independent
            // of login state because the dirs live at the data root, not
            // inside a user scope.
            if let Err(e) =
                storage::migration_root_cleanup::cleanup_legacy_archive_if_expired(aijia_home.root())
            {
                log::warn!("[setup] legacy-root archive GC warning: {}", e);
            }
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

            // Bundled runtime: shipped inside the installer at
            // `<resource_dir>/runtime/<platform>/{node,python,uv}/...`.
            // Populated at build time by `scripts/prepare-bundled-runtime.{sh,ps1}`.
            // When available, this avoids the OSS download on first launch entirely.
            let resource_dir = app
                .path()
                .resource_dir()
                .unwrap_or_else(|_| aijia_home.root().to_path_buf());
            let bundled_resolver =
                runtime::dependencies::BundledRuntimeResolver::new(resource_dir.clone());
            let bundled_version = bundled_resolver.bundled_version();
            log::info!(
                "[runtime] bundled resolver mounted at {} (version={:?})",
                resource_dir.display(),
                bundled_version
            );

            let runtime_manager: runtime::dependencies::ManagedRuntimeManager = Arc::new(
                runtime::dependencies::RuntimeManager::new(
                    runtime_paths.clone(),
                    env!("CARGO_PKG_VERSION"),
                )
                .with_bundled_fallback(bundled_resolver.clone())
                .with_manifest_source(
                    runtime::dependencies::RuntimeManifestSource::Url(manifest_url),
                    "primary",
                    platform,
                ),
            );
            let runtime_resolver: runtime::dependencies::ManagedRuntimeResolver =
                runtime_manager.clone();

            let runtime_manager_bg = runtime_manager.clone();
            tauri::async_runtime::spawn(async move {
                match runtime_manager_bg.ensure_managed().await {
                    Ok(result) if result.skipped => {
                        log::info!(
                            "[runtime] cache runtime ready; ensure skipped version={}",
                            result.bundle_version
                        );
                    }
                    Ok(result) => {
                        log::info!(
                            "[runtime] cache runtime initialized version={} path={}",
                            result.bundle_version,
                            result.install_dir.display()
                        );
                    }
                    Err(error) => {
                        log::warn!("[runtime] background ensure failed: {}", error);
                    }
                }
            });

            app.manage(runtime_manager.clone());
            app.manage(runtime_resolver.clone());

            // Initialize prompt store from external .md files
            llm::prompts::init_prompts(&resource_dir, aijia_home.root());

            // Initialize fallback root storage; user-scoped storage replaces it after auth restore.
            let root_db = Arc::new(
                storage::file_store::AppStorage::new(aijia_home.root())
                    .expect("Failed to initialize file storage"),
            );

            // Initialize file manager with root as default; will be updated after user scope activates.
            let file_mgr = Arc::new(storage::file_manager::FileManager::new(aijia_home.root()));

            // Logging was initialized in run() before the Tauri builder.
            // Retain the variable for cleanup below.
            let logs_dir = aijia_home.root().join("logs");

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

            // Auto-cleanup old local logs. Desktop logs can grow quickly when
            // stream diagnostics are enabled, so keep only the recent window.
            cleanup_old_logs(&logs_dir, APP_LOG_RETENTION_DAYS);

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
            // Dev-only: seed the gateway-host override from persisted config so
            // auth/LLM paths (which have no app handle) see it. No-op / not
            // compiled in release builds.
            #[cfg(debug_assertions)]
            {
                let persisted = global_store
                    .get_setting(environment::dev::CONFIG_KEY)
                    .ok()
                    .flatten();
                environment::dev::load(persisted.as_deref());
            }
            // Data-layout compatibility gate: if the on-disk layout predates a
            // breaking storage / encryption change, the legacy `cloud_auth`
            // blob is purged so the user is forced to re-login cleanly.  Must
            // run BEFORE `bootstrap_cloud_auth_if_needed`, otherwise that
            // function would copy the stale ciphertext from
            // `~/.renlijia/config.json` back into the new location.
            storage::data_version::ensure_compatible(
                aijia_home.as_ref(),
                secure_storage.as_deref(),
            );
            if let Err(e) = storage::migration_user_scope::bootstrap_cloud_auth_if_needed(
                aijia_home.root(),
                &aijia_home.global_dir(),
            ) {
                log::warn!("[setup] cloud_auth bootstrap warning: {}", e);
            }
            let auth_manager = Arc::new(auth::AuthManager::new(
                global_store.clone(),
                secure_storage.clone(),
                aijia_home.as_ref(),
            ));
            // Restore persisted auth state
            tauri::async_runtime::block_on(auth_manager.restore());

            let current_user_storage =
                Arc::new(storage::CurrentUserStorage::new(aijia_home.clone()));
            let skill_enablement_store = Arc::new(
                plugin::skill::enablement::SkillEnablementStore::new(
                    current_user_storage.clone(),
                ),
            );
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
                if let Err(e) = storage::migration_user_scope::migrate_legacy_turn_stages_if_needed(
                    aijia_home.root(),
                    &user_dir,
                ) {
                    log::warn!("[setup] turn-stages migration warning: {}", e);
                }
                // Archive legacy root copies after the user-scope migration
                // grace window (24h).  Runs every startup; the function
                // self-skips if already done or grace not elapsed.
                if let Err(e) = storage::migration_root_cleanup::cleanup_legacy_root_if_claimed(
                    aijia_home.root(),
                    &aijia_home.global_state_path(),
                ) {
                    log::warn!("[setup] legacy-root cleanup warning: {}", e);
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

            #[allow(deprecated)]
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
                    // Title bar is rendered by the HTML TitleBar component, but the
                    // native window title still drives the macOS Dock right-click
                    // menu, Mission Control and Cmd+Tab window list — a blank " "
                    // there shows up as a nameless entry. Seed it with the product
                    // name; brandingStore refines it to the tenant productName once
                    // the webview loads.
                    let native_title = app
                        .config()
                        .product_name
                        .clone()
                        .filter(|value| !value.trim().is_empty())
                        .or_else(|| std::env::var("AIJIA_DEV_APP_NAME").ok())
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "AIjia".to_string());
                    let _ = win.set_title(&native_title);

                    // macOS: `titleBarStyle: Overlay` floats the traffic lights over
                    // content but does NOT hide the native title text — it would
                    // otherwise show "AIjia" right of the lights. Hide it so the
                    // TitleBar React component is the only thing in that strip.
                    commands::window_chrome::hide_window_title(&win);

                    // Windows: disable native decorations to avoid double titlebar.
                    // macOS uses titleBarStyle: Overlay (set in tauri.conf.json) which
                    // keeps the traffic light buttons overlaid on content.
                    #[cfg(target_os = "windows")]
                    {
                        let _ = win.set_decorations(false);
                    }
                }
            }

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
            // Load SKILL.md-based skills from both the per-user root and
            // the restricted global root. Dual-root is intentional:
            //
            // - `~/.renlijia/skills/` (global) is only for required platform
            //   builtins. Marketplace and tenant skills must be explicitly
            //   installed by the user before they appear in the runtime
            //   SkillRegistry.
            // - `~/.renlijia/users/{scope}/skills/` holds user-imported
            //   SKILL packages (`install_custom_skill`). Per-account so
            //   account switches don't leak.
            //
            // User-facing delete normally operates on the user root; legacy
            // global orphans are pruned/handled by the global sync path.
            let global_skills_dir = aijia_home.skills_dir();
            let user_skills_dir = current_user_storage
                .resolve_paths()
                .map(|paths| paths.skills_dir());
            // (root, source) pairs — explicit so we don't rely on positional
            // index-0=User convention.
            if let Err(error) =
                plugin::skill::global_sync::prune_non_required_global_skill_installs(
                    &aijia_home.global_state_path(),
                    &global_skills_dir,
                )
            {
                log::warn!(
                    "[setup] prune legacy marketplace global skills failed: {}",
                    error
                );
            }
            let skill_roots_tagged: Vec<(std::path::PathBuf, plugin::skill::types::SkillSource)> =
                match user_skills_dir {
                    Some(user) => vec![
                        (user, plugin::skill::types::SkillSource::User),
                        (global_skills_dir.clone(), plugin::skill::types::SkillSource::Global),
                    ],
                    None => vec![(global_skills_dir.clone(), plugin::skill::types::SkillSource::Global)],
                };
            let skill_roots: Vec<std::path::PathBuf> =
                skill_roots_tagged.iter().map(|(p, _)| p.clone()).collect();
            let loaded_skills = plugin::skill::loader::load_skill_roots_tagged(&skill_roots_tagged)
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

            // Build agent registry: builtins + user-scope agents/*.md (if logged in)
            // + dynamic projection of Active Employees.
            let user_agents_dir = current_user_storage
                .resolve_paths()
                .map(|paths| paths.agents_dir());
            let agent_registry = Arc::new(
                runtime::agent::registry_loader::load_registry_with_user_dir(
                    user_agents_dir.as_deref(),
                    None,
                ),
            );

            // EmployeeStore as app singleton: lib.rs creates one Arc, seeds AgentRegistry,
            // wires the sync hook; all Tauri commands pull this same Arc from app state.
            // Without singletization, set_sync below would only affect this transient
            // boot instance — every Tauri command that does EmployeeStore::new(...) would
            // miss the hook and AgentRegistry would drift.
            let employee_store_arc: Option<Arc<runtime::employee::store::EmployeeStore>> =
                current_user_storage.resolve_paths().map(|paths| {
                    Arc::new(runtime::employee::store::EmployeeStore::new(
                        paths.employees_dir(),
                    ))
                });

            if let Some(emp_store) = employee_store_arc.as_ref() {
                match emp_store.list() {
                    Ok(records) => {
                        let n = runtime::agent::employee_projection::seed_registry_from_employees(
                            &agent_registry,
                            &records,
                        );
                        log::info!(
                            "[agent-registry] seeded {} active employees into AgentRegistry",
                            n
                        );
                    }
                    Err(e) => log::warn!("[agent-registry] employee seed failed: {e}"),
                }
                let sync = Arc::new(runtime::agent::employee_projection::AgentRegistrySync {
                    registry: agent_registry.clone(),
                });
                emp_store.set_sync(sync);
            }

            app.manage(agent_registry.clone());
            if let Some(store) = employee_store_arc {
                app.manage(store);
            }
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

            // Initialize runtime repository facade (routes settings/persona/file/export
            // commands through domain traits instead of direct AppStorage access).
            // IMPORTANT: facade must be managed before TauriChatCommandAdapter::new() is
            // called, because new() calls try_state::<RuntimeRepositoryFacade>() to wire
            // authorized_workspace_store. Registering it here ensures try_state succeeds.
            let facade = Arc::new(storage::file_store::RuntimeRepositoryFacade::from_storage_with_cus(
                db.clone(),
                Some(current_user_storage.clone()),
            ));
            app.manage(facade);

            // LTR: managed BEFORE TauriChatCommandAdapter::new() (line ~525)
            // because the adapter's `try_state::<Arc<TeamRegistry>>()` runs
            // inline during construction and must see these registries.
            // Without this ordering, LTR tools (TeamCreate / SendMessage /
            // TaskClaim / TeammateStop) all panic with
            // "team_registry not injected into ToolExecutionContext".
            app.manage(runtime::agent::TeamRegistry::new());
            app.manage(runtime::agent::AgentNameRegistry::new());
            app.manage(runtime::agent::InboxRegistry::new());
            app.manage(runtime::agent::LeadIdleSupervisor::new());
            app.manage(runtime::agent::CancellationRegistry::new());

            // Shared registry: ChannelManager worker inserts new session ids here;
            // IMAskCoordinator reads from it to decide if an event belongs to an IM session.
            // Created unconditionally before TauriChatCommandAdapter so the event adapter
            // can use it to suppress desktop-dialog forwarding for IM-channel sessions.
            let channel_session_ids: Arc<std::sync::RwLock<std::collections::HashSet<String>>> =
                Arc::new(std::sync::RwLock::new(std::collections::HashSet::new()));
            let output_binding_registry =
                Arc::new(runtime::human_interaction::RunOutputBindingRegistry::new());
            let im_app_feedback =
                connector::im::shared::app_feedback::IMAppFeedbackCoordinator::new();
            app.manage(output_binding_registry);

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
                        as Arc<dyn connector::im::ask_coordinator::ChannelSessionRegistry>),
                )
                .with_im_app_feedback(im_app_feedback.clone()),
            );
            // LTR P2 follow-up: wire Path C wake (continuation turn triggered by
            // teammate SendMessage) AFTER the adapter is in an Arc, so the wake
            // closure can upgrade a Weak<Self> and reuse `send_chat_request` —
            // the same code path user-driven `send_message` uses, which
            // constructs a per-request ToolDispatcher with all services.
            //
            // This replaces the previous `runtime.wire_lead_idle_wake_path()`
            // call: that route ran continuation turns through the base
            // SessionRuntime whose QueryEngine had no ToolDispatcher, so every
            // tool call inside a continuation turn errored with
            // "tool dispatcher not configured" and polluted messages.jsonl.
            chat_adapter.wire_path_c_wake_to_self();
            chat_adapter.wire_task_notification_wake_to_self();

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
            app.manage(skill_enablement_store);
            app.manage(auth_manager);
            app.manage(dingtalk_bridge);
            app.manage(tool_registry);
            app.manage(mcp_server_manager);
            app.manage(mcp_config_store);
            app.manage(permission_store);
            app.manage(skill_registry);
            app.manage(agent_runtime);
            app.manage(chat_adapter);
            app.manage(im_app_feedback);
            app.manage(async_agent_task_store);
            app.manage(std::sync::Arc::new(
                crate::runtime::employee::EmployeeActiveRuns::new(),
            ));

            // PendingQueueManager: per-session queue with debounced drain.
            // Must be created after RuntimeRunRegistry, RuntimeRepositoryFacade, and
            // TauriRuntimeHost (via app handle). Requires an active user scope; if the
            // user is not logged in at startup, the manager is still registered but
            // uses the legacy root conversations dir as a best-effort fallback.
            {
                let pending_event_bus = crate::runtime::RuntimeEventBus::new();
                let pending_host = std::sync::Arc::new(
                    crate::transport::tauri_runtime_host::TauriRuntimeHost::new(app.handle().clone()),
                );
                let pending_adapter: std::sync::Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber> = std::sync::Arc::new(
                    crate::transport::tauri_event_adapter::TauriEventAdapter::new(pending_host),
                );
                pending_event_bus.subscribe(pending_adapter.clone());
                // Anchor the subscriber so the Weak ref in the bus stays alive.
                app.manage(pending_adapter);

                let run_registry_ref = app
                    .state::<std::sync::Arc<crate::runtime::RuntimeRunRegistry>>()
                    .inner()
                    .clone();

                // Resolve pending paths from CurrentUserStorage dynamically so
                // same-process account switches cannot keep writing to the
                // scope that happened to be active at app startup.
                let pending_resolver: std::sync::Arc<dyn crate::runtime::pending::ConvDirResolver> =
                    std::sync::Arc::new(crate::runtime::pending::CurrentUserPendingResolver::new(
                        (*aijia_home).clone(),
                        current_user_storage.clone(),
                    ));

                let pending_manager = crate::runtime::pending::PendingQueueManager::new(
                    run_registry_ref,
                    std::sync::Arc::new(pending_event_bus),
                    pending_resolver,
                    crate::runtime::pending::PendingConfig::default(),
                );
                {
                    let mgr_for_restore = pending_manager.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = mgr_for_restore.restore_from_disk().await {
                            log::warn!("[pending] restore_from_disk failed: {:#}", e);
                        }
                    });
                }
                // Wire the chat adapter as the drain dispatcher: when the
                // debounce timer fires, the manager calls TauriChatCommandAdapter
                // ::dispatch which delegates to send_chat_request.
                {
                    let chat_adapter_for_pending: Arc<
                        dyn crate::runtime::pending::ChatTurnDispatcher,
                    > = app
                        .state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                        .inner()
                        .clone();
                    let mgr_for_dispatcher = pending_manager.clone();
                    tauri::async_runtime::spawn(async move {
                        mgr_for_dispatcher
                            .set_dispatcher(chat_adapter_for_pending)
                            .await;
                    });
                }
                {
                    let chat_adapter_for_events = app
                        .state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                        .inner()
                        .clone();
                    chat_adapter_for_events.subscribe_event_listener(
                        pending_manager.clone()
                            as Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>,
                    );
                }
                app.manage(pending_manager);
            }

            // Slot is registered unconditionally — actual instance is installed by
            // ensure_channel_manager_registered() either now (if logged in at boot)
            // or later via cloud_login.
            app.manage(Arc::new(connector::im::ChannelManagerSlot::new()));
            app.manage(channel_session_ids);

            // Boot-time bring-up: only effective when current_user_storage already
            // has a scope (restored from disk in setup above).
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    ensure_channel_manager_registered(&handle).await;
                });
            }

            // Register deactivation handlers — fire on logout / change_password / auto-401.
            {
                let slot_ref = app
                    .state::<Arc<connector::im::ChannelManagerSlot>>()
                    .inner()
                    .clone();
                let cus_ref = app
                    .state::<Arc<storage::CurrentUserStorage>>()
                    .inner()
                    .clone();
                let file_mgr_ref = app
                    .state::<Arc<storage::file_manager::FileManager>>()
                    .inner()
                    .clone();
                let pending_ref = app
                    .state::<Arc<crate::runtime::pending::PendingQueueManager>>()
                    .inner()
                    .clone();
                let home_ref = aijia_home.clone();
                let am = app.state::<Arc<auth::AuthManager>>().inner().clone();
                tauri::async_runtime::block_on(async {
                    am.register_deactivation_handler(Arc::new(ChannelManagerDeactivator {
                        slot: slot_ref,
                    }))
                    .await;
                    am.register_deactivation_handler(Arc::new(CurrentUserStorageDeactivator {
                        cus: cus_ref,
                    }))
                    .await;
                    am.register_deactivation_handler(Arc::new(FileManagerWorkspaceResetter {
                        file_mgr: file_mgr_ref,
                        home: home_ref,
                    }))
                    .await;
                    am.register_deactivation_handler(pending_ref)
                        .await;
                    am.register_revoked_handler(Arc::new(AuthExpiredEmitter {
                        app: app.handle().clone(),
                    }))
                    .await;
                });
            }

            runtime::agenda::spawn_agenda_runner(
                current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>,
                app.state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                    .inner()
                    .clone() as Arc<dyn runtime::agenda::AgendaRunDispatcher>,
            );

            runtime::employee::runner::spawn_employee_scheduler(
                app.try_state::<Arc<runtime::employee::store::EmployeeStore>>()
                    .map(|s| s.inner().clone()),
                current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>,
                app.state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                    .inner()
                    .clone(),
            );

            // Background diagnostic auto-upload: ships only bytes after the
            // persisted watermark so each tick stays small. Initial run is
            // delayed 30s after startup to let auth restore + first-screen
            // render settle, then 15min cadence afterwards. Failures are
            // logged at warn-level and do not block the loop.
            {
                let auth = app.state::<Arc<auth::AuthManager>>().inner().clone();
                let home = aijia_home.clone();
                let fmgr = app.state::<Arc<storage::file_manager::FileManager>>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    match commands::diagnostics::upload_incremental(&auth, &home, &fmgr, "startup").await {
                        Ok(n) if n > 0 => log::info!("[diag-auto] startup uploaded {n} chunks"),
                        Ok(_) => log::debug!("[diag-auto] startup: nothing to upload"),
                        Err(e) => log::warn!("[diag-auto] startup upload failed: {e}"),
                    }
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
                    // After laptop sleep / suspend, default `Burst` mode fires
                    // every missed tick back-to-back — that's how a single
                    // expired-key event turns into 80+ retries in one second.
                    // Skip missed ticks instead so we resume on the next
                    // natural cadence.
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    tick.tick().await; // skip the immediate first tick
                    loop {
                        tick.tick().await;
                        match commands::diagnostics::upload_incremental(&auth, &home, &fmgr, "periodic").await {
                            Ok(n) if n > 0 => log::info!("[diag-auto] periodic uploaded {n} chunks"),
                            Ok(_) => log::debug!("[diag-auto] periodic: nothing to upload"),
                            Err(e) => {
                                log::warn!("[diag-auto] periodic upload failed: {e}");
                                // Auth / 401 errors should not be hammered each
                                // tick — back off 1 hour to give the user time
                                // to relogin or for the gateway to recover.
                                if e.contains("401") || e.contains("auth_error") {
                                    log::warn!(
                                        "[diag-auto] auth error detected — backing off 1h before next attempt"
                                    );
                                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                                }
                            }
                        }
                    }
                });
            }

            // --- Network probe (spec docs/superpowers/specs/2026-05-26-network-detection-design.md) ---
            {
                let probe_host: Arc<dyn crate::transport::runtime_host::RuntimeHost> = Arc::new(
                    crate::transport::tauri_runtime_host::TauriRuntimeHost::new(app.handle().clone()),
                );
                let network_probe = crate::runtime::network::probe::NetworkProbe::new(probe_host);
                app.manage(network_probe.clone());
                // Spawn on Tauri's managed runtime — Tauri's `setup()` runs on the
                // main thread, not inside a tokio runtime, so `tokio::spawn` would
                // panic with "there is no reactor running". `tauri::async_runtime`
                // wraps the runtime Tauri itself uses.
                tauri::async_runtime::spawn(network_probe.run());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Chat commands
            chat::send_message,
            chat::compact_conversation,
            chat::stop_streaming,
            chat::approve_permission_request,
            chat::deny_permission_request,
            chat::cancel_permission_request,
            chat::pending_permission_snapshot_for_session,
            chat::pending_interaction_snapshot_for_session,
            chat::submit_user_interaction,
            chat::cancel_user_interaction,
            chat::get_messages,
            chat::get_subagent_transcript,
            chat::get_team_overview,
            chat::get_teammate_transcript,
            chat::team_chat_messages,
            chat::create_conversation,
            chat::delete_conversation,
            chat::rename_conversation,
            chat::set_conversation_expert_team,
            chat::get_conversation_meta,
            chat::archive_conversation,
            chat::restore_conversation,
            chat::get_archived_conversations,
            chat::set_conversation_pinned,
            chat::set_conversation_expert_team,
            chat::clear_conversation_source,
            chat::get_conversation_source,
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
            file::is_generated_file_available,
            file::is_local_file_available,
            file::save_generated_file_as,
            file::get_file_preview,
            file::get_local_file_preview,
            file::open_local_file,
            file::save_local_file_as,
            file::preview_file,
            file::delete_file,
            file::open_file_by_name,
            file::reveal_file_by_name,
            transport::tauri_commands::conversation_export::export_conversation,
            transport::tauri_commands::conversation_export::reveal_export_in_folder,
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
            // Log level commands
            transport::tauri_commands::logging::get_log_level,
            transport::tauri_commands::logging::set_log_level,
            // Plugin commands
            commands::plugin::list_tools,
            commands::plugin::get_visible_tools_for_current_request,
            commands::plugin::list_skills,
            commands::plugin::get_skill_detail,
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
            transport::tauri_commands::runtime::runtime_diagnostics,
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
            // Agenda commands
            transport::tauri_commands::agenda::list_agenda_items,
            transport::tauri_commands::agenda::create_agenda_item,
            transport::tauri_commands::agenda::get_agenda_item,
            transport::tauri_commands::agenda::update_agenda_item,
            transport::tauri_commands::agenda::delete_agenda_item,
            transport::tauri_commands::agenda::cancel_agenda_item,
            transport::tauri_commands::agenda::restore_agenda_item,
            transport::tauri_commands::agenda::run_agenda_item_now,
            transport::tauri_commands::agenda::list_agenda_occurrences,
            transport::tauri_commands::agenda::skip_occurrence,
            transport::tauri_commands::agenda::unskip_occurrence,
            // Employee commands
            commands::employees::employee_list,
            commands::employees::employee_get,
            commands::employees::employee_create,
            commands::employees::employee_update,
            commands::employees::employee_delete,
            commands::employees::employee_trigger,
            commands::employees::employee_template_check_upgrade,
            commands::employees::employee_template_upgrade,
            commands::employees::employee_stop_run,
            commands::employees::employee_active_run,
            commands::employees::employee_index_knowledge_async,
            commands::employees::employee_template_catalog,
            commands::employees::employee_template_refresh,
            commands::expert_team_templates::expert_team_template_catalog,
            commands::expert_team_templates::expert_team_template_refresh,
            commands::workplace_directory::workplace_directory_catalog,
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
            commands::auth::cloud_send_sms_code,
            commands::auth::cloud_send_email_code,
            commands::auth::cloud_register,
            commands::auth::cloud_reset_password,
            commands::auth::get_last_brand,
            // Billing/account usage commands
            crate::transport::tauri_commands::billing::billing_summary,
            crate::transport::tauri_commands::billing::billing_usage_records,
            commands::auth::save_last_brand,
            // Skill management commands
            commands::skill_management::list_custom_skills,
            commands::skill_management::install_custom_skill,
            commands::skill_management::uninstall_custom_skill,
            commands::skill_management::init_skill_template,
            commands::skill_management::pack_skill,
            commands::skill_management::refresh_skill_registry_cmd,
            commands::skill_management::set_skill_enabled,
            // Skill package import/export (drag-drop zip / SkillCard export)
            commands::skill_draft::import_skill_package,
            commands::skill_draft::export_installed_skill,
            crate::plugin::skill::sync_command::sync_builtin_skills,
            // Marketplace commands
            commands::skill_management::list_marketplace_skills,
            commands::skill_management::preview_marketplace_skill,
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
            commands::channel::channel_send_dingtalk_greeting,
            commands::channel::channel_wecom_save,
            commands::channel::channel_wecom_test_connection,
            commands::channel::channel_wecom_remove,
            commands::channel::channel_wecom_set_enabled,
            commands::channel::channel_wecom_begin_registration,
            commands::channel::channel_wecom_poll_registration,
            commands::channel::channel_telegram_save,
            commands::channel::channel_telegram_remove,
            commands::channel::channel_telegram_set_enabled,
            commands::channel::channel_telegram_begin_pairing,
            commands::channel::channel_telegram_list_pending_pairings,
            commands::channel::channel_telegram_approve_pairing,
            commands::channel::channel_telegram_reject_pairing,
            commands::channel::channel_telegram_revoke_user,
            commands::channel::channel_telegram_list_paired_users,
            commands::channel::channel_whatsapp_update_allow_from,
            commands::channel::channel_whatsapp_get_allow_from,
            // Pending queue commands
            crate::transport::tauri_commands::pending::pending_snapshot_for_session,
            crate::transport::tauri_commands::pending::pending_remove_item,
            // Turn-stage persistence (spec 2026-05-17-turn-stages §5)
            crate::transport::tauri_commands::turn_stage::get_active_turn_stage,
            crate::transport::tauri_commands::turn_stage::clear_active_turn_stage,
            // Updater cache/download commands
            updater::commands::updater_check_cache,
            updater::commands::updater_download,
            updater::commands::updater_read_cached_bytes,
            updater::commands::updater_clear_cache,
            updater::commands::updater_install_cached,
            updater::commands::updater_platform_key,
            // Network status commands (spec docs/superpowers/specs/2026-05-26-network-detection-design.md)
            transport::tauri_commands::network::network_get_status,
            transport::tauri_commands::network::network_force_probe,
            set_app_menu_language,
            // Dev-only environment switcher (not compiled in release builds)
            #[cfg(debug_assertions)]
            commands::dev_environment::get_dev_environment,
            #[cfg(debug_assertions)]
            commands::dev_environment::set_dev_environment,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                if !has_visible_windows {
                    if let Some(win) = app_handle.get_webview_window("main") {
                        if let Err(err) = win.show() {
                            log::warn!("Failed to show main window on macOS Dock reopen: {err}");
                        }
                        if let Err(err) = win.set_focus() {
                            log::warn!("Failed to focus main window on macOS Dock reopen: {err}");
                        }
                    }
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(chat_adapter) = app_handle
                    .try_state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
                {
                    if let Err(err) = chat_adapter.flush_pending_message_writes() {
                        log::warn!(
                            "Failed to flush pending assistant message writes on exit: {err}"
                        );
                    }
                }
                // LTR (P1.8): drop all per-session Team / name bindings on
                // app close.  Teammate idle loops self-clean as their inbox
                // senders are dropped; we just need to clear the registry
                // tables so a relaunch starts with a fresh slate.
                if let (Some(team_reg), Some(name_reg)) = (
                    app_handle
                        .try_state::<Arc<runtime::agent::TeamRegistry>>()
                        .map(|s| s.inner().clone()),
                    app_handle
                        .try_state::<Arc<runtime::agent::AgentNameRegistry>>()
                        .map(|s| s.inner().clone()),
                ) {
                    let inbox_reg = app_handle
                        .try_state::<Arc<runtime::agent::InboxRegistry>>()
                        .map(|s| s.inner().clone());
                    let cancel_reg = app_handle
                        .try_state::<Arc<runtime::agent::CancellationRegistry>>()
                        .map(|s| s.inner().clone());
                    tauri::async_runtime::block_on(async move {
                        team_reg.clear_all().await;
                        name_reg.clear_all().await;
                        if let Some(reg) = inbox_reg {
                            reg.clear_all().await;
                        }
                        if let Some(reg) = cancel_reg {
                            reg.clear_all().await;
                        }
                    });
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod app_menu_tests {
    use super::*;

    #[test]
    fn localized_app_menu_preserves_standard_edit_actions() {
        let source = include_str!("lib.rs");
        let function_start = source
            .find("fn build_localized_app_menu")
            .expect("build_localized_app_menu should exist");
        let setup_start = source
            .find("fn install_app_navigation_menu")
            .expect("install_app_navigation_menu should exist");
        let menu_setup = &source[function_start..setup_start];

        assert!(
            menu_setup.contains("PredefinedMenuItem::copy")
                && menu_setup.contains("PredefinedMenuItem::paste")
                && menu_setup.contains("PredefinedMenuItem::cut")
                && menu_setup.contains("PredefinedMenuItem::undo")
                && menu_setup.contains("PredefinedMenuItem::redo")
                && menu_setup.contains("PredefinedMenuItem::select_all"),
            "localized Edit menu must keep Tauri predefined items so Cmd+C/V/X/Z/A continue to work"
        );
        assert!(
            menu_setup.contains("labels.file")
                && menu_setup.contains("labels.edit")
                && menu_setup.contains("labels.view")
                && menu_setup.contains("labels.window")
                && menu_setup.contains("labels.help")
                && menu_setup.contains("labels.navigation"),
            "all visible top-level menu labels should come from the app language"
        );
    }

    #[test]
    fn app_menu_labels_follow_supported_app_languages() {
        assert_eq!(
            app_menu_labels("en-US"),
            AppMenuLabels {
                file: "File",
                edit: "Edit",
                view: "View",
                window: "Window",
                help: "Help",
                navigation: "Navigation",
                back: "Back",
                forward: "Forward",
                tray_tooltip: "AIjia",
                tray_show: "Show AIjia",
                tray_hide: "Hide to Tray",
                tray_quit: "Quit AIjia",
            }
        );
        assert_eq!(
            app_menu_labels("zh-CN"),
            AppMenuLabels {
                file: "文件",
                edit: "编辑",
                view: "显示",
                window: "窗口",
                help: "帮助",
                navigation: "导航",
                back: "后退",
                forward: "前进",
                tray_tooltip: "AI 小家",
                tray_show: "显示主窗口",
                tray_hide: "隐藏到托盘",
                tray_quit: "退出 AI 小家",
            }
        );
        assert_eq!(
            app_menu_labels("fr-FR"),
            AppMenuLabels {
                file: "文件",
                edit: "编辑",
                view: "显示",
                window: "窗口",
                help: "帮助",
                navigation: "导航",
                back: "后退",
                forward: "前进",
                tray_tooltip: "AI 小家",
                tray_show: "显示主窗口",
                tray_hide: "隐藏到托盘",
                tray_quit: "退出 AI 小家",
            }
        );
    }
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
    use super::*;

    #[test]
    fn scan_external_plugins_is_intentional_noop() {
        // scan_external_plugins is a no-op in Phase B.
        // SKILL.md disk loading is implemented in Phase C/D via plugin::skill module.
        // This test documents the intentional state.
    }

    #[test]
    fn app_log_retention_defaults_to_three_days() {
        assert_eq!(APP_LOG_RETENTION_DAYS, 3);
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

struct ChannelManagerDeactivator {
    slot: std::sync::Arc<connector::im::ChannelManagerSlot>,
}

#[async_trait::async_trait]
impl auth::AuthDeactivationHandler for ChannelManagerDeactivator {
    async fn on_deactivated(&self) {
        let prev = self.slot.replace(None).await;
        if let Some(cm) = prev {
            tokio::spawn(async move {
                log::info!("[channel] deactivation: shutting down current instance");
                cm.shutdown().await;
            });
        }
    }
}

struct CurrentUserStorageDeactivator {
    cus: std::sync::Arc<storage::CurrentUserStorage>,
}

#[async_trait::async_trait]
impl auth::AuthDeactivationHandler for CurrentUserStorageDeactivator {
    async fn on_deactivated(&self) {
        self.cus.deactivate();
    }
}

struct FileManagerWorkspaceResetter {
    file_mgr: std::sync::Arc<storage::file_manager::FileManager>,
    home: std::sync::Arc<storage::AiJiaHome>,
}

#[async_trait::async_trait]
impl auth::AuthDeactivationHandler for FileManagerWorkspaceResetter {
    async fn on_deactivated(&self) {
        self.file_mgr.update_workspace_path(self.home.root());
    }
}

struct AuthExpiredEmitter {
    app: tauri::AppHandle,
}

#[async_trait::async_trait]
impl auth::AuthRevokedHandler for AuthExpiredEmitter {
    async fn on_revoked(&self, message: &str) {
        let _ = self.app.emit(
            "auth:expired",
            serde_json::json!({
                "message": message,
            }),
        );
    }
}

struct ChannelJudgeSettingsProvider {
    db: std::sync::Arc<storage::file_store::AppStorage>,
    crypto: Option<std::sync::Arc<storage::crypto::SecureStorage>>,
}

impl connector::im::ask_coordinator::JudgeSettingsProvider for ChannelJudgeSettingsProvider {
    fn load_settings(&self) -> models::settings::AppSettings {
        let settings_map = self.db.get_all_settings().unwrap_or_default();
        let mut settings = if settings_map.is_empty() {
            models::settings::AppSettings::default()
        } else {
            models::settings::AppSettings::from_string_map(&settings_map)
        };
        if let Some(ss) = self.crypto.as_ref() {
            settings.primary_api_key = transport::tauri_commands::settings::decrypt_if_encrypted(
                ss,
                &settings.primary_api_key,
            );
        }
        settings
    }
}

/// Idempotent helper: ensure an active `ChannelManager` exists in the slot
/// matching the currently-active user scope. Safe to call from setup (when
/// the user was already logged in at boot) and from `cloud_login` (post-
/// login activation).
pub async fn ensure_channel_manager_registered(app: &tauri::AppHandle) {
    use std::sync::Arc;
    use tauri::Manager;

    let cus = app
        .state::<Arc<storage::CurrentUserStorage>>()
        .inner()
        .clone();
    let Some(paths) = cus.resolve_paths() else {
        log::info!("[channel] ensure: no active user scope, slot stays empty");
        return;
    };
    let current_scope = cus.scope();

    let slot = app
        .state::<Arc<connector::im::ChannelManagerSlot>>()
        .inner()
        .clone();

    // Fast path: same scope already installed.
    if let Some(existing) = slot.current().await {
        if existing.active_scope() == current_scope {
            log::info!("[channel] ensure: instance already matches active scope");
            return;
        }
    }

    // Build new instance — same wiring as the old inline setup block.
    let chat_adapter_ref = app
        .state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
        .inner()
        .clone();
    let im_app_feedback = app
        .state::<Arc<connector::im::shared::app_feedback::IMAppFeedbackCoordinator>>()
        .inner()
        .clone();

    let output_binding_registry = app
        .state::<Arc<crate::runtime::human_interaction::RunOutputBindingRegistry>>()
        .inner()
        .clone();
    let reply_manager = Arc::new(
        connector::im::DingtalkReplyManager::new_with_output_binding_registry(
            output_binding_registry,
        ),
    );
    im_app_feedback.set_sink(
        reply_manager.clone() as Arc<dyn connector::im::shared::app_feedback::AppFeedbackSink>
    );
    let gateway = app.state::<Arc<llm::gateway::LlmGateway>>().inner().clone();
    let secure_storage = app
        .state::<Option<Arc<storage::crypto::SecureStorage>>>()
        .inner()
        .clone();
    let judge_settings_db = cus.get().unwrap_or_else(|| {
        app.state::<Arc<storage::file_store::AppStorage>>()
            .inner()
            .clone()
    });
    let judge_settings_provider = Arc::new(ChannelJudgeSettingsProvider {
        db: judge_settings_db,
        crypto: secure_storage,
    })
        as Arc<dyn connector::im::ask_coordinator::JudgeSettingsProvider>;
    let reply_judge = Arc::new(
        connector::im::ask_coordinator::GatewayPendingReplyJudge::new(
            gateway,
            judge_settings_provider,
        ),
    ) as Arc<dyn connector::im::ask_coordinator::PendingReplyJudge>;
    let channel_session_ids = app
        .state::<Arc<std::sync::RwLock<std::collections::HashSet<String>>>>()
        .inner()
        .clone();
    let pending_queue_manager = app
        .state::<Arc<crate::runtime::pending::PendingQueueManager>>()
        .inner()
        .clone();
    let ask_coordinator = Arc::new(
        connector::im::ask_coordinator::IMAskCoordinator::new_with_judge(
            channel_session_ids.clone()
                as Arc<dyn connector::im::ask_coordinator::ChannelSessionRegistry>,
            reply_manager.clone() as Arc<dyn connector::im::ask_coordinator::AskOutputSink>,
            chat_adapter_ref.permission_control_plane(),
            chat_adapter_ref.interaction_control_plane(),
            reply_judge,
        )
        .with_app_feedback(im_app_feedback.clone())
        .with_pending_queue(pending_queue_manager),
    );

    let new_cm = Arc::new(connector::im::ChannelManager::new(
        app.clone(),
        chat_adapter_ref,
        cus.get()
            .map(|s| s as Arc<dyn crate::runtime::store::ConversationStore>)
            .unwrap_or_else(|| {
                app.state::<Arc<storage::file_store::RuntimeRepositoryFacade>>()
                    .inner()
                    .conversation_store_arc()
            }),
        app.state::<Option<Arc<storage::crypto::SecureStorage>>>()
            .inner()
            .clone(),
        paths.channels_dir(),
        Some(ask_coordinator),
        reply_manager,
        channel_session_ids,
        app.state::<Arc<crate::runtime::pending::PendingQueueManager>>()
            .inner()
            .clone(),
        current_scope,
    ));

    // Swap in, fire-and-forget shutdown on old.
    let prev = slot.replace(Some(new_cm.clone())).await;
    if let Some(old) = prev {
        tokio::spawn(async move {
            log::info!("[channel] ensure: shutting down previous instance");
            old.shutdown().await;
        });
    }

    // Hydrate + auto-connect on the new instance.
    let cm_clone = new_cm.clone();
    tauri::async_runtime::spawn(async move {
        cm_clone.hydrate_conversations().await;
        cm_clone.auto_connect_if_configured().await;
    });
    log::info!("[channel] ensure: installed new ChannelManager for scope");
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
