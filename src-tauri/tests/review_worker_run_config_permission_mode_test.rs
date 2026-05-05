//! WorkerRunConfig 必须携带 permission_mode 字段。

use app_lib::runtime::agent::worker_runtime::WorkerRunConfig;
use app_lib::runtime::tools::permission::PermissionMode;

#[test]
fn review_worker_run_config_has_permission_mode_field() {
    let config = WorkerRunConfig {
        allowed_tools: vec![],
        conversation_id: "conv-test".into(),
        parent_run_id: None,
        background: false,
        app_handle: None,
        cancel_token: None,
        permission_mode: PermissionMode::Plan,
        model_override: None,
        parent_tool_use_id: None,
    };
    assert_eq!(config.permission_mode, PermissionMode::Plan);
}

#[test]
fn review_worker_run_config_default_permission_mode_is_default() {
    let config = WorkerRunConfig {
        allowed_tools: vec![],
        conversation_id: "conv-test".into(),
        parent_run_id: None,
        background: false,
        app_handle: None,
        cancel_token: None,
        permission_mode: PermissionMode::Default,
        model_override: None,
        parent_tool_use_id: None,
    };
    assert_eq!(config.permission_mode, PermissionMode::Default);
}
