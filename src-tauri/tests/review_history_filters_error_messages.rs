//! PR2 守卫测试：build_chat_history 必须过滤掉 error.is_some() 的 StoredMessage,
//! 避免错误气泡作为对话历史回灌给 LLM（spec §3.2）。
//!
//! 与 claude-code-best `isSyntheticApiErrorMessage` 过滤等价。

use app_lib::runtime::chat::history::{build_chat_history, HistoryConfig};
use app_lib::storage::file_store::types::{ErrorKind, MessageError, StoredMessage};

fn make_user_msg(id: &str, text: &str) -> StoredMessage {
    StoredMessage {
        seq: None,
        rev: None,
        id: id.to_string(),
        conversation_id: "conv-1".to_string(),
        role: "user".to_string(),
        content: serde_json::json!({"text": text}),
        created_at: "2026-05-28T00:00:00Z".to_string(),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: None,
        sequence: None,
        error: None,
    }
}

fn make_assistant_msg(id: &str, text: &str, error: Option<MessageError>) -> StoredMessage {
    StoredMessage {
        seq: None,
        rev: None,
        id: id.to_string(),
        conversation_id: "conv-1".to_string(),
        role: "assistant".to_string(),
        content: serde_json::json!({"text": text}),
        created_at: "2026-05-28T00:00:01Z".to_string(),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: None,
        sequence: None,
        error,
    }
}

#[test]
fn build_chat_history_skips_messages_with_error() {
    let stored = vec![
        make_user_msg("u1", "hi"),
        make_assistant_msg(
            "a1",
            "对不起，AI 服务超时",
            Some(MessageError {
                kind: ErrorKind::ChunkTimeout,
                message: "AI 服务暂时无法响应".to_string(),
                raw: None,
            }),
        ),
        make_user_msg("u2", "再试一次"),
        make_assistant_msg("a2", "好的，这是回复", None),
    ];

    let messages = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();

    // 错误气泡（a1）必须被过滤掉
    let texts: Vec<String> = messages.iter().map(|m| m.content.clone()).collect();
    assert!(
        !texts.iter().any(|t| t.contains("对不起，AI 服务超时")),
        "错误气泡不应被回灌给 LLM: {:?}",
        texts
    );

    // 正常消息（u1, u2, a2）必须保留
    assert!(texts.iter().any(|t| t.contains("hi")), "user u1 应保留");
    assert!(
        texts.iter().any(|t| t.contains("再试一次")),
        "user u2 应保留"
    );
    assert!(
        texts.iter().any(|t| t.contains("好的，这是回复")),
        "assistant a2 应保留"
    );
}

#[test]
fn build_chat_history_with_only_errors_returns_empty() {
    // 一个 user 后跟一连串错误气泡 → user 保留，所有错误过滤
    let stored = vec![
        make_user_msg("u1", "hi"),
        make_assistant_msg(
            "a1",
            "err1",
            Some(MessageError {
                kind: ErrorKind::ChunkTimeout,
                message: "...".to_string(),
                raw: None,
            }),
        ),
        make_assistant_msg(
            "a2",
            "err2",
            Some(MessageError {
                kind: ErrorKind::Network,
                message: "...".to_string(),
                raw: None,
            }),
        ),
    ];

    let messages = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();
    assert_eq!(messages.len(), 1, "只应留下 user u1");
    assert!(messages[0].content.contains("hi"));
}

#[test]
fn build_chat_history_no_error_field_compat() {
    // 旧数据没有 error 字段（反序列化后 error=None）必须正常通过
    let stored = vec![
        make_user_msg("u1", "hi"),
        make_assistant_msg("a1", "回复", None),
    ];
    let messages = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();
    assert_eq!(messages.len(), 2);
}
