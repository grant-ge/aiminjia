//! TelegramReplyForwarder — 镜像 WecomReplyForwarder：监听 MessagePersisted →
//! 一次性发整段 markdown 到 Telegram chat。
//!
//! 跟 wecom 一样：Telegram Bot API 不支持 CardKit / 实时 patch 协议，per-delta
//! 发 `sendMessage` 会变成"每个 chunk 一条消息"的灾难，所以只处理
//! `MessagePersisted`——assistant 完整回复持久化后一次性下发 markdown。
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

    async fn set_status_reaction(&self, session_id: &str, candidates: &[&str], label: &str) {
        if candidates.is_empty() {
            return;
        }

        for (idx, emoji) in candidates.iter().enumerate() {
            match self
                .connector
                .react_to_latest_inbound(session_id, Some(emoji))
                .await
            {
                Ok(()) => {
                    if idx > 0 {
                        log::warn!(
                            "[telegram-reply-forwarder] status reaction {} fallback succeeded (session={} requested={} fallback={})",
                            label,
                            session_id,
                            candidates[0],
                            emoji
                        );
                    }
                    return;
                }
                Err(e) => {
                    let will_try_next =
                        idx + 1 < candidates.len() && Self::should_try_next_reaction_candidate(&e);
                    log::warn!(
                        "[telegram-reply-forwarder] status reaction {} failed (session={} emoji={} will_try_next={}): {:#}",
                        label,
                        session_id,
                        emoji,
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

        match &event.kind {
            RuntimeEventKind::RunStarted => {
                self.set_status_reaction(
                    &session_id,
                    TELEGRAM_STARTED_REACTION_CANDIDATES,
                    "started",
                )
                .await;
            }
            RuntimeEventKind::StreamError { error, .. } => {
                log::debug!(
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
            }
            RuntimeEventKind::TurnCompleted { outcome, .. } if outcome.is_success() => {
                self.set_status_reaction(&session_id, TELEGRAM_DONE_REACTION_CANDIDATES, "done")
                    .await;
            }
            RuntimeEventKind::TurnCompleted { outcome, .. } if outcome.is_error() => {
                self.set_status_reaction(
                    &session_id,
                    TELEGRAM_ERROR_REACTION_CANDIDATES,
                    "turn-error",
                )
                .await;
            }
            RuntimeEventKind::RunCancelled => {
                self.clear_status_reaction(&session_id, "cancelled").await;
            }
            RuntimeEventKind::MessagePersisted { role, content, .. } => {
                if role != "assistant" {
                    return Ok(());
                }
                let Some(text) = Self::extract_markdown(content) else {
                    log::debug!(
                        "[telegram-reply-forwarder] empty assistant content for session={}, skip",
                        session_id
                    );
                    return Ok(());
                };
                let target = ReplyTarget {
                    session_id: session_id.clone(),
                    external_conversation_key: String::new(),
                };
                if let Err(e) = self
                    .connector
                    .send(target, ReplyContent::Markdown(text))
                    .await
                {
                    log::warn!(
                        "[telegram-reply-forwarder] send Markdown failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
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
}
