use std::collections::HashSet;

use app_lib::runtime::chat::compaction::{AutoCompactConfig, AutoCompactState, MicrocompactConfig};
use app_lib::runtime::chat::preprocess::{
    prepare_messages_for_llm, CollapseConfig, PreprocessConfig, PreprocessRuntimeState,
    PreprocessTrigger, ToolResultBudgetConfig,
};
use serde_json::{json, Value};

#[test]
fn r2_empty_project_instruction_does_not_create_segment() {
    let config = PreprocessConfig {
        project_instruction_content: Some("  ".to_string()),
        ..PreprocessConfig::default()
    };

    assert!(
        config.project_instruction_system_segment().is_none(),
        "blank project instructions should not create a system segment"
    );
}

#[tokio::test]
async fn r2_prepare_messages_returns_project_instruction_system_segment_after_auto_compact() {
    let messages = vec![
        json!({"role": "user", "id": "old", "content": "x".repeat(500)}),
        json!({"role": "assistant", "id": "old-a", "content": "old answer"}),
        json!({"role": "user", "id": "tail", "content": "latest"}),
    ];
    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 32,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: 10_000,
            keep_recent_tool_results: 1,
            replacement_preview_chars: 32,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: 1,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        compact_boundary: None,
        project_instruction_content: Some("# AGENTS.md\nKeep this context".to_string()),
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let prepared = prepare_messages_for_llm(
        messages,
        "conv-r2",
        PreprocessTrigger::Normal,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("summary".to_string()) },
    )
    .await
    .expect("prepare should compact");

    assert_eq!(prepared.post_compact_system_segments.len(), 1);
    let segment = &prepared.post_compact_system_segments[0];
    assert!(
        !segment.cache,
        "compact reinjection must not use cache_control"
    );
    assert!(segment.text.contains("Keep this context"));
    assert!(segment.text.contains("<project_context>"));
    assert!(
        prepared.messages.iter().all(|m| {
            m.get("subtype").and_then(Value::as_str) != Some("claude_md_reinjection")
                && !matches!(
                    (
                        m.get("role").and_then(Value::as_str),
                        m.get("content").and_then(Value::as_str)
                    ),
                    (Some("system"), Some(content)) if content.contains("Keep this context")
                )
        }),
        "project instructions must not be inserted as a normal mid-history system message"
    );
}
