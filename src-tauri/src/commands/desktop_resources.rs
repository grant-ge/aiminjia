use crate::runtime::desktop_resources::catalog::DesktopResourceIndex;

#[tauri::command]
pub async fn sync_desktop_resources() -> Result<DesktopResourceIndex, String> {
    Ok(DesktopResourceIndex::default())
}

#[tauri::command]
pub async fn get_desktop_resource_status() -> Result<DesktopResourceIndex, String> {
    Ok(DesktopResourceIndex::default())
}
