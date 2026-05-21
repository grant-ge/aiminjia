#[test]
fn ask_coordinator_does_not_depend_on_tauri() {
    let source = std::fs::read_to_string("src/connector/im/shared/ask_coordinator.rs")
        .expect("read ask_coordinator.rs");
    assert!(!source.contains("use tauri"));
    assert!(!source.contains("tauri::"));
}

#[test]
fn ask_coordinator_uses_sink_trait_for_dingtalk_output() {
    let source = std::fs::read_to_string("src/connector/im/shared/ask_coordinator.rs")
        .expect("read ask_coordinator.rs");
    assert!(source.contains("trait AskOutputSink"));
    assert!(!source.contains("dingtalk_card::"));
    assert!(!source.contains("create_and_deliver_card"));
}
