//! TelegramReplyForwarder — 监听 RuntimeEventBus，把 AI 输出转发到
//! TelegramConnector::send(AiCardChunk) 的文本编辑流式路径。
//!
//! Telegram Bot API 没有 CardKit，但支持 editMessageText；connector 内部
//! 维护 preview message 状态，避免 per-delta 新发多条消息。
//!
//! 过滤：用 `connector.has_session()` 判定 session 是否属于 telegram；不属于的
//! 事件忽略（同一个 RuntimeEventBus 上挂着多个平台 forwarder，互不影响）。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use super::{api::TelegramApiError, connector::TelegramConnector};
use crate::connector::im::trait_def::{IMConnector, ReplyContent, ReplyTarget};
use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

const TELEGRAM_STARTED_REACTION_CANDIDATES: &[&str] = &["👀"];
const TELEGRAM_DONE_REACTION_CANDIDATES: &[&str] = &["✅", "👍", "🎉", "💯"];
const TELEGRAM_ERROR_REACTION_CANDIDATES: &[&str] = &["❌", "😱", "😨", "🤯"];
const TELEGRAM_REACTION_TEST_FORCE_COMPLETION_ENV: &str =
    "AIJIA_TELEGRAM_REACTION_TEST_FORCE_COMPLETION";

pub struct TelegramReplyForwarder {
    connector: Arc<TelegramConnector>,
}

impl TelegramReplyForwarder {
    pub fn new(connector: Arc<TelegramConnector>) -> Self {
        Self { connector }
    }

    fn should_try_next_reaction_candidate(error: &anyhow::Error) -> bool {
        matches!(
            error.downcast_ref::<TelegramApiError>(),
            Some(TelegramApiError::BadRequest(_))
        )
    }

    fn emoji_codepoints(emoji: &str) -> String {
        use std::fmt::Write as _;

        let mut out = String::new();
        for (idx, ch) in emoji.chars().enumerate() {
            if idx > 0 {
                out.push('+');
            }
            let _ = write!(&mut out, "U+{:04X}", ch as u32);
        }
        out
    }

    #[cfg(debug_assertions)]
    fn completion_reaction_override_value_is_error(value: Option<&str>) -> bool {
        matches!(
            value.map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_ascii_lowercase()),
            Some(v) if matches!(v.as_str(), "error" | "turn-error" | "failed" | "failure")
        )
    }

    fn should_force_completion_error_for_tests() -> bool {
        #[cfg(debug_assertions)]
        {
            let value = std::env::var(TELEGRAM_REACTION_TEST_FORCE_COMPLETION_ENV).ok();
            Self::completion_reaction_override_value_is_error(value.as_deref())
        }

        #[cfg(not(debug_assertions))]
        {
            false
        }
    }

    async fn set_status_reaction(&self, session_id: &str, candidates: &[&str], label: &str) {
        if candidates.is_empty() {
            return;
        }

        for (idx, emoji) in candidates.iter().enumerate() {
            let emoji_codepoints = Self::emoji_codepoints(emoji);
            match self
                .connector
                .react_to_latest_inbound(session_id, Some(emoji))
                .await
            {
                Ok(()) => {
                    if idx > 0 {
                        let requested_codepoints = Self::emoji_codepoints(candidates[0]);
                        log::warn!(
                            "[telegram-reply-forwarder] status reaction {} fallback succeeded (session={} requested={} requested_codepoints={} fallback={} fallback_codepoints={})",
                            label,
                            session_id,
                            candidates[0],
                            requested_codepoints,
                            emoji,
                            emoji_codepoints
                        );
                    }
                    return;
                }
                Err(e) => {
                    let will_try_next =
                        idx + 1 < candidates.len() && Self::should_try_next_reaction_candidate(&e);
                    log::warn!(
                        "[telegram-reply-forwarder] status reaction {} failed (session={} emoji={} emoji_codepoints={} will_try_next={}): {:#}",
                        label,
                        session_id,
                        emoji,
                        emoji_codepoints,
                        will_try_next,
                        e
                    );
                    if !will_try_next {
                        return;
                    }
                }
            }
        }
    }

    async fn clear_status_reaction(&self, session_id: &str, label: &str) {
        if let Err(e) = self
            .connector
            .react_to_latest_inbound(session_id, None)
            .await
        {
            log::warn!(
                "[telegram-reply-forwarder] status reaction {} failed (session={}): {:#}",
                label,
                session_id,
                e
            );
        }
    }

    /// 从 `MessagePersisted.content` 里取 markdown 正文 —— assistant 回复的
    /// `content` 是 `MessageContent` 的 JSON shape (`{ text, codeBlocks, ... }`)，
    /// 这里只关心 `text` 字段。其它富类型（codeBlocks / tables / generatedFiles）
    /// 暂不下发到 Telegram，后续可扩展。
    fn extract_markdown(content: &serde_json::Value) -> Option<String> {
        let t = content.get("text").and_then(|v| v.as_str())?.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for TelegramReplyForwarder {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id = event.session_id.as_str().to_string();

        // 仅处理 telegram 自己 remember_session 过的会话；其它平台的事件直接跳过。
        if !self.connector.has_session(&session_id).await {
            return Ok(());
        }

        let target = || ReplyTarget {
            session_id: session_id.clone(),
            external_conversation_key: String::new(),
        };

        match &event.kind {
            RuntimeEventKind::RunStarted => {
                self.set_status_reaction(
                    &session_id,
                    TELEGRAM_STARTED_REACTION_CANDIDATES,
                    "started",
                )
                .await;
            }
            RuntimeEventKind::StreamDelta { content } => {
                log::debug!(
                    "[telegram-reply-forwarder] event=stream_delta session={} bytes={}",
                    session_id,
                    content.len()
                );
                if let Err(e) = self
                    .connector
                    .send(
                        target(),
                        ReplyContent::AiCardChunk {
                            delta: content.clone(),
                            final_chunk: false,
                        },
                    )
                    .await
                {
                    log::warn!(
                        "[telegram-reply-forwarder] send stream delta failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
            }
            RuntimeEventKind::StreamDone => {
                log::debug!(
                    "[telegram-reply-forwarder] event=stream_done session={}",
                    session_id
                );
                if let Err(e) = self
                    .connector
                    .send(
                        target(),
                        ReplyContent::AiCardChunk {
                            delta: String::new(),
                            final_chunk: true,
                        },
                    )
                    .await
                {
                    log::warn!(
                        "[telegram-reply-forwarder] send stream final failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
                self.connector
                    .stop_typing(&session_id, event.run_id.as_str())
                    .await;
            }
            RuntimeEventKind::StreamError { error, .. } => {
                log::warn!(
                    "[telegram-reply-forwarder] StreamError session={} error={}",
                    session_id,
                    error
                );
                self.set_status_reaction(
                    &session_id,
                    TELEGRAM_ERROR_REACTION_CANDIDATES,
                    "stream-error",
                )
                .await;
                if let Err(e) = self
                    .connector
                    .send(target(), ReplyContent::AiCardFail)
                    .await
                {
                    log::warn!(
                        "[telegram-reply-forwarder] send fail marker failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
                self.connector
                    .stop_typing(&session_id, event.run_id.as_str())
                    .await;
            }
            RuntimeEventKind::TurnCompleted { outcome, .. } if outcome.is_success() => {
                if Self::should_force_completion_error_for_tests() {
                    log::warn!(
                        "[telegram-reply-forwarder] forcing completion error reaction for test (session={} env={})",
                        session_id,
                        TELEGRAM_REACTION_TEST_FORCE_COMPLETION_ENV
                    );
                    self.set_status_reaction(
                        &session_id,
                        TELEGRAM_ERROR_REACTION_CANDIDATES,
                        "test-forced-turn-error",
                    )
                    .await;
                } else {
                    self.set_status_reaction(
                        &session_id,
                        TELEGRAM_DONE_REACTION_CANDIDATES,
                        "done",
                    )
                    .await;
                }
                self.connector
                    .stop_typing(&session_id, event.run_id.as_str())
                    .await;
            }
            RuntimeEventKind::TurnCompleted { outcome, .. } if outcome.is_error() => {
                self.set_status_reaction(
                    &session_id,
                    TELEGRAM_ERROR_REACTION_CANDIDATES,
                    "turn-error",
                )
                .await;
                self.connector
                    .stop_typing(&session_id, event.run_id.as_str())
                    .await;
            }
            RuntimeEventKind::RunCancelled => {
                self.clear_status_reaction(&session_id, "cancelled").await;
                self.connector
                    .stop_typing(&session_id, event.run_id.as_str())
                    .await;
            }
            RuntimeEventKind::MessagePersisted { role, content, .. } => {
                if role != "assistant" {
                    return Ok(());
                }
                if self.connector.has_active_or_recent_draft(&session_id).await {
                    log::debug!(
                        "[telegram-reply-forwarder] event=message_persisted_skip_draft session={}",
                        session_id
                    );
                    self.connector
                        .stop_typing(&session_id, event.run_id.as_str())
                        .await;
                    return Ok(());
                }
                let Some(text) = Self::extract_markdown(content) else {
                    log::debug!(
                        "[telegram-reply-forwarder] empty assistant content for session={}, skip",
                        session_id
                    );
                    self.connector
                        .stop_typing(&session_id, event.run_id.as_str())
                        .await;
                    return Ok(());
                };
                if let Err(e) = self
                    .connector
                    .send(target(), ReplyContent::Markdown(text))
                    .await
                {
                    log::warn!(
                        "[telegram-reply-forwarder] send Markdown fallback failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
                self.connector
                    .stop_typing(&session_id, event.run_id.as_str())
                    .await;
            }
            RuntimeEventKind::PermissionAskRequired { .. }
            | RuntimeEventKind::UserInteractionRequired { .. }
            | RuntimeEventKind::TurnCompleted { .. }
            | RuntimeEventKind::RunCompleted => {
                self.connector
                    .stop_typing(&session_id, event.run_id.as_str())
                    .await;
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_markdown_ignores_empty_text() {
        assert!(
            TelegramReplyForwarder::extract_markdown(&serde_json::json!({ "text": "  " }))
                .is_none()
        );
    }

    #[test]
    fn extract_markdown_returns_trimmed_text() {
        assert_eq!(
            TelegramReplyForwarder::extract_markdown(&serde_json::json!({ "text": " hello " }))
                .as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn status_reaction_candidates_keep_requested_emoji_first() {
        assert_eq!(TELEGRAM_STARTED_REACTION_CANDIDATES, ["👀"]);
        assert_eq!(TELEGRAM_DONE_REACTION_CANDIDATES, ["✅", "👍", "🎉", "💯"]);
        assert_eq!(TELEGRAM_ERROR_REACTION_CANDIDATES, ["❌", "😱", "😨", "🤯"]);
    }

    #[test]
    fn bad_request_can_try_next_reaction_candidate() {
        let error = anyhow::Error::new(TelegramApiError::BadRequest(
            "Bad Request: reaction is unavailable".into(),
        ));
        assert!(TelegramReplyForwarder::should_try_next_reaction_candidate(
            &error
        ));
    }

    #[test]
    fn transport_error_does_not_try_next_reaction_candidate() {
        let error = anyhow::Error::new(TelegramApiError::TransportConnected(
            "connection reset".into(),
        ));
        assert!(!TelegramReplyForwarder::should_try_next_reaction_candidate(
            &error
        ));
    }

    #[test]
    fn emoji_codepoints_are_stable_ascii_for_logs() {
        assert_eq!(TelegramReplyForwarder::emoji_codepoints("✅"), "U+2705");
        assert_eq!(TelegramReplyForwarder::emoji_codepoints("💯"), "U+1F4AF");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn completion_reaction_override_accepts_error_values() {
        for value in ["error", "turn-error", "failed", "failure", " ERROR "] {
            assert!(
                TelegramReplyForwarder::completion_reaction_override_value_is_error(Some(value)),
                "{value} should force error reaction"
            );
        }

        for value in [None, Some(""), Some("done"), Some("success"), Some("nope")] {
            assert!(
                !TelegramReplyForwarder::completion_reaction_override_value_is_error(value),
                "{value:?} should not force error reaction"
            );
        }
    }
}
