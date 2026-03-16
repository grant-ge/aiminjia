//! Persona management commands.

use std::sync::Arc;
use tauri::State;

use crate::storage::file_store::{AppStorage, persona::{Persona, PersonaSummary}};

#[tauri::command]
pub async fn list_personas(db: State<'_, Arc<AppStorage>>) -> Result<Vec<PersonaSummary>, String> {
    db.list_personas().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_persona(db: State<'_, Arc<AppStorage>>, id: String) -> Result<Persona, String> {
    db.get_persona(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_persona(db: State<'_, Arc<AppStorage>>, persona: Persona) -> Result<(), String> {
    db.save_persona(&persona).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_persona(db: State<'_, Arc<AppStorage>>, id: String) -> Result<(), String> {
    db.delete_persona(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_active_persona(db: State<'_, Arc<AppStorage>>, id: String) -> Result<(), String> {
    db.set_active_persona(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_active_persona(db: State<'_, Arc<AppStorage>>) -> Result<Persona, String> {
    db.get_active_persona().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_personas(db: State<'_, Arc<AppStorage>>, id: String) -> Result<String, String> {
    db.export_persona(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_personas(db: State<'_, Arc<AppStorage>>, json: String) -> Result<String, String> {
    db.import_persona(&json).map_err(|e| e.to_string())
}
