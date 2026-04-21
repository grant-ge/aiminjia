//! WorkerRunConfig 必须有 control_plane 字段，可接受 Option<Arc<dyn PendingPermissionControlPlane>>。

use std::sync::Arc;

use app_lib::runtime::agent::worker_runtime::WorkerRunConfig;
use app_lib::runtime::store::PendingPermissionControlPlane;
use app_lib::runtime::tools::permission::PermissionMode;

#[test]
fn review_worker_run_config_accepts_optional_control_plane() {
    let _config = WorkerRunConfig {
        allowed_tools: vec![],
        conversation_id: "c".into(),
        parent_run_id: None,
        background: false,
        app_handle: None,
        cancel_token: None,
        permission_mode: PermissionMode::Default,
        control_plane: None::<Arc<dyn PendingPermissionControlPlane>>,
    };
    // 若能编译即通过
}
