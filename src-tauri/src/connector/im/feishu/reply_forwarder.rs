//! FeishuReplyForwarder — 订阅 RuntimeEventBus，把 AI 输出转发到 FeishuConnector::send(AiCardChunk)
//!
//! 钉钉走 DingtalkReplyManager 直接调钉钉 SDK 投放。飞书的等价物是 CardKitSender，
//! 已经封装在 FeishuConnector::send(ReplyContent::AiCardChunk) 里——这里只做"bus 事件 →
//! connector.send" 的桥接。
//!
//! 过滤：用 connector.has_session() 判断 session 是否属于飞书；不属于的事件忽略。
//! 这样订阅者挂上 RuntimeEventBus 后跟钉钉的 reply_manager 互不影响。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

use super::connector::FeishuConnector;
use crate::connector::im::trait_def::{IMConnector, ReplyContent, ReplyTarget};

pub struct FeishuReplyForwarder {
    connector: Arc<FeishuConnector>,
}

impl FeishuReplyForwarder {
    pub fn new(connector: Arc<FeishuConnector>) -> Self {
        Self { connector }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for FeishuReplyForwarder {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id = event.session_id.as_str().to_string();

        // 仅处理飞书自己 remember_session 过的会话；钉钉/普通桌面会话直接跳过。
        if !self.connector.has_session(&session_id).await {
            return Ok(());
        }

        let target = ReplyTarget {
            session_id: session_id.clone(),
            external_conversation_key: String::new(),
        };

        match &event.kind {
            RuntimeEventKind::StreamDelta { content } => {
                if let Err(e) = self
                    .connector
                    .send(
                        target,
                        ReplyContent::AiCardChunk {
                            delta: content.clone(),
                            final_chunk: false,
                        },
                    )
                    .await
                {
                    log::warn!(
                        "[feishu-reply-forwarder] send AiCardChunk delta failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
            }
            RuntimeEventKind::StreamDone => {
                if let Err(e) = self
                    .connector
                    .send(
                        target,
                        ReplyContent::AiCardChunk {
                            delta: String::new(),
                            final_chunk: true,
                        },
                    )
                    .await
                {
                    log::warn!(
                        "[feishu-reply-forwarder] send AiCardChunk final failed (session={}): {:?}",
                        session_id,
                        e
                    );
                }
            }
            RuntimeEventKind::StreamError { error, .. } => {
                log::warn!(
                    "[feishu-reply-forwarder] StreamError session={} error={}",
                    session_id,
                    error
                );
                if let Err(e) = self.connector.send(target, ReplyContent::AiCardFail).await {
                    log::warn!(
                        "[feishu-reply-forwarder] send AiCardFail failed (session={}): {:?}",
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
