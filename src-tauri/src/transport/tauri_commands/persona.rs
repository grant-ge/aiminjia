//! Tauri command adapter for persona IPC.
//!
//! # DEPRECATED (2026-05-10)
//!
//! Persona 系统已进入退役流程，由数字员工（Employee, `employee_*` commands）替代。
//! 这些命令在 PR-5 完成 agenda 迁移后会被移除；前端 `tauri.ts` 中对应的导出
//! 已全部带 `@deprecated` JSDoc。新代码请使用 `employee_list` / `employee_get` 等。

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
        self.db.delete_persona(&id).map_err(|e| e.to_string())?;
        // 联动：把这个 persona 作 organizer 的 agenda items 转 Orphaned，
        // 让它们在 UI 显示警示色，可被用户重指 organizer 复活（spec §1.8）。
        // 失败仅 log，不阻塞 persona 删除——孤儿 item 不会再触发（runner 只接 Active）。
        let agenda_store = crate::runtime::agenda::AgendaStore::new(self.db.base_dir());
        if let Err(e) = agenda_store.mark_orphaned_by_organizer(&id) {
            log::warn!(
                "[delete_persona] mark_orphaned_by_organizer({}) failed: {}",
                id,
                e
            );
        }
        Ok(())
    }

    pub fn set_active_persona(&self, id: String) -> Result<(), String> {
        self.db.set_active_persona(&id).map_err(|e| e.to_string())
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
