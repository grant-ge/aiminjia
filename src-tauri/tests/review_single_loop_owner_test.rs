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
        .find("self.services.gateway.clear_task(&conversation_id)")
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
