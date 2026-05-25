//! WechatReplyForwarder — 订阅 RuntimeEventBus，监听 `MessagePersisted`
//! 把 assistant 完整 markdown 通过 `WechatConnector::send(Markdown)` 投递。
//!
//! 跟 wecom 的差别就 capability: wechat 也走 markdown 主动发，但 iLink
//! `sendMessage` 没有 markdown 类型；我们的 connector 把 ReplyContent::Markdown
//! 当成 plain text 一次性 POST 出去（emoji / 中文都 OK，markdown 语法在微信
//! 客户端会作为纯文本展示，PR2 加 sendmedia / markdown 渲染时再细化）。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

use super::connector::WechatConnector;
use crate::connector::im::trait_def::{IMConnector, ReplyContent, ReplyTarget};

pub struct WechatReplyForwarder {
    connector: Arc<WechatConnector>,
}

impl WechatReplyForwarder {
    pub fn new(connector: Arc<WechatConnector>) -> Self {
        Self { connector }
    }

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
impl RuntimeEventSubscriber for WechatReplyForwarder {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id = event.session_id.as_str().to_string();

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
                        "[wechat-reply-forwarder] empty assistant content for session={}, skip",
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
                        "[wechat-reply-forwarder] send failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
            }
            // iLink 没有 CardKit 之类的流式更新，StreamDelta/StreamDone/Error 都
            // 不动 —— 等 MessagePersisted 一次性发整段（同 wecom）。
            _ => {}
        }
        Ok(())
    }
}
