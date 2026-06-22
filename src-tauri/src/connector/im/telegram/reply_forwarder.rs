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

use super::connector::TelegramConnector;
use crate::connector::im::trait_def::{IMConnector, ReplyContent, ReplyTarget};
use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

pub struct TelegramReplyForwarder {
    connector: Arc<TelegramConnector>,
}

impl TelegramReplyForwarder {
    pub fn new(connector: Arc<TelegramConnector>) -> Self {
        Self { connector }
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
            }
            RuntimeEventKind::StreamError { error, .. } => {
                log::warn!(
                    "[telegram-reply-forwarder] StreamError session={} error={}",
                    session_id,
                    error
                );
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
                    return Ok(());
                }
                let Some(text) = Self::extract_markdown(content) else {
                    log::debug!(
                        "[telegram-reply-forwarder] empty assistant content for session={}, skip",
                        session_id
                    );
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
            }
            _ => {}
        }
        Ok(())
    }
}
