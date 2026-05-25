//! WecomReplyForwarder — 订阅 RuntimeEventBus，把 AI 完整回复转发到
//! WecomConnector::send(Markdown)。
//!
//! 跟飞书 / 钉钉的差别：
//! - 钉钉走 `DingtalkReplyManager` 直接 SDK 投放；
//! - 飞书走 `FeishuReplyForwarder` 把 `StreamDelta` 流式 push 到 CardKit；
//! - 企微 aibot 不支持类似 CardKit 的实时 patch 协议（`outbound_aicard=false`），
//!   只能用 markdown 主动消息。如果 per-delta 调 `Sender::send_markdown` 会
//!   导致每个 chunk 都新发一条消息，体验非常差。改为监听 `MessagePersisted`：
//!   后端把 assistant 完整回复持久化后会发这条事件（参见 `chat_turn_driver`
//!   的 `persist_assistant_message`），此时一次性把整段 markdown 投放出去，
//!   对应 openclaw `wecom-openclaw-plugin/src/channel.ts:48` 的
//!   `wsClient.sendMessage(chatId, { msgtype: 'markdown', markdown })`。
//!
//! 过滤：用 `connector.has_session()` 判定 session 是否属于企微；不属于的
//! 事件忽略。这样订阅者挂上 RuntimeEventBus 后跟飞书 / 钉钉 互不影响。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

use super::connector::WecomConnector;
use crate::connector::im::trait_def::{IMConnector, ReplyContent, ReplyTarget};

pub struct WecomReplyForwarder {
    connector: Arc<WecomConnector>,
}

impl WecomReplyForwarder {
    pub fn new(connector: Arc<WecomConnector>) -> Self {
        Self { connector }
    }

    /// 从 `MessagePersisted.content` 里取 markdown 正文 —— assistant 回复的
    /// `content` 是 `MessageContent` 的 JSON shape (`{ text, codeBlocks, ... }`)，
    /// 这里只关心 `text` 字段。其它富类型（codeBlocks / tables / generatedFiles）
    /// 暂不下发到企微（aibot markdown 不能直接渲染），后续可扩展。
    fn extract_markdown(content: &serde_json::Value) -> Option<String> {
        let t = content.get("text").and_then(|v| v.as_str())?;
        let t = t.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for WecomReplyForwarder {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id = event.session_id.as_str().to_string();

        // 仅处理企微自己 remember_session 过的会话；其它平台的事件直接跳过。
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
                        "[wecom-reply-forwarder] empty assistant content for session={}, skip",
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
                        "[wecom-reply-forwarder] send Markdown failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
            }
            // 跟飞书路径不对称：企微不支持流式 patch，所以 StreamDelta /
            // StreamDone 都不处理，等 MessagePersisted 一次性发整段。
            // StreamError 也忽略 —— 没有"撤回"协议，发个"❌ 处理失败"反而
            // 会让用户看到双消息（先 partial 再 fail）��等 MessagePersisted
            // 自然不来 / 内容为空就好。
            _ => {}
        }
        Ok(())
    }
}
