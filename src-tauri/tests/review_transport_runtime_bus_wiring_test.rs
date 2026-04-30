#[test]
fn review_tauri_chat_command_adapter_should_bridge_runtime_events_to_host() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");
    assert!(
        source.contains("TauriEventAdapter"),
        "production Tauri chat adapter still constructs SessionRuntime without wiring a TauriEventAdapter, so runtime events are recorded but not emitted through the host"
    );
}
