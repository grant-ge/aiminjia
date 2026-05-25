//! Tool handler implementations.
//!
//! Each tool has a dedicated `handle_*` async function called by its
//! corresponding `ToolPlugin` wrapper in `plugin/builtin/tools/`.
//!
//! # Legacy zone
//!
//! All handler functions in this module accept `&PluginContext`, which is the
//! deprecated full-service-locator context.  The suppression below is
//! intentional — these are existing (legacy) tools bridged via
//! `LegacyToolAdapter`.  Migrate individual handlers to `RuntimeTool` +
//! `CapabilityContext` when touching them; do not add new handlers here.
// Legacy tool handlers: PluginContext is intentionally used here.
// dead_code / unused_imports allowed module-wide because the entire
// module is being phased out in favour of `runtime/tools/builtin/`.
// Re-exports below preserve the public crate API for any straggler
// callers; both the lints and these handlers will be removed together
// once migration finishes.
#![allow(deprecated, dead_code, unused_imports)]

mod dingtalk;
mod search;
pub(crate) mod spawn_subagent;
mod util;

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::plugin::tool_trait::FileMeta;

// ─────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────

/// Result from file-generating tool handlers (retained for compile compatibility
/// with plugin/tool_trait.rs conversion implementations).
pub struct FileGenResult {
    pub content: String,
    pub file_meta: FileMeta,
    pub is_degraded: bool,
    pub degradation_notice: Option<String>,
}

// ─────────────────────────────────────────────────
// Re-exports — preserve external import paths
// ───────────────────────────────────────────��─────

pub(crate) use search::execute_web_search_core;
pub(crate) use search::handle_web_search;
pub(crate) use spawn_subagent::DefaultSpawnSubagentLauncher;
// DingTalk — AI Table (6)
pub(crate) use dingtalk::handle_dingtalk_create_record;
pub(crate) use dingtalk::handle_dingtalk_delete_record;
pub(crate) use dingtalk::handle_dingtalk_list_bases;
pub(crate) use dingtalk::handle_dingtalk_query_records;
pub(crate) use dingtalk::handle_dingtalk_schema;
pub(crate) use dingtalk::handle_dingtalk_update_record;
// DingTalk — Contacts (3)
pub(crate) use dingtalk::handle_dingtalk_get_department;
pub(crate) use dingtalk::handle_dingtalk_get_user;
pub(crate) use dingtalk::handle_dingtalk_search_contacts;
// DingTalk — Chat (3)
pub(crate) use dingtalk::handle_dingtalk_list_groups;
pub(crate) use dingtalk::handle_dingtalk_search_chat;
pub(crate) use dingtalk::handle_dingtalk_send_message;
// DingTalk — Calendar (3)
pub(crate) use dingtalk::handle_dingtalk_create_event;
pub(crate) use dingtalk::handle_dingtalk_free_busy;
pub(crate) use dingtalk::handle_dingtalk_list_events;
// DingTalk — Todo (3)
pub(crate) use dingtalk::handle_dingtalk_complete_todo;
pub(crate) use dingtalk::handle_dingtalk_create_todo;
pub(crate) use dingtalk::handle_dingtalk_list_todos;

// ─────────────────────────────────────────────────
// Argument extraction helpers (shared by submodules)
// ─────────────────────────────────────────────────

/// Extract a required string argument from a JSON Value.
fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required string argument: {}", key))
}

/// Extract an optional string argument.
fn optional_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

/// Extract an optional integer argument with a default value.
fn optional_i64(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

/// Extract an optional f64 argument with a default value.
fn optional_f64(args: &Value, key: &str, default: f64) -> f64 {
    args.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
}

// ─────────────────────────────────────────────────
// Tests — shared helpers + argument extraction tests
// ─────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::plugin::context::PluginContext;
    use crate::storage::file_manager::FileManager;
    use crate::storage::file_store::AppStorage;
    use serde_json::json;
    use std::sync::Arc;

    // ── Test helpers ─────────────────────────────

    pub fn create_test_db() -> (Arc<AppStorage>, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(AppStorage::new(dir.path()).unwrap());
        // Create a conversation for testing.
        db.create_conversation("test_conv_1", "Test Conversation")
            .unwrap();
        (db, dir)
    }

    pub fn create_test_context(db: Arc<AppStorage>) -> PluginContext {
        let workspace = std::env::temp_dir().join("tool_executor_test");
        std::fs::create_dir_all(&workspace).ok();
        PluginContext {
            storage: db,
            file_manager: Arc::new(FileManager::new(&workspace)),
            workspace_path: workspace.clone(),
            conversation_id: "test_conv_1".to_string(),
            session_id: crate::runtime::ids::SessionId::new("test_conv_1"),
            run_id: Some(crate::runtime::ids::RunId::new("run-test-1")),
            agent_id: None,
            app_handle: None,
            auth_manager: None,
            dingtalk_bridge: None,
            model: "test-model".to_string(),
            gateway: None,
            tool_registry: None,
            app_settings: None,
            agent_runtime: None,
            event_bus: None,
            skill_registry: None,
            authorized_workspace: None,
            read_file_state: None,
            cancellation: None,
            permission_mode: crate::runtime::tools::permission::PermissionMode::Default,
            runtime_resolver: None,
            permission_ctx: None,
            current_persona_id: None,
        }
    }

    // ── Argument extraction tests ────────────────

    #[test]
    fn test_require_str_present() {
        let args = json!({"name": "hello"});
        assert_eq!(require_str(&args, "name").unwrap(), "hello");
    }

    #[test]
    fn test_require_str_missing() {
        let args = json!({"other": 42});
        assert!(require_str(&args, "name").is_err());
    }

    #[test]
    fn test_require_str_wrong_type() {
        let args = json!({"name": 123});
        assert!(require_str(&args, "name").is_err());
    }

    #[test]
    fn test_optional_str_present() {
        let args = json!({"key": "value"});
        assert_eq!(optional_str(&args, "key"), Some("value"));
    }

    #[test]
    fn test_optional_str_missing() {
        let args = json!({});
        assert_eq!(optional_str(&args, "key"), None);
    }

    #[test]
    fn test_optional_i64_present() {
        let args = json!({"count": 10});
        assert_eq!(optional_i64(&args, "count", 5), 10);
    }

    #[test]
    fn test_optional_i64_missing() {
        let args = json!({});
        assert_eq!(optional_i64(&args, "count", 5), 5);
    }

    #[test]
    fn test_optional_f64_present() {
        let args = json!({"alpha": 0.01});
        assert!((optional_f64(&args, "alpha", 0.05) - 0.01).abs() < f64::EPSILON);
    }

    #[test]
    fn test_optional_f64_missing() {
        let args = json!({});
        assert!((optional_f64(&args, "alpha", 0.05) - 0.05).abs() < f64::EPSILON);
    }
}
