//! Pending permission control plane 的 owner 已收敛到 RuntimeChatTurnDriver；
//! worker config 不应再回退持有旧字段。

#[test]
fn review_worker_config_no_longer_owns_control_plane_field() {
    let worker_src = std::fs::read_to_string("src/runtime/agent/worker_runtime.rs")
        .expect("read worker_runtime.rs");
    assert!(
        !worker_src.contains("pub control_plane:"),
        "WorkerRunConfig 不应重新持有 control_plane 字段"
    );
}

#[test]
fn review_runtime_chat_driver_owns_pending_permission_control_plane() {
    let driver_src = std::fs::read_to_string("src/runtime/chat/chat_turn_driver.rs")
        .expect("read chat_turn_driver.rs");
    assert!(
        driver_src.contains(
            "pending_permission_control_plane: Option<Arc<dyn PendingPermissionControlPlane>>"
        ),
        "RuntimeChatTurnDriver 应继续持有 pending_permission_control_plane"
    );
    assert!(
        driver_src.contains("with_llm_executor_and_permission_control_plane"),
        "RuntimeChatTurnDriver 应保留 control plane 注入入口"
    );
}
