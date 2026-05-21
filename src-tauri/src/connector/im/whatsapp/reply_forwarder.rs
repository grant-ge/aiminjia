//! WhatsAppReplyForwarder — 镜像 TelegramReplyForwarder：监听 MessagePersisted →
//! 一次性发整段 markdown（strip_to_wa 转 WhatsApp 受限格式）到对应 jid。
//!
//! WhatsApp 不支持 ai card / 实时 patch 协议（PR6 走的是占位 + edit_message 路径，
//! 由 AiCardChunk 触发；MessagePersisted 是最终完整结果，单独走文本送达）。
//!
//! 过滤：用 `connector.has_session()` 判定 session 是否属于 WhatsApp；不属于的
//! 事件忽略（同一个 RuntimeEventBus 上挂着多个平台 forwarder，互不影响）。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use super::connector::WhatsAppConnector;
use crate::connector::im::trait_def::{IMConnector, ReplyContent, ReplyTarget};
use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

pub struct WhatsAppReplyForwarder {
    connector: Arc<WhatsAppConnector>,
}

impl WhatsAppReplyForwarder {
    pub fn new(connector: Arc<WhatsAppConnector>) -> Self {
        Self { connector }
    }

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
impl RuntimeEventSubscriber for WhatsAppReplyForwarder {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id = event.session_id.as_str().to_string();

        // 仅处理 WhatsApp 自己 remember_inbound 过的会话；其它平台事件跳过。
        if !self.connector.has_session(&session_id).await {
            return Ok(());
        }

        if let RuntimeEventKind::MessagePersisted { role, content, .. } = &event.kind {
            if role != "assistant" {
                return Ok(());
            }
            let Some(text) = Self::extract_markdown(content) else {
                log::debug!(
                    "[whatsapp-reply-forwarder] empty assistant content for session={}, skip",
                    session_id
                );
                return Ok(());
            };
            // 反查 chat_jid。session_inbound 是 PR6 加的，由 manager worker 在每条
            // 入站消息时写入；正常情况下 has_session=true 必然能查到。
            let Some(chat_jid) = self.connector.lookup_chat_jid(&session_id).await else {
                log::warn!(
                    "[whatsapp-reply-forwarder] session={} has_session=true but jid missing",
                    session_id
                );
                return Ok(());
            };
            let target = ReplyTarget {
                session_id: session_id.clone(),
                external_conversation_key: chat_jid,
            };
            if let Err(e) = self
                .connector
                .send(target, ReplyContent::Markdown(text))
                .await
            {
                log::warn!(
                    "[whatsapp-reply-forwarder] send Markdown failed (session={}): {:?}",
                    session_id,
                    e
                );
            }
        }
        Ok(())
    }
}
