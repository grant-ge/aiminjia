#[test]
fn review_send_message_entrypoint_routes_through_session_runtime() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");
    assert!(
        source.contains("self.runtime\n            .run_chat_request(request)")
            || source.contains("self.runtime.run_chat_request(request)"),
        "send_message production entry must delegate through SessionRuntime::run_chat_request"
    );
}

#[test]
fn review_send_message_clears_gateway_busy_after_runtime_returns() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");
    let runtime_call = source
        .find("self.runtime.run_chat_request(request).await")
        .or_else(|| source.find("runtime.run_chat_request(request).await"))
        .or_else(|| source.find(".run_chat_request(request).await"))
        .expect("send_message should call SessionRuntime::run_chat_request");
    let clear_call = source[runtime_call..]
        .find(".clear_task_for_run(&conversation_id, &run_id)")
        .expect("send_message should clear gateway busy state after runtime returns");

    assert!(
        clear_call < 500,
        "gateway busy cleanup should live immediately after runtime returns, before follow-up work"
    );
}

#[test]
fn review_chat_runtime_impl_is_helper_only_not_loop_owner() {
    let source = include_str!("../src/transport/tauri_commands/chat/chat_runtime_impl.rs");

    assert!(
        source.contains("pub(crate) async fn build_visible_tool_defs(")
            || source.contains("pub(crate) fn build_llm_content("),
        "chat_runtime_impl.rs should remain as a helper module for transport/runtime bridging"
    );

    for forbidden in [
        "pub(crate) async fn legacy_send_message_impl(",
        "async fn agent_loop(",
        "fn finish_agent(",
    ] {
        assert!(
            !source.contains(forbidden),
            "chat_runtime_impl.rs should not keep legacy loop owner symbol: {forbidden}"
        );
    }
}

#[test]
fn review_chat_transport_no_longer_reexports_legacy_chat_support() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");

    for forbidden in ["mod chat_support;", "pub(crate) use chat_support::{"] {
        assert!(
            !source.contains(forbidden),
            "chat.rs should not keep legacy chat_support transport helpers: {forbidden}"
        );
    }
}

#[test]
fn review_send_message_clears_gateway_task_after_runtime_turn_before_title_generation() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");
    let start = source
        .find("async fn run_chat_request_internal")
        .expect("send_message should reserve and clear runs inside run_chat_request_internal");
    let end = source[start..]
        .find("pub fn flush_pending_message_writes")
        .map(|offset| start + offset)
        .expect("run_chat_request_internal should end before flush_pending_message_writes");
    let body = &source[start..end];
    let run_call = body
        .find("runtime.run_chat_request(request).await")
        .expect("send_message should run the runtime chat request");
    let title_guard = body
        .find("if result.is_ok()")
        .expect("send_message should keep title generation behind result.is_ok()");
    let reserve_call = body
        .find(".set_busy_for_run(&conversation_id, run_id.clone())")
        .expect("send_message must reserve the gateway run before the turn starts");
    let cleanup_call = body
        .find(".clear_task_for_run(&conversation_id, &run_id)")
        .expect("send_message must clear only the gateway run it owns after the turn exits");

    assert!(
        reserve_call < run_call && run_call < cleanup_call && cleanup_call < title_guard,
        "gateway active run must be reserved before run_chat_request and cleared by run_id before post-turn work"
    );
}

#[test]
fn review_send_chat_request_inherits_authorized_workspace_for_request_scoped_tools() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");
    let start = source
        .find("pub async fn send_chat_request")
        .expect("send_chat_request should exist");
    let end = source[start..]
        .find("fn default_permission_mode_from_settings")
        .map(|offset| start + offset)
        .expect("send_chat_request should end before default_permission_mode_from_settings");
    let body = &source[start..end];

    assert!(
        body.contains("authorized_workspace: chat_runtime_impl::load_authorized_workspace"),
        "send_chat_request must pass the conversation authorized workspace into request-scoped tools"
    );
    assert!(
        !body.contains("authorized_workspace: None"),
        "send_chat_request must not drop authorized workspace; pending drain and IM turns use this path"
    );
}
