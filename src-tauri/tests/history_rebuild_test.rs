mod common;

use app_lib::runtime::chat::compaction::{CompactBoundaryRecord, CompactTrigger};
use app_lib::runtime::chat::history::{HistoryConfig, build_chat_history};
use app_lib::storage::file_store::AppStorage;
use app_lib::storage::file_store::types::StoredMessage;
use app_lib::transport::tauri_commands::chat::{
    deserialize_chat_messages_for_gateway, load_history_via_runtime_history,
};
use tempfile::TempDir;

fn setup_storage() -> (AppStorage, TempDir) {
    let dir = TempDir::new().expect("create temp dir");
    let storage = AppStorage::new(dir.path()).expect("create app storage");
    storage
        .create_conversation("c1", "History Test")
        .expect("create conversation");
    (storage, dir)
}

#[test]
fn valid_tool_pair_passes_through() {
    let stored = vec![
        common::make_user("1", "hello"),
        common::make_assistant_with_tc("2", "tc_1", "exec"),
        common::make_tool_result("3", "tc_1", "exec", "result"),
        common::make_assistant("4", "done"),
    ];

    let history =
        build_chat_history(&stored, None, &HistoryConfig::default(), None).expect("build history");
    assert_eq!(history.len(), 4);
    assert_eq!(history[1].role, "assistant");
    assert!(
        history[1]
            .tool_calls
            .as_ref()
            .is_some_and(|calls| calls.len() == 1)
    );
    assert_eq!(history[2].role, "tool");
    assert_eq!(history[2].tool_call_id.as_deref(), Some("tc_1"));
}

#[test]
fn orphan_tool_dropped() {
    let stored = vec![
        common::make_user("1", "hi"),
        common::make_tool_result("2", "tc_99", "exec", "orphan"),
        common::make_assistant("3", "ok"),
    ];

    let history =
        build_chat_history(&stored, None, &HistoryConfig::default(), None).expect("build history");
    assert!(!history.iter().any(|m| m.role == "tool"));
}

#[test]
fn assistant_without_result_tool_calls_cleared() {
    let stored = vec![
        common::make_user("1", "hi"),
        common::make_assistant_with_tc("2", "tc_1", "exec"),
        common::make_assistant("3", "done"),
    ];

    let history =
        build_chat_history(&stored, None, &HistoryConfig::default(), None).expect("build history");
    let assistant_with_tool_calls = history
        .iter()
        .find(|m| m.role == "assistant" && m.tool_calls.is_some());
    assert!(
        assistant_with_tool_calls.is_none(),
        "assistant tool_calls without corresponding tool result should be cleared"
    );
}

#[test]
fn boundary_summary_and_tail_slice_are_applied() {
    let stored = vec![
        common::make_user("1", "old question"),
        common::make_assistant("2", "old answer"),
        common::make_user("3", "new question"),
        common::make_assistant("4", "new answer"),
    ];
    let boundary = CompactBoundaryRecord {
        id: "b1".into(),
        conversation_id: "c1".into(),
        trigger: CompactTrigger::Auto,
        pre_tokens: 100,
        post_tokens: 20,
        messages_summarized: 2,
        created_at: "2026-04-24T00:00:00Z".into(),
        summary_text: "summary text".into(),
        tail_message_id: Some("3".into()),
        preserved_segment: None,
    };

    let history = build_chat_history(&stored, Some(&boundary), &HistoryConfig::default(), None)
        .expect("build history");
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].role, "user");
    assert!(history[0].content.contains("summary text"));
    assert_eq!(history[1].content, "new question");
    assert_eq!(history[2].content, "new answer");
}

#[test]
fn user_history_with_uploaded_files_preserves_file_hints() {
    let mut user = common::make_user("1", "请继续分析这个表格");
    user.content = serde_json::json!({
        "text": "请继续分析这个表格",
        "files": [
            {
                "id": "attachment-1",
                "fileName": "sales.csv",
                "filePath": "/tmp/sales.csv",
                "kind": "file",
                "fileType": "csv",
                "mimeType": "text/csv"
            }
        ]
    });

    let history =
        build_chat_history(&[user], None, &HistoryConfig::default(), None).expect("build history");
    assert_eq!(history.len(), 1);
    assert!(history[0].content.contains("[当前消息附件]"));
    assert!(history[0].content.contains("/tmp/sales.csv"));
    assert!(history[0].content.contains("本轮附件已自动加入授权目录"));
}

#[test]
fn user_history_with_authorized_workspace_uses_workspace_hint() {
    let mut user = common::make_user("1", "请继续分析这个表格");
    user.content = serde_json::json!({
        "text": "请继续分析这个表格",
        "files": [
            {
                "id": "attachment-1",
                "fileName": "sales.csv",
                "filePath": "/tmp/sales.csv",
                "kind": "file",
                "fileType": "csv",
                "mimeType": "text/csv"
            }
        ]
    });

    let config = HistoryConfig {
        include_uploaded_file_hints: true,
        has_authorized_workspace: true,
    };
    let history = build_chat_history(&[user], None, &config, None).expect("build history");
    assert_eq!(history.len(), 1);
    assert!(history[0].content.contains("Read"));
}

#[test]
fn load_history_via_runtime_history_uses_v2_storage_and_boundary() {
    let (storage, _dir) = setup_storage();
    storage
        .insert_chat_message_record(&common::make_user("1", "old question"))
        .expect("insert old user");
    storage
        .insert_chat_message_record(&common::make_assistant("2", "old answer"))
        .expect("insert old assistant");

    let mut new_user = common::make_user("3", "new question");
    new_user.sequence = Some(3);
    storage
        .insert_chat_message_record(&new_user)
        .expect("insert new user");

    let mut new_assistant = common::make_assistant("4", "new answer");
    new_assistant.sequence = Some(4);
    storage
        .insert_chat_message_record(&new_assistant)
        .expect("insert new assistant");

    storage
        .append_compact_boundary(&CompactBoundaryRecord {
            id: "b1".into(),
            conversation_id: "c1".into(),
            trigger: CompactTrigger::Auto,
            pre_tokens: 100,
            post_tokens: 20,
            messages_summarized: 2,
            created_at: "2026-04-24T00:00:00Z".into(),
            summary_text: "summary text".into(),
            tail_message_id: Some("3".into()),
            preserved_segment: None,
        })
        .expect("append boundary");

    let history = load_history_via_runtime_history(&storage, "c1", false).expect("load history");
    assert_eq!(history.len(), 3);
    assert_eq!(history[0]["role"], "user");
    assert!(
        history[0]["content"]
            .as_str()
            .unwrap_or("")
            .contains("summary text")
    );
    assert_eq!(history[1]["content"], "new question");
    assert_eq!(history[2]["content"], "new answer");
}

#[test]
fn load_history_via_runtime_history_can_recover_boundary_from_transcript_artifact() {
    let (storage, _dir) = setup_storage();
    storage
        .insert_chat_message_record(&common::make_user("1", "old question"))
        .expect("insert old user");
    storage
        .insert_chat_message_record(&common::make_assistant("2", "old answer"))
        .expect("insert old assistant");
    storage
        .insert_chat_message_record(&common::make_user("3", "current question"))
        .expect("insert current user");

    let boundary = StoredMessage {
        id: "4".to_string(),
        conversation_id: "c1".to_string(),
        role: "system".to_string(),
        content: serde_json::json!({"text": "Conversation compacted"}),
        created_at: "2026-04-24T00:00:04Z".to_string(),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        subtype: Some("compact_boundary".to_string()),
        compact_metadata: Some(serde_json::json!({
            "trigger": "auto",
            "preTokens": 1000,
            "postTokens": 200,
            "messagesSummarized": 3,
            "tailMessageId": "3"
        })),
        is_compact_summary: None,
        run_id: None,
        schema_version: Some(2),
        sequence: None,
        seq: None,
        rev: None,
        error: None,
    };
    storage
        .insert_chat_message_record(&boundary)
        .expect("insert compact boundary transcript artifact");

    let mut summary = common::make_user("5", "<context>\nsummary body\n</context>");
    summary.is_compact_summary = Some(true);
    storage
        .insert_chat_message_record(&summary)
        .expect("insert compact summary transcript artifact");
    storage
        .insert_chat_message_record(&common::make_assistant("6", "current answer"))
        .expect("insert current assistant");

    let history = load_history_via_runtime_history(&storage, "c1", false).expect("load history");

    assert_eq!(history.len(), 3);
    assert_eq!(history[0]["role"], "user");
    assert!(
        history[0]["content"]
            .as_str()
            .unwrap_or("")
            .contains("summary body")
    );
    assert_eq!(history[1]["id"], "3");
    assert_eq!(history[1]["content"], "current question");
    assert_eq!(history[2]["content"], "current answer");
    assert!(!history.iter().any(|message| {
        message.get("subtype").and_then(|value| value.as_str()) == Some("compact_boundary")
            || message
                .get("isCompactSummary")
                .and_then(|value| value.as_bool())
                == Some(true)
    }));
}

#[test]
fn load_history_via_runtime_history_merges_incomplete_sidecar_from_transcript_artifact() {
    let (storage, _dir) = setup_storage();
    storage
        .insert_chat_message_record(&common::make_user("1", "old question"))
        .expect("insert old user");
    storage
        .insert_chat_message_record(&common::make_assistant("2", "old answer"))
        .expect("insert old assistant");
    storage
        .insert_chat_message_record(&common::make_user("3", "current question"))
        .expect("insert current user");

    let boundary_artifact = StoredMessage {
        id: "b-transcript".to_string(),
        conversation_id: "c1".to_string(),
        role: "system".to_string(),
        content: serde_json::json!({"text": "Conversation compacted"}),
        created_at: "2026-04-24T00:00:04Z".to_string(),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        subtype: Some("compact_boundary".to_string()),
        compact_metadata: Some(serde_json::json!({
            "trigger": "auto",
            "preTokens": 1000,
            "postTokens": 200,
            "messagesSummarized": 2,
            "tailMessageId": "3"
        })),
        is_compact_summary: None,
        run_id: None,
        schema_version: Some(2),
        sequence: None,
        seq: None,
        rev: None,
        error: None,
    };
    storage
        .insert_chat_message_record(&boundary_artifact)
        .expect("insert boundary transcript artifact");
    let mut summary = common::make_user("5", "<context>\ntranscript summary\n</context>");
    summary.is_compact_summary = Some(true);
    storage
        .insert_chat_message_record(&summary)
        .expect("insert summary transcript artifact");
    storage
        .insert_chat_message_record(&common::make_assistant("6", "current answer"))
        .expect("insert current assistant");

    storage
        .append_compact_boundary(&CompactBoundaryRecord {
            id: "b-sidecar".into(),
            conversation_id: "c1".into(),
            trigger: CompactTrigger::Auto,
            pre_tokens: 1000,
            post_tokens: 200,
            messages_summarized: 2,
            created_at: "2026-04-24T00:00:04Z".into(),
            summary_text: String::new(),
            tail_message_id: None,
            preserved_segment: None,
        })
        .expect("append incomplete sidecar boundary");

    let history = load_history_via_runtime_history(&storage, "c1", false).expect("load history");

    assert_eq!(history.len(), 3);
    assert_eq!(history[0]["role"], "user");
    assert!(
        history[0]["content"]
            .as_str()
            .unwrap_or("")
            .contains("transcript summary")
    );
    assert_eq!(history[1]["id"], "3");
    assert_eq!(history[1]["content"], "current question");
    assert_eq!(history[2]["content"], "current answer");
}

#[test]
fn load_history_via_runtime_history_keeps_over_budget_history_for_compaction() {
    let (storage, _dir) = setup_storage();
    let large_prompt = "x".repeat(166_000);
    storage
        .insert_chat_message_record(&common::make_user("1", &large_prompt))
        .expect("insert large user message");
    storage
        .insert_chat_message_record(&common::make_assistant("2", "ack"))
        .expect("insert assistant message");

    let history = load_history_via_runtime_history(&storage, "c1", false).expect("load history");

    assert_eq!(
        history.len(),
        2,
        "production history loading must not pre-trim before auto-compact can inspect pressure"
    );
    assert_eq!(history[0]["id"], "1");
    assert!(
        history[0]["content"]
            .as_str()
            .expect("history content should be string")
            .len()
            > 120_000
    );
}

#[test]
fn load_history_via_runtime_history_keeps_more_than_30_rounds_for_compaction() {
    let (storage, _dir) = setup_storage();

    for i in 0..35u64 {
        let mut user = common::make_user(
            &(i * 2 + 1).to_string(),
            &format!("round-{i} 已排除误判=QUALITY-COMPACT-EXCLUDE-MANUAL-PATH"),
        );
        user.sequence = Some(i * 2 + 1);
        storage
            .insert_chat_message_record(&user)
            .expect("insert user");

        let mut assistant =
            common::make_assistant(&(i * 2 + 2).to_string(), &format!("round-{i} ack"));
        assistant.sequence = Some(i * 2 + 2);
        storage
            .insert_chat_message_record(&assistant)
            .expect("insert assistant");
    }

    let history = load_history_via_runtime_history(&storage, "c1", false).expect("load history");

    assert_eq!(
        history.len(),
        70,
        "production history loading must not drop early rounds before auto-compact can summarize them"
    );
    assert_eq!(history[0]["role"], "user");
    assert!(
        history[0]["content"]
            .as_str()
            .expect("history content should be string")
            .contains("QUALITY-COMPACT-EXCLUDE-MANUAL-PATH"),
        "early exclusion facts must survive into the compaction input so later LLM turns do not reinterpret them"
    );
}

#[test]
fn load_history_via_runtime_history_preserves_authorized_workspace_file_hints() {
    let (storage, _dir) = setup_storage();
    let mut user = common::make_user("1", "请继续分析这个表格");
    user.content = serde_json::json!({
        "text": "请继续分析这个表格",
        "files": [
            {
                "id": "attachment-1",
                "fileName": "sales.csv",
                "filePath": "/tmp/sales.csv",
                "kind": "file",
                "fileType": "csv",
                "mimeType": "text/csv"
            }
        ]
    });

    storage
        .insert_chat_message_record(&user)
        .expect("insert user with files");

    let history = load_history_via_runtime_history(&storage, "c1", true).expect("load history");
    assert_eq!(history.len(), 1);
    let content = history[0]["content"].as_str().unwrap_or("");
    assert!(content.contains("Read"));
}

#[test]
fn user_history_with_file_path_round_trips_attachment_path() {
    let mut user = common::make_user("1", "请分析这个截图");
    user.content = serde_json::json!({
        "text": "请分析这个截图",
        "files": [
            {
                "id": "attachment-image-1",
                "fileName": "clipboard-1.png",
                "filePath": "/tmp/clipboard-1.png",
                "kind": "image",
                "fileType": "image",
                "fileSize": 12,
                "mimeType": "image/png"
            }
        ]
    });

    let history =
        build_chat_history(&[user], None, &HistoryConfig::default(), None).expect("build history");
    assert_eq!(history.len(), 1);
    assert!(history[0].content.contains("/tmp/clipboard-1.png"));
    assert!(history[0].content.contains("clipboard-1.png"));
}

#[test]
fn deserialize_chat_messages_for_gateway_reports_dropped_messages() {
    let input = vec![
        serde_json::json!({ "role": "user", "content": "ok" }),
        serde_json::json!({ "role": "user", "content": 123 }),
    ];

    let result = deserialize_chat_messages_for_gateway(&input, "c1");
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.dropped_count, 1);
    assert_eq!(result.messages[0].content, "ok");
}
