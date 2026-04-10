use std::sync::Arc;

use crate::storage::file_store::{
    persona::{Persona, PersonaSummary},
    AppStorage,
};

#[derive(Clone)]
pub struct TauriPersonaCommandAdapter {
    db: Arc<AppStorage>,
}

impl TauriPersonaCommandAdapter {
    pub fn new(db: Arc<AppStorage>) -> Self {
        Self { db }
    }

    pub fn list_personas(&self) -> Result<Vec<PersonaSummary>, String> {
        self.db.list_personas().map_err(|e| e.to_string())
    }

    pub fn get_persona(&self, id: String) -> Result<Persona, String> {
        self.db.get_persona(&id).map_err(|e| e.to_string())
    }

    pub fn save_persona(&self, persona: Persona) -> Result<(), String> {
        self.db.save_persona(&persona).map_err(|e| e.to_string())
    }

    pub fn delete_persona(&self, id: String) -> Result<(), String> {
        self.db.delete_persona(&id).map_err(|e| e.to_string())
    }

    pub fn set_active_persona(&self, id: String) -> Result<(), String> {
        self.db
            .set_active_persona(&id)
            .map_err(|e| e.to_string())
    }

    pub fn get_active_persona(&self) -> Result<Persona, String> {
        self.db.get_active_persona().map_err(|e| e.to_string())
    }

    pub fn export_personas(&self, id: String) -> Result<String, String> {
        self.db.export_persona(&id).map_err(|e| e.to_string())
    }

    pub fn import_personas(&self, json: String) -> Result<String, String> {
        self.db.import_persona(&json).map_err(|e| e.to_string())
    }
}
