use std::sync::Arc;

use crate::auth::client::BASE_URL;
use crate::auth::AuthManager;
use crate::runtime::desktop_resources::catalog::DesktopResourceIndex;
use crate::runtime::desktop_resources::sync;
use tauri::State;

#[tauri::command]
pub async fn sync_desktop_resources(
    auth_manager: State<'_, Arc<AuthManager>>,
    language: Option<String>,
) -> Result<DesktopResourceIndex, String> {
    let session_key = auth_manager
        .get_session_key()
        .await
        .map_err(|err| err.to_string())?;
    let client = reqwest::Client::new();
    sync::sync_desktop_resources(&client, BASE_URL, &session_key, language.as_deref())
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn get_desktop_resource_status() -> Result<DesktopResourceIndex, String> {
    Ok(DesktopResourceIndex::default())
}
