//! Domain trait for persona management.
//!
//! # DEPRECATED (2026-05-10)
//!
//! Persona 系统已进入退役流程，由数字员工（Employee, `runtime/employee/`）替代。
//! agenda 切换到 `organizer_employee_id`（PR-5）之前保留这个 trait；
//! 新代码请用 `runtime::employee::store::EmployeeStore`。
//!
//! Commands that manage personas (list, get, save, delete, set-active, export/import)
//! go through this trait so they are decoupled from the `AppStorage` file-store details.

use anyhow::Result;

/// Lightweight summary returned by list operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaSummary {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub builtin: bool,
    #[serde(default)]
    pub name_en: String,
    #[serde(default)]
    pub description_en: String,
}

/// Full persona definition (mirrors `file_store::persona::Persona`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaRecord {
    pub id: String,
    pub version: u32,
    pub builtin: bool,
    pub name: String,
    pub icon: String,
    pub description: String,
    #[serde(default)]
    pub name_en: String,
    #[serde(default)]
    pub description_en: String,
    pub identity: String,
    pub expertise: Vec<String>,
    pub memory_hints: Vec<String>,
    pub linked_categories: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub trait PersonaStore: Send + Sync {
    fn list_personas(&self) -> Result<Vec<PersonaSummary>>;
    fn get_persona(&self, id: &str) -> Result<PersonaRecord>;
    fn save_persona(&self, persona: &PersonaRecord) -> Result<()>;
    fn delete_persona(&self, id: &str) -> Result<()>;
    fn get_active_persona_id(&self) -> Result<String>;
    fn set_active_persona(&self, id: &str) -> Result<()>;
    fn export_persona(&self, id: &str) -> Result<String>;
    fn import_persona(&self, json: &str) -> Result<String>;
}
