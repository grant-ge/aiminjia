use std::path::Path;

#[test]
fn runtime_llm_executor_does_not_contain_compact_summary() {
    let path = Path::new("src/runtime/chat/chat_turn_driver.rs");
    let content = std::fs::read_to_string(path).expect("read chat_turn_driver.rs");

    let trait_start = content
        .find("pub trait RuntimeLlmExecutor")
        .expect("RuntimeLlmExecutor trait must exist");
    let trait_end_marker = "// END_TRAIT_RuntimeLlmExecutor";
    let trait_end = content[trait_start..]
        .find(trait_end_marker)
        .expect("must mark the end of RuntimeLlmExecutor trait with `// END_TRAIT_RuntimeLlmExecutor`")
        + trait_start;
    let trait_body = &content[trait_start..trait_end];

    assert!(
        !trait_body.contains("fn compact_summary"),
        "compact_summary must be removed from RuntimeLlmExecutor; \
         move it to CompactSummaryClient (runtime/chat/compact_client.rs)"
    );
}

#[test]
fn compact_summary_client_trait_exists() {
    let path = Path::new("src/runtime/chat/compact_client.rs");
    let content = std::fs::read_to_string(path)
        .expect("runtime/chat/compact_client.rs must exist after P0.2");
    assert!(
        content.contains("pub trait CompactSummaryClient"),
        "compact_client.rs must declare `pub trait CompactSummaryClient`"
    );
    assert!(
        content.contains("fn compact_summary"),
        "CompactSummaryClient must declare `compact_summary` method"
    );
}
