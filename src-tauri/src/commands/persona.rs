//! Persona management commands.
//!
//! All storage access goes through `RuntimeRepositoryFacade::persona_store()` so that
//! `AppStorage` is hidden behind the domain trait boundary.

use std::sync::Arc;
use tauri::State;

use crate::runtime::store::{PersonaRecord, PersonaSummary};
use crate::storage::file_store::RuntimeRepositoryFacade;

#[tauri::command]
pub async fn list_personas(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
) -> Result<Vec<PersonaSummary>, String> {
    facade
        .persona_store()
        .list_personas()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_persona(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    id: String,
) -> Result<PersonaRecord, String> {
    facade
        .persona_store()
        .get_persona(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_persona(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    persona: PersonaRecord,
) -> Result<(), String> {
    facade
        .persona_store()
        .save_persona(&persona)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_persona(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    id: String,
) -> Result<(), String> {
    facade
        .persona_store()
        .delete_persona(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_active_persona(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    id: String,
) -> Result<(), String> {
    facade
        .persona_store()
        .set_active_persona(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_active_persona(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
) -> Result<PersonaRecord, String> {
    let store = facade.persona_store();
    let id = store.get_active_persona_id().map_err(|e| e.to_string())?;
    store.get_persona(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_personas(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    id: String,
) -> Result<String, String> {
    facade
        .persona_store()
        .export_persona(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_personas(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    json: String,
) -> Result<String, String> {
    facade
        .persona_store()
        .import_persona(&json)
        .map_err(|e| e.to_string())
}
