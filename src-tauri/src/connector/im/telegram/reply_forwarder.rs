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

        match &event.kind {
            RuntimeEventKind::MessagePersisted { role, content, .. } => {
                if role != "assistant" {
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
                self.connector
                    .stop_typing(&session_id, event.run_id.as_str())
                    .await;
            }
            RuntimeEventKind::PermissionAskRequired { .. }
            | RuntimeEventKind::UserInteractionRequired { .. }
            | RuntimeEventKind::StreamDone
            | RuntimeEventKind::StreamError { .. }
            | RuntimeEventKind::TurnCompleted { .. }
            | RuntimeEventKind::RunCancelled
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
