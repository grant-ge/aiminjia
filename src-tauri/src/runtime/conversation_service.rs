use std::sync::Arc;

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::ChatMessage;
use crate::models::message::SubAgentTranscriptEntryFrontend;
use crate::models::settings::AppSettings;
use crate::runtime::agent::subagent_result_envelope::SubAgentResultEnvelope;
use crate::runtime::agent::AgentRuntime;
use crate::runtime::store::conversation_store::ConversationStore;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;
use crate::transport::runtime_host::RuntimeHost;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteConversationOutcome {
    pub conversation_id: String,
    pub cancelled_active_agent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameConversationOutcome {
    pub conversation_id: String,
    pub new_title: String,
}

pub async fn stop_streaming(
    gateway: Arc<LlmGateway>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    conversation_id: String,
) -> Result<(), String> {
    if let Some(run_id) = gateway.active_run_id(&conversation_id) {
        let _ = session_mgr.interrupt_run(&run_id).await;
    } else {
        let _ = session_mgr.interrupt(&conversation_id).await;
    }
    gateway
        .cancel_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

pub async fn get_messages(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let messages = db
        .get_messages(&conversation_id)
        .map_err(|e| e.to_string())?;
    Ok(messages
        .into_iter()
        .map(transform_message_json_for_frontend)
        .collect())
}

pub async fn get_subagent_transcript(
    runtime: Arc<AgentRuntime>,
    transcript_ref: String,
) -> Result<Vec<SubAgentTranscriptEntryFrontend>, String> {
    let entries = runtime
        .transcript_store_get(&transcript_ref)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("missing subagent transcript: {transcript_ref}"))?;

    Ok(entries
        .into_iter()
        .map(SubAgentTranscriptEntryFrontend::from)
        .collect())
}

pub fn transform_message_json_for_frontend(mut message: serde_json::Value) -> serde_json::Value {
    let Some(content) = message
        .get_mut("content")
        .and_then(|value| value.as_object_mut())
    else {
        return message;
    };

    let Some(raw_text) = content
        .get("text")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    else {
        return message;
    };

    let Some(envelope) = SubAgentResultEnvelope::from_storage_summary(&raw_text) else {
        return message;
    };

    content.remove("text");
    content.insert(
        "subagentEnvelope".to_string(),
        serde_json::to_value(crate::models::message::SubAgentEnvelopePayload::from(
            envelope,
        ))
        .unwrap_or(serde_json::Value::Null),
    );

    message
}

pub async fn create_conversation(db: Arc<dyn ConversationStore>) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db.create_conversation(&id, "新对话")
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn get_conversation_model_override(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<Option<String>, String> {
    db.get_conversation_model_override(&conversation_id)
        .map_err(|e| e.to_string())
}

pub async fn set_conversation_model_override(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
    model_override: Option<String>,
) -> Result<(), String> {
    db.set_conversation_model_override(&conversation_id, model_override)
        .map_err(|e| e.to_string())
}

pub async fn delete_conversation(
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    session_mgr: Arc<crate::python::session::PythonSessionManager>,
    conversation_id: String,
) -> Result<DeleteConversationOutcome, String> {
    session_mgr.destroy(&conversation_id).await;
    if let Some(run_id) = gateway.active_run_id(&conversation_id) {
        session_mgr.destroy_run(&run_id).await;
    }

    let was_busy = gateway.is_conversation_busy(&conversation_id);
    if was_busy {
        log::info!(
            "delete_conversation: cancelling active agent for conversation {}",
            conversation_id
        );
        gateway.cancel_conversation(&conversation_id).ok();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        gateway.clear_task(&conversation_id);
        db.remove_active_task(&conversation_id).ok();
    }

    let file_paths = db
        .get_file_paths_for_conversation(&conversation_id)
        .map_err(|e| e.to_string())?;

    let mut deleted = 0usize;
    let mut failures = Vec::new();
    for path in &file_paths {
        let full_path = file_mgr.full_path(path);
        match std::fs::remove_file(&full_path) {
            Ok(()) => deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                log::warn!("Failed to delete file {:?}: {}", full_path, e);
                failures.push(format!("{}: {}", full_path.display(), e));
            }
        }
    }
    if !file_paths.is_empty() {
        log::info!(
            "Conversation {} file cleanup: {} deleted, {} failed, {} already gone",
            conversation_id,
            deleted,
            failures.len(),
            file_paths.len() - deleted - failures.len()
        );
    }
    if !failures.is_empty() {
        return Err(format!(
            "failed to delete associated files: {}",
            failures.join("; ")
        ));
    }

    let _ = db.delete_memories_by_prefix(&format!("loaded:{}:", conversation_id));
    let _ = db.delete_memories_by_prefix(&format!("note:{}:", conversation_id));

    db.delete_conversation(&conversation_id)
        .map_err(|e| e.to_string())?;

    Ok(DeleteConversationOutcome {
        conversation_id,
        cancelled_active_agent: was_busy,
    })
}

pub async fn rename_conversation(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
    new_title: String,
) -> Result<RenameConversationOutcome, String> {
    db.rename_conversation(&conversation_id, &new_title)
        .map_err(|e| e.to_string())?;
    Ok(RenameConversationOutcome {
        conversation_id,
        new_title,
    })
}

pub fn sanitize_title(raw: &str) -> String {
    let line = raw.lines().next().unwrap_or("").trim();

    // Strip markdown link syntax: [text](url) -> text
    let mut stripped = String::with_capacity(line.len());
    let bytes: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '[' {
            if let Some(close) = bytes[i + 1..].iter().position(|&c| c == ']') {
                let text_end = i + 1 + close;
                if text_end + 1 < bytes.len() && bytes[text_end + 1] == '(' {
                    if let Some(paren_close) =
                        bytes[text_end + 2..].iter().position(|&c| c == ')')
                    {
                        stripped.extend(&bytes[i + 1..text_end]);
                        i = text_end + 2 + paren_close + 1;
                        continue;
                    }
                }
            }
        }
        stripped.push(bytes[i]);
        i += 1;
    }

    // Drop characters used for markdown decoration anywhere in the line
    // (#, *, _, `, ~) plus surrounding quotes / brackets.
    let cleaned: String = stripped
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '"' | '\''
                    | '\u{201C}'
                    | '\u{201D}'
                    | '\u{2018}'
                    | '\u{2019}'
                    | '「'
                    | '」'
                    | '『'
                    | '』'
                    | '#'
                    | '*'
                    | '_'
                    | '`'
                    | '~'
                    | '['
                    | ']'
                    | '<'
                    | '>'
            )
        })
        .collect();

    let candidate: String = cleaned.trim().chars().take(10).collect();

    if looks_like_refusal(&candidate) {
        return String::new();
    }
    candidate
}

fn looks_like_refusal(s: &str) -> bool {
    const REFUSAL_PREFIXES: &[&str] = &[
        "我无法",
        "我不能",
        "抱歉",
        "对不起",
        "很抱歉",
        "Sorry",
        "sorry",
        "I cannot",
        "I can't",
        "I'm sorry",
        "I am sorry",
        "I am unable",
        "I'm unable",
    ];
    REFUSAL_PREFIXES.iter().any(|p| s.starts_with(p))
}

pub fn should_auto_title(
    db: &dyn crate::runtime::store::conversation_store::ConversationStore,
    conversation_id: &str,
) -> anyhow::Result<bool> {
    let convs = db.get_conversations()?;
    let title = convs
        .iter()
        .find(|c| c["id"].as_str() == Some(conversation_id))
        .and_then(|c| c["title"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    if title != "新对话" {
        return Ok(false);
    }

    let messages = db.get_messages(conversation_id)?;
    let has_user = messages.iter().any(|m| m["role"].as_str() == Some("user"));
    let has_assistant = messages
        .iter()
        .any(|m| m["role"].as_str() == Some("assistant"));
    Ok(has_user && has_assistant)
}

/// Generate and set a title for the conversation.
/// Returns the rename outcome (new conversationId + title) so the caller
/// (transport layer) can emit any necessary events.
/// All errors are logged and swallowed; returns None on failure.
pub async fn generate_and_set_title(
    db: Arc<dyn crate::runtime::store::conversation_store::ConversationStore>,
    gateway: Arc<LlmGateway>,
    host: Arc<dyn RuntimeHost>,
    conversation_id: String,
    settings: AppSettings,
) -> Option<RenameConversationOutcome> {
    match generate_and_set_title_inner(db, gateway, host, conversation_id, settings).await {
        Ok(outcome) => outcome,
        Err(e) => {
            log::warn!("[auto-title] failed: {}", e);
            None
        }
    }
}

async fn generate_and_set_title_inner(
    db: Arc<dyn crate::runtime::store::conversation_store::ConversationStore>,
    gateway: Arc<LlmGateway>,
    host: Arc<dyn RuntimeHost>,
    conversation_id: String,
    settings: AppSettings,
) -> anyhow::Result<Option<RenameConversationOutcome>> {
    // Check title guard inline (no message fetch needed here)
    let convs = db.get_conversations()?;
    let current_title = convs
        .iter()
        .find(|c| c["id"].as_str() == Some(conversation_id.as_str()))
        .and_then(|c| c["title"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    if current_title != "新对话" {
        return Ok(None);
    }

    // Fetch messages once
    let messages = db.get_messages(&conversation_id)?;
    let has_user = messages.iter().any(|m| m["role"].as_str() == Some("user"));
    let has_assistant = messages
        .iter()
        .any(|m| m["role"].as_str() == Some("assistant"));
    if !has_user || !has_assistant {
        return Ok(None);
    }

    let extract_text = |m: &serde_json::Value| -> String {
        m["content"]["text"]
            .as_str()
            .or_else(|| m["content"].as_str())
            .unwrap_or("")
            .chars()
            .take(500)
            .collect()
    };

    let first_nonempty = |role: &str| -> String {
        messages
            .iter()
            .filter(|m| m["role"].as_str() == Some(role))
            .map(extract_text)
            .find(|s| !s.trim().is_empty())
            .unwrap_or_default()
    };

    let first_user = first_nonempty("user");
    let first_assistant = first_nonempty("assistant");

    if first_user.is_empty() {
        anyhow::bail!("no user message content found");
    }

    let mut llm_messages = vec![ChatMessage::text("user", &first_user)];
    if !first_assistant.is_empty() {
        llm_messages.push(ChatMessage::text("assistant", &first_assistant));
    }

    let system_prompt =
        "你是一个对话标题生成器。根据下面的对话内容，用不超过 10 个中文字生成一个简洁标题，\
         只输出纯文本标题本身，禁止使用任何 Markdown 语法（不要 #、*、_、`、链接、引号或括号），\
         不加标点、不加解释。";

    let response = gateway
        .send_message(
            &settings,
            llm_messages,
            MaskingLevel::Relaxed,
            Some(system_prompt),
            None,
            Some(vec![]),
        )
        .await?;

    let title = sanitize_title(&response.content);
    if title.is_empty() {
        anyhow::bail!("sanitized title is empty");
    }

    let outcome = rename_conversation(db, conversation_id, title)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    let _ = host.emit_legacy_event(
        "conversation:title-updated",
        serde_json::json!({
            "conversationId": outcome.conversation_id,
            "title": outcome.new_title,
        }),
    );

    log::info!("[auto-title] set title: {}", outcome.new_title);
    Ok(Some(outcome))
}

pub async fn get_conversations(
    db: Arc<dyn ConversationStore>,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_conversations().map_err(|e| e.to_string())
}

pub async fn archive_conversation(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<(), String> {
    db.archive_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

pub async fn restore_conversation(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
) -> Result<(), String> {
    db.restore_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

pub async fn get_archived_conversations(
    db: Arc<dyn ConversationStore>,
) -> Result<Vec<serde_json::Value>, String> {
    db.get_archived_conversations().map_err(|e| e.to_string())
}

#[cfg(test)]
mod title_tests {
    use super::*;
    use crate::runtime::store::conversation_store::{ConversationStore, InMemoryConversationStore};
    use std::sync::Arc;

    struct StoreWithMessages {
        inner: InMemoryConversationStore,
        messages: std::sync::Mutex<Vec<serde_json::Value>>,
        conversation_id: String,
    }

    impl StoreWithMessages {
        fn new(title: &str, conversation_id: &str, msgs: Vec<serde_json::Value>) -> Arc<Self> {
            let inner = InMemoryConversationStore::new();
            inner.create_conversation(conversation_id, title).unwrap();
            Arc::new(Self {
                inner,
                messages: std::sync::Mutex::new(msgs),
                conversation_id: conversation_id.to_string(),
            })
        }
    }

    impl ConversationStore for StoreWithMessages {
        fn create_conversation(&self, id: &str, title: &str) -> anyhow::Result<()> {
            self.inner.create_conversation(id, title)
        }
        fn list_conversation_ids(&self) -> anyhow::Result<Vec<String>> {
            self.inner.list_conversation_ids()
        }
        fn get_conversations(&self) -> anyhow::Result<Vec<serde_json::Value>> {
            self.inner.get_conversations()
        }
        fn delete_conversation(&self, id: &str) -> anyhow::Result<()> {
            self.inner.delete_conversation(id)
        }
        fn rename_conversation(&self, id: &str, new_title: &str) -> anyhow::Result<()> {
            self.inner.rename_conversation(id, new_title)
        }
        fn insert_active_task(&self, id: &str) -> anyhow::Result<()> {
            self.inner.insert_active_task(id)
        }
        fn remove_active_task(&self, id: &str) -> anyhow::Result<()> {
            self.inner.remove_active_task(id)
        }
        fn get_messages(&self, conversation_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
            if conversation_id == self.conversation_id {
                Ok(self.messages.lock().unwrap().clone())
            } else {
                Ok(vec![])
            }
        }
        fn append_compact_boundary(
            &self,
            record: crate::runtime::chat::compaction::CompactBoundaryRecord,
        ) -> anyhow::Result<()> {
            self.inner.append_compact_boundary(record)
        }
        fn list_compact_boundaries(
            &self,
            id: &str,
        ) -> anyhow::Result<Vec<crate::runtime::chat::compaction::CompactBoundaryRecord>> {
            self.inner.list_compact_boundaries(id)
        }
        fn get_conversation_model_override(&self, id: &str) -> anyhow::Result<Option<String>> {
            self.inner.get_conversation_model_override(id)
        }
        fn set_conversation_model_override(
            &self,
            id: &str,
            v: Option<String>,
        ) -> anyhow::Result<()> {
            self.inner.set_conversation_model_override(id, v)
        }
        fn archive_conversation(&self, id: &str) -> anyhow::Result<()> {
            self.inner.archive_conversation(id)
        }
        fn restore_conversation(&self, id: &str) -> anyhow::Result<()> {
            self.inner.restore_conversation(id)
        }
        fn get_archived_conversations(&self) -> anyhow::Result<Vec<serde_json::Value>> {
            self.inner.get_archived_conversations()
        }
    }

    #[test]
    fn sanitize_title_strips_quotes_and_whitespace() {
        assert_eq!(sanitize_title("  「标题」  "), "标题");
        assert_eq!(sanitize_title("\"测试标题\""), "测试标题");
        assert_eq!(sanitize_title("'hello'\nworld"), "hello");
        assert_eq!(sanitize_title(""), "");
    }

    #[test]
    fn sanitize_title_truncates_long_output() {
        let long = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十X";
        let result = sanitize_title(long);
        assert!(result.chars().count() <= 10);
        assert_eq!(result, "一二三四五六七八九十");
    }

    #[test]
    fn sanitize_title_rejects_refusals() {
        assert_eq!(
            sanitize_title("我无法直接访问网页，但可以根据公开信息为你总结 **Reac"),
            ""
        );
        assert_eq!(sanitize_title("抱歉，我无法完成请求"), "");
        assert_eq!(sanitize_title("Sorry, I can't help with that"), "");
        assert_eq!(sanitize_title("I cannot access external URLs"), "");
    }

    #[test]
    fn sanitize_title_strips_markdown_decoration() {
        // # heading marker stripped, then truncated to 10 chars
        assert_eq!(sanitize_title("# React 19 新特性详解"), "React 19 新");
        assert_eq!(sanitize_title("**重要标题**"), "重要标题");
        // bold/italic mid-string and inline code
        assert_eq!(sanitize_title("讨论 **React** 的 `useEffect`"), "讨论 React 的");
        // markdown link [text](url) keeps only the text
        assert_eq!(sanitize_title("[React 文档](https://react.dev)"), "React 文档");
        // underscore italic + tilde strikethrough
        assert_eq!(sanitize_title("_emphasis_ ~strike~"), "emphasis s");
    }

    #[test]
    fn should_auto_title_returns_false_when_already_titled() {
        let store = StoreWithMessages::new(
            "已有标题",
            "conv1",
            vec![
                serde_json::json!({"role": "user",      "content": {"text": "hello"}}),
                serde_json::json!({"role": "assistant",  "content": {"text": "hi"}}),
            ],
        );
        assert!(!should_auto_title(store.as_ref() as &dyn ConversationStore, "conv1").unwrap());
    }

    #[test]
    fn should_auto_title_returns_false_when_no_assistant_message() {
        let store = StoreWithMessages::new(
            "新对话",
            "conv1",
            vec![serde_json::json!({"role": "user", "content": {"text": "hello"}})],
        );
        assert!(!should_auto_title(store.as_ref() as &dyn ConversationStore, "conv1").unwrap());
    }

    #[test]
    fn should_auto_title_returns_true_when_conditions_met() {
        let store = StoreWithMessages::new(
            "新对话",
            "conv1",
            vec![
                serde_json::json!({"role": "user",      "content": {"text": "hello"}}),
                serde_json::json!({"role": "assistant",  "content": {"text": "hi"}}),
            ],
        );
        assert!(should_auto_title(store.as_ref() as &dyn ConversationStore, "conv1").unwrap());
    }
}
