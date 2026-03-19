//! Tauri IPC commands for SaaS connector management.

use std::sync::Arc;
use tauri::State;
use crate::connector::ConnectorEngine;
use crate::connector::types::SaasAppInfo;

#[tauri::command]
pub async fn get_saas_apps(
    engine: State<'_, Arc<ConnectorEngine>>,
) -> Result<Vec<SaasAppInfo>, String> {
    engine.get_apps().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_oauth_connect(
    engine: State<'_, Arc<ConnectorEngine>>,
    app_id: u64,
) -> Result<String, String> {
    engine.get_oauth_url(app_id).await
}

#[tauri::command]
pub async fn check_oauth_status(
    engine: State<'_, Arc<ConnectorEngine>>,
    app_id: u64,
) -> Result<String, String> {
    engine.check_status(app_id).await
}

#[tauri::command]
pub async fn disconnect_saas(
    engine: State<'_, Arc<ConnectorEngine>>,
    app_id: u64,
) -> Result<(), String> {
    engine.disconnect(app_id).await
}

#[tauri::command]
pub async fn sync_saas_config(
    engine: State<'_, Arc<ConnectorEngine>>,
) -> Result<(), String> {
    engine.sync_config().await
}

#[tauri::command]
pub async fn toggle_saas_app(
    engine: State<'_, Arc<ConnectorEngine>>,
    app_id: u64,
    enabled: bool,
) -> Result<(), String> {
    engine.toggle_app(app_id, enabled).await
}

#[tauri::command]
pub async fn set_saas_credential(
    engine: State<'_, Arc<ConnectorEngine>>,
    app_name: String,
    api_credential: String,
) -> Result<String, String> {
    engine.set_credential_by_name(&app_name, &api_credential).await
}
