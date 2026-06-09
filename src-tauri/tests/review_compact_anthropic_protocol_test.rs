#[test]
fn compact_summary_client_system_segment_does_not_enable_cache_control() {
    let src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/llm/compact_summary_client.rs"
    ));

    assert!(
        src.contains("cache: false"),
        "compact summary system segment must be explicit about not using cache_control"
    );
    assert!(
        !src.contains("cache: true"),
        "compact summary calls must not attach Anthropic cache_control"
    );
}

#[test]
fn compact_reinjection_is_not_mid_history_system_message() {
    let compaction_src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/runtime/chat/compaction.rs"
    ));
    let preprocess_src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/runtime/chat/preprocess.rs"
    ));

    assert!(
        !compaction_src.contains("claude_md_reinjection"),
        "project instruction reinjection must not create a normal role=system history message"
    );
    assert!(
        !preprocess_src.contains("claude_md_reinjection"),
        "preprocess must carry post-compact project instructions as system segments"
    );
}

#[test]
fn compact_summary_client_is_stateless_and_conversation_id_is_trace_only() {
    let src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/llm/compact_summary_client.rs"
    ));

    assert!(
        !src.contains("settings: AppSettings"),
        "LlmCompactSummaryClient must not store a default AppSettings snapshot"
    );
    assert!(
        !src.contains("AppSettings::default()"),
        "compact summary settings must come from the current turn"
    );
    assert!(
        !src.contains("Some(conversation_id)"),
        "conversation_id must not be passed as gateway sticky-routing state for compact summaries"
    );
    assert!(
        src.contains("Some(vec![]), // tool_defs_override"),
        "compact summary calls must explicitly disable tools instead of falling back to the full registry"
    );
}

#[test]
fn runtime_chat_modules_do_not_import_tauri_for_compaction() {
    let compaction_src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/runtime/chat/compaction.rs"
    ));
    let preprocess_src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/runtime/chat/preprocess.rs"
    ));

    assert!(!compaction_src.contains("use tauri"));
    assert!(!preprocess_src.contains("use tauri"));
}

#[test]
fn manual_compact_has_dedicated_tauri_command_and_trigger() {
    let commands_src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands/chat.rs"));
    let lib_src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"));
    let transport_src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/transport/tauri_commands/chat.rs"
    ));

    assert!(
        commands_src.contains("pub async fn compact_conversation"),
        "manual /compact must be exposed as a dedicated Tauri command"
    );
    assert!(
        lib_src.contains("chat::compact_conversation"),
        "manual compact command must be registered in the Tauri invoke handler"
    );
    assert!(
        transport_src.contains("pub async fn compact_conversation"),
        "manual compact must be implemented on TauriChatCommandAdapter"
    );
    assert!(
        transport_src.contains("PreprocessTrigger::ManualCompact"),
        "manual compact must run the compaction pipeline with ManualCompact trigger"
    );
}

#[test]
fn non_stream_gateway_dispatch_handles_aijia_v2_provider() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/llm/gateway.rs"));
    let dispatch_send = src
        .split("async fn dispatch_send")
        .nth(1)
        .and_then(|tail| tail.split("#[cfg(test)]").next())
        .expect("gateway.rs must define dispatch_send before its unit tests");

    assert!(
        dispatch_send.contains("\"aijia-v2\""),
        "non-stream dispatch_send must route aijia-v2 instead of falling back to lotus"
    );
    assert!(
        dispatch_send.contains("aijia_gateway_v2::AijiaGatewayV2Provider::with_route"),
        "non-stream dispatch_send must use the V2 gateway provider implementation"
    );
}
