use std::sync::Arc;

use serde::Serialize;

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::ChatMessage;
use crate::models::message::SubAgentTranscriptEntryFrontend;
use crate::models::settings::AppSettings;
use crate::runtime::agent::subagent_result_envelope::SubAgentResultEnvelope;
use crate::runtime::agent::AgentRuntime;
use crate::runtime::store::conversation_store::ConversationStore;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::types::ConversationMeta;
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

/// Frontend-facing view of `ConversationMeta`. Exposed by the
/// `get_conversation_meta` Tauri command for routes that need fields
/// not present in the lightweight `ConversationIndexEntry` (e.g.
/// `expert_team_id`, `active_team_name`). Serialized camelCase to match
/// the `Conversation` shape on the TS side.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMetaDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_archived: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_team_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expert_team_id: Option<String>,
}

impl From<ConversationMeta> for ConversationMetaDto {
    fn from(m: ConversationMeta) -> Self {
        // Derive expert_team_id from our tagged source field so callers that
        // still ask for `expertTeamId` (legacy from main) get a sensible value.
        let expert_team_id =
            if let crate::storage::file_store::types::ConversationSource::ExpertTeam {
                expert_team_id,
            } = &m.source
            {
                Some(expert_team_id.clone())
            } else {
                None
            };
        Self {
            id: m.id,
            title: m.title,
            created_at: m.created_at,
            updated_at: m.updated_at,
            is_archived: m.is_archived,
            employee_id: m.employee_id,
            active_team_name: m.active_team_name,
            expert_team_id,
        }
    }
}

pub async fn stop_streaming(
    gateway: Arc<LlmGateway>,
    conversation_id: String,
) -> Result<(), String> {
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

pub async fn delete_conversation(
    db: Arc<AppStorage>,
    gateway: Arc<LlmGateway>,
    file_mgr: Arc<FileManager>,
    conversation_id: String,
) -> Result<DeleteConversationOutcome, String> {
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

/// Visual width of a character: ASCII counts as 1, otherwise 2.
/// Used to cap title length so mixed CJK/Latin titles get equal screen real
/// estate (~16 CJK chars or ~32 ASCII chars).
fn visual_width(c: char) -> usize {
    if c.is_ascii() {
        1
    } else {
        2
    }
}

const TITLE_VISUAL_WIDTH_CAP: usize = 32;

/// Take a leading prefix bounded by visual width. Truncates on a char boundary.
fn take_by_visual_width(s: &str, cap: usize) -> String {
    let mut acc = 0usize;
    let mut out = String::new();
    for c in s.chars() {
        let w = visual_width(c);
        if acc + w > cap {
            break;
        }
        acc += w;
        out.push(c);
    }
    out
}

/// Find the first non-empty trimmed line. Skip common LLM lead-ins like
/// "标题：" / "标题如下：" that prefix the actual title.
fn first_meaningful_line(raw: &str) -> &str {
    const LEAD_INS: &[&str] = &[
        "好的，",
        "好的:",
        "好的：",
        "标题：",
        "标题:",
        "标题如下：",
        "标题如下:",
        "Title:",
        "title:",
    ];
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if LEAD_INS.iter().any(|p| trimmed == *p) {
            continue;
        }
        return trimmed;
    }
    ""
}

/// 从 user 首句生成对话标题：
/// - 取首句（按 `，。?!？！,.\n` 切）
/// - 剥常见礼貌前缀（"请帮我" / "Please " 等）
/// - 截到 32 视觉宽度（≈ 16 中文字 / 32 ASCII）
pub fn title_from_user_text(user_text: &str) -> String {
    if user_text.trim().is_empty() {
        return String::new();
    }
    // 切第一个有内容的句子
    let first_sentence = user_text
        .split(|c: char| matches!(c, '，' | '。' | '?' | '!' | '？' | '！' | ',' | '.' | '\n'))
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .trim();
    if first_sentence.is_empty() {
        return String::new();
    }
    // 剥礼貌前缀
    let polite_prefixes = [
        "请帮我",
        "请帮",
        "请",
        "麻烦",
        "你好",
        "帮我",
        "帮忙",
        "可以",
        "能否",
        "Please ",
        "please ",
        "Can you ",
        "can you ",
        "Could you ",
        "could you ",
    ];
    let mut s = first_sentence.to_string();
    for p in polite_prefixes {
        if s.starts_with(p) {
            s = s[p.len()..].trim().to_string();
            break;
        }
    }
    // 走 sanitize（保留宽度截断 + markdown 剥离 + 引号清理），但跳过 refusal 检测
    // 直接复用 sanitize_title 即可——user 文本不会触发 refusal
    sanitize_title(&s)
}

pub fn sanitize_title(raw: &str) -> String {
    let line = first_meaningful_line(raw);

    // Strip markdown link syntax: [text](url) -> text
    let mut stripped = String::with_capacity(line.len());
    let bytes: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '[' {
            if let Some(close) = bytes[i + 1..].iter().position(|&c| c == ']') {
                let text_end = i + 1 + close;
                if text_end + 1 < bytes.len() && bytes[text_end + 1] == '(' {
                    if let Some(paren_close) = bytes[text_end + 2..].iter().position(|&c| c == ')')
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

    let trimmed_cleaned = cleaned.trim();

    // Refusal detection runs against the full cleaned text (not the truncated
    // candidate), so a long apology like "我无法直接访问网页…" is recognised
    // even when the first 10 chars look benign.
    if looks_like_refusal(trimmed_cleaned) {
        return String::new();
    }

    take_by_visual_width(trimmed_cleaned, TITLE_VISUAL_WIDTH_CAP)
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

pub fn is_generic_auto_title(title: &str) -> bool {
    let normalized = title.trim().to_lowercase();
    matches!(
        normalized.as_str(),
        "new conversation"
            | "conversation"
            | "untitled"
            | "untitled conversation"
            | "新对话"
            | "无标题"
            | "新的对话"
    )
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

    // 只需要 user 消息：早期触发标题生成不必等到 assistant 回复完毕。
    // turn 跑完后会再触发一次，但那时 title 已不是"新对话"，会被这里的 guard 跳过。
    let messages = db.get_messages(conversation_id)?;
    let has_user = messages.iter().any(|m| m["role"].as_str() == Some("user"));
    Ok(has_user)
}

/// Generate and set a title for the conversation.
///
/// 策略：调 LLM 总结 user 首句，失败则兜底到 user 首句字面截断。
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
    // Idempotent guard
    let convs = db.get_conversations()?;
    let current_title = convs
        .iter()
        .find(|c| c["id"].as_str() == Some(conversation_id.as_str()))
        .and_then(|c| c["title"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    if current_title != "新对话" {
        return Ok(None);
    }

    // 取 user 首条非空消息（含附件 / skill 兜底）
    let messages = db.get_messages(&conversation_id)?;
    let extract_text = |m: &serde_json::Value| -> String {
        let raw = m["content"]["text"]
            .as_str()
            .or_else(|| m["content"].as_str())
            .unwrap_or("");

        if raw.trim().is_empty() && m["role"].as_str() == Some("user") {
            // 纯附件 / 纯 skill / 纯粘贴图：从 commandText / skill label / 附件名拼
            let mut parts: Vec<String> = Vec::new();
            if let Some(cmd) = m["content"]["commandText"].as_str() {
                if !cmd.trim().is_empty() {
                    parts.push(cmd.trim().to_string());
                }
            }
            if let Some(label) = m["content"]["skillCommand"]["label"].as_str() {
                if !label.trim().is_empty() && !parts.iter().any(|p| p.contains(label)) {
                    parts.push(label.trim().to_string());
                }
            }
            if let Some(files) = m["content"]["files"].as_array() {
                let names: Vec<String> = files
                    .iter()
                    .filter_map(|f| f["fileName"].as_str())
                    .filter(|n| !n.trim().is_empty())
                    .take(3)
                    .map(|s| s.to_string())
                    .collect();
                if !names.is_empty() {
                    parts.push(format!("附件：{}", names.join("、")));
                }
            }
            return parts.join(" ").chars().take(500).collect();
        }

        raw.chars().take(500).collect()
    };

    let first_user = messages
        .iter()
        .filter(|m| m["role"].as_str() == Some("user"))
        .map(extract_text)
        .find(|s| !s.trim().is_empty())
        .unwrap_or_default();

    if first_user.is_empty() {
        return Ok(None);
    }

    // 先尝试 LLM 总结，失败/空 → fallback 到 user 首句截断
    let title = match try_llm_title(&gateway, &settings, &first_user, &conversation_id).await {
        Ok(t) if !t.is_empty() && !is_generic_auto_title(&t) => t,
        Ok(t) => {
            log::warn!(
                "[auto-title] LLM returned empty/generic title; falling back to user-text. conv={} title={:?}",
                conversation_id,
                t
            );
            title_from_user_text(&first_user)
        }
        Err(e) => {
            log::warn!(
                "[auto-title] LLM call failed ({:#}); falling back to user-text. conv={}",
                e,
                conversation_id
            );
            title_from_user_text(&first_user)
        }
    };

    if title.is_empty() || is_generic_auto_title(&title) {
        log::info!(
            "[auto-title] skipped title update: derived title is empty/generic. conv={} title={:?}",
            conversation_id,
            title
        );
        return Ok(None);
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

async fn try_llm_title(
    gateway: &LlmGateway,
    settings: &AppSettings,
    first_user: &str,
    _conversation_id: &str,
) -> anyhow::Result<String> {
    let llm_messages = vec![ChatMessage::text("user", first_user)];

    let system_prompt = "你是一个对话标题生成器。根据下面的用户消息，生成一个能完整概括主题的简洁标题。\
         **标题语言必须与用户消息的自然语言一致**：用户用中文 → 用中文标题；user writes in English → English title; \
         其他语言同理。即使用户消息只有一个词（如 \"hello\"），也按该词的语言出标题。\
         长度：中文标题 6-16 字、英文标题 2-6 个单词，必须语义完整，不要在词语中间截断。\
         只输出纯文本标题本身，禁止使用任何 Markdown 语法（不要 #、*、_、`、链接、引号或括号），\
         不加结尾标点、不加解释、不加前缀（如\"标题：\"或\"Title:\"）。";

    let response = gateway
        .send_message(
            settings,
            llm_messages,
            MaskingLevel::Relaxed,
            Some(system_prompt),
            None,
            // 必须传空数组（而非 None）：None 会让 gateway 默认注入全部工具，
            // 模型看到工具列表后会用 tool_use 回应（如 Grep 搜索 user 提到的 token）
            // 而不是输出文本，导致 response.content 为空。
            Some(vec![]),
        )
        .await?;

    Ok(sanitize_title(&response.content))
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

pub async fn pin_conversation(
    db: Arc<dyn ConversationStore>,
    conversation_id: String,
    pinned: bool,
) -> Result<(), String> {
    db.set_conversation_pinned(&conversation_id, pinned)
        .map_err(|e| e.to_string())
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
        fn archive_conversation(&self, id: &str) -> anyhow::Result<()> {
            self.inner.archive_conversation(id)
        }
        fn restore_conversation(&self, id: &str) -> anyhow::Result<()> {
            self.inner.restore_conversation(id)
        }
        fn get_archived_conversations(&self) -> anyhow::Result<Vec<serde_json::Value>> {
            self.inner.get_archived_conversations()
        }
        fn set_conversation_pinned(&self, id: &str, pinned: bool) -> anyhow::Result<()> {
            self.inner.set_conversation_pinned(id, pinned)
        }
    }

    #[test]
    fn title_from_user_text_takes_first_sentence() {
        // 你那个例子：长 user 句子取首句作为标题
        assert_eq!(
            title_from_user_text(
                "这个文件夹内有啥, 可以作为我的年中总结的资料吗, 不够的话, 我再去找资料"
            ),
            "这个文件夹内有啥"
        );
    }

    #[test]
    fn title_from_user_text_strips_polite_prefix() {
        assert_eq!(
            title_from_user_text("请帮我分析一下销售数据"),
            "分析一下销售数据"
        );
        assert_eq!(title_from_user_text("麻烦你看下这个 bug"), "你看下这个 bug");
        assert_eq!(
            title_from_user_text("Please review the design"),
            "review the design"
        );
    }

    #[test]
    fn title_from_user_text_truncates_long_input() {
        // 没有句号但句子很长时按视觉宽度截
        let long = "讨论数据库迁移方案的具体实施步骤以及相关的配置改造需要哪些注意事项";
        let result = title_from_user_text(long);
        // 应该截到 16 字内（32 视觉宽度）
        let visual_width: usize = result
            .chars()
            .map(|c| if c.is_ascii() { 1 } else { 2 })
            .sum();
        assert!(visual_width <= 32, "got: {result}");
        assert!(result.starts_with("讨论数据库迁移方案"));
    }

    #[test]
    fn title_from_user_text_handles_empty() {
        assert_eq!(title_from_user_text(""), "");
        assert_eq!(title_from_user_text("   "), "");
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
        // 16 中文字 = 32 视觉宽度，恰好填满；第 17 字被截掉
        let long = "一二三四五六七八九十一二三四五六七八九十";
        let result = sanitize_title(long);
        assert_eq!(result, "一二三四五六七八九十一二三四五六");
    }

    #[test]
    fn sanitize_title_strips_lead_in_prefix() {
        // 模型偶尔输出 "好的，标题如下：\n实际标题"
        assert_eq!(
            sanitize_title("标题：\nReact 19 新特性详解"),
            "React 19 新特性详解"
        );
        assert_eq!(sanitize_title("好的，\n实际标题"), "实际标题");
    }

    #[test]
    fn sanitize_title_keeps_full_chinese_title() {
        // 16 字中文应该完整保留
        assert_eq!(
            sanitize_title("讨论数据库迁移方案的具体实施步骤"),
            "讨论数据库迁移方案的具体实施步骤"
        );
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
        // # heading marker stripped; 16 字内全部保留（不再硬截 10 char）
        assert_eq!(
            sanitize_title("# React 19 新特性详解"),
            "React 19 新特性详解"
        );
        assert_eq!(sanitize_title("**重要标题**"), "重要标题");
        // bold/italic mid-string and inline code
        assert_eq!(
            sanitize_title("讨论 **React** 的 `useEffect`"),
            "讨论 React 的 useEffect"
        );
        // markdown link [text](url) keeps only the text
        assert_eq!(
            sanitize_title("[React 文档](https://react.dev)"),
            "React 文档"
        );
        // underscore italic + tilde strikethrough
        assert_eq!(sanitize_title("_emphasis_ ~strike~"), "emphasis strike");
    }

    #[test]
    fn generic_auto_titles_are_rejected() {
        assert!(is_generic_auto_title("New Conversation"));
        assert!(is_generic_auto_title(" new conversation "));
        assert!(is_generic_auto_title("新对话"));
        assert!(is_generic_auto_title("Untitled"));
        assert!(!is_generic_auto_title("分析销售数据"));
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
    fn should_auto_title_returns_true_when_only_user_present() {
        // 早期触发：只要有 user message 就总结，不必等 assistant
        let store = StoreWithMessages::new(
            "新对话",
            "conv1",
            vec![serde_json::json!({"role": "user", "content": {"text": "hello"}})],
        );
        assert!(should_auto_title(store.as_ref() as &dyn ConversationStore, "conv1").unwrap());
    }

    #[test]
    fn should_auto_title_returns_false_when_no_user_message() {
        // 没有任何 user 消息时不触发（防止空对话被乱总结）
        let store = StoreWithMessages::new("新对话", "conv1", vec![]);
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
