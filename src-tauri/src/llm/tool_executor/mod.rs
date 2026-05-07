//! Tool handler implementations for the 10 registered tools.
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
#![allow(deprecated)]

mod chart;
mod dingtalk;
pub(crate) mod file_load;
mod internal_system;
mod python;
mod report;
mod search;
pub(crate) mod spawn_subagent;
mod util;

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::plugin::tool_trait::FileMeta;

// ─────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────

/// Result from file-generating tool handlers (generate_report, generate_chart).
pub struct FileGenResult {
    pub content: String,
    pub file_meta: FileMeta,
    pub is_degraded: bool,
    pub degradation_notice: Option<String>,
}

// ─────────────────────────────────────────────────
// Re-exports — preserve external import paths
// ─────────────────────────────────────────────────

pub(crate) use chart::build_chart_python;
pub(crate) use chart::handle_generate_chart;
pub(crate) use chart::handle_generate_chart_core;
pub(crate) use chart::ChartCoreParams;
pub(crate) use file_load::handle_load_file;
pub(crate) use internal_system::execute_browse_data;
pub(crate) use internal_system::handle_browse_and_extract;
pub(crate) use internal_system::handle_browse_navigate;
pub(crate) use internal_system::handle_extract_table_data;
pub(crate) use internal_system::handle_extract_with_pagination;
pub(crate) use internal_system::handle_page_execute_js;
pub(crate) use internal_system::handle_read_page_content;
pub(crate) use internal_system::DefaultBrowseDataLauncher;
pub(crate) use python::handle_execute_python;
pub(crate) use python::handle_execute_python_core;
pub(crate) use python::ExecutePythonCoreParams;
pub(crate) use report::generate_report_bytes_core;
pub(crate) use report::handle_generate_report;
pub(crate) use report::handle_generate_report_core;
pub(crate) use report::ReportCoreParams;
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
pub(crate) use util::py_escape;
pub(crate) use util::slugify;

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
            tavily_api_key: None,
            bocha_api_key: None,
            app_handle: None,
            session_manager: Arc::new(crate::python::session::PythonSessionManager::new(
                workspace, None,
            )),
            auth_manager: None,
            connector_engine: None,
            dingtalk_bridge: None,
            use_cloud: false,
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

    // ── util tests ──────────────────────────────

    #[test]
    fn test_slugify() {
        assert_eq!(util::slugify("Hello World"), "hello_world");
        assert_eq!(util::slugify("Report #1 (Final)"), "report__1__final");
    }

    #[test]
    fn test_indent_python_basic() {
        let code = "line1\nline2\nline3";
        let indented = util::indent_python(code, 4);
        assert_eq!(indented, "    line1\n    line2\n    line3");
    }

    #[test]
    fn test_indent_python_preserves_relative() {
        let code = "if True:\n    print('hi')";
        let indented = util::indent_python(code, 4);
        assert_eq!(indented, "    if True:\n        print('hi')");
    }

    #[test]
    fn test_indent_python_skips_empty_lines() {
        let code = "a\n\nb";
        let indented = util::indent_python(code, 4);
        assert_eq!(indented, "    a\n\n    b");
    }
}
