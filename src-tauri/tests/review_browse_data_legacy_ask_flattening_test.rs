#[test]
fn review_internal_system_no_longer_keeps_legacy_browse_data_ask_string_fallback() {
    let source = include_str!("../src/llm/tool_executor/internal_system.rs");

    assert!(
        !source.contains("Permission Ask required:"),
        "internal_system.rs should not keep the legacy browse_data Ask string flattening fallback"
    );
    assert!(
        !source.contains("pub(crate) async fn handle_browse_data("),
        "internal_system.rs should not keep the legacy string-returning browse_data helper"
    );
}

#[test]
fn review_tool_executor_mod_no_longer_reexports_legacy_browse_data_helper() {
    let source = include_str!("../src/llm/tool_executor/mod.rs");

    assert!(
        !source.contains("pub(crate) use internal_system::handle_browse_data;"),
        "tool_executor/mod.rs should not re-export the removed legacy browse_data helper"
    );
}
