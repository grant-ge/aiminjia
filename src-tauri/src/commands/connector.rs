//! IPC commands for internal system connector (WebView-based auth + CDP browser).

use std::sync::Arc;
use tauri::State;
use crate::connector::ConnectorEngine;
use crate::connector::types::InternalAppInfo;
use crate::connector::webview_auth::{WebViewAuthManager, WebLoginConfig, CapturedCredential, ProxyResponse};

/// Get all internal apps from cached config.
#[tauri::command]
pub async fn get_internal_apps(
    engine: State<'_, Arc<ConnectorEngine>>,
) -> Result<Vec<InternalAppInfo>, String> {
    Ok(engine.get_apps().await)
}

/// Sync internal apps config from Lotus API.
#[tauri::command]
pub async fn sync_internal_apps(
    engine: State<'_, Arc<ConnectorEngine>>,
) -> Result<(), String> {
    engine.sync_config().await
}

/// Connect to an internal app — opens WebView login + notifies Lotus.
#[tauri::command]
pub async fn connect_internal_app(
    engine: State<'_, Arc<ConnectorEngine>>,
    app_id: u64,
) -> Result<(), String> {
    engine.connect(app_id).await
}

/// Disconnect from an internal app — close session + notify Lotus.
#[tauri::command]
pub async fn disconnect_internal_app(
    engine: State<'_, Arc<ConnectorEngine>>,
    app_id: u64,
) -> Result<(), String> {
    engine.disconnect(app_id).await
}

/// Open a WebView login window for an internal system (low-level, kept for verification).
#[tauri::command]
pub async fn open_internal_login(
    manager: State<'_, Arc<WebViewAuthManager>>,
    app_id: u64,
    app_name: String,
    login_url: String,
    base_url: String,
    success_url_prefix: String,
) -> Result<CapturedCredential, String> {
    let config = WebLoginConfig {
        app_id,
        app_name,
        login_url,
        base_url,
        success_url_prefix,
    };
    manager.open_login(config).await
}

/// Execute an HTTP request through the WebView session (proxied fetch).
#[tauri::command]
pub async fn proxy_internal_request(
    manager: State<'_, Arc<WebViewAuthManager>>,
    app_id: u64,
    method: String,
    url: String,
    headers: Option<serde_json::Value>,
    body: Option<serde_json::Value>,
    cached_token: Option<String>,
) -> Result<ProxyResponse, String> {
    manager
        .proxy_request(
            app_id,
            &method,
            &url,
            headers.as_ref(),
            body.as_ref(),
            cached_token.as_deref(),
        )
        .await
}

/// Check if we have an active WebView session for an app.
#[tauri::command]
pub async fn has_internal_session(
    manager: State<'_, Arc<WebViewAuthManager>>,
    app_id: u64,
) -> Result<bool, String> {
    Ok(manager.has_active_session(app_id).await)
}

/// Close a WebView session for an app.
#[tauri::command]
pub async fn close_internal_session(
    manager: State<'_, Arc<WebViewAuthManager>>,
    app_id: u64,
) -> Result<(), String> {
    manager.close_session(app_id).await;
    Ok(())
}

/// Show the CDP browser window (bring active tab to front).
#[tauri::command]
pub async fn show_browse_view(
    engine: State<'_, Arc<ConnectorEngine>>,
) -> Result<(), String> {
    engine.browser_show().await
}
