//! 出站封装。优先走被动回复（respond_msg，需要还活着的 req_id），否则走主动推送（send_msg）。
//!
//! `SessionMap` 维护 session_id → (req_id, recorded_at)；超过 cache 窗口（默认 5 分钟）
//! 视为 expired，回落到主动推送。req_id 的有效期由 aibot 服务端决定，本期保守取 5 分钟。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use super::aibot_client::AibotClient;
use super::aibot_protocol::{RespondMarkdownBody, SendMsgBody};
use crate::connector::im::trait_def::ReplyTarget;

/// 抽象出来给 sender 测试 mock 用。生产路径直接传 `Arc<AibotClient>`，本 trait 即由
/// AibotClient 实现。
#[async_trait]
pub trait AibotChannel: Send + Sync + 'static {
    async fn respond(&self, req_id: &str, body: Value) -> anyhow::Result<()>;
    async fn send_msg(&self, body: Value) -> anyhow::Result<()>;
}

#[async_trait]
impl AibotChannel for AibotClient {
    async fn respond(&self, req_id: &str, body: Value) -> anyhow::Result<()> {
        // 调 AibotClient 的固有方法（inherent method），避免 trait method recursion。
        AibotClient::respond(self, req_id, body).await
    }
    async fn send_msg(&self, body: Value) -> anyhow::Result<()> {
        AibotClient::send_msg(self, body).await
    }
}

#[derive(Clone)]
pub struct SessionMap {
    inner: Arc<RwLock<HashMap<String, (String, Instant)>>>,
    ttl: Duration,
}

impl SessionMap {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }
    pub async fn record(&self, session_id: &str, req_id: &str) {
        self.inner
            .write()
            .await
            .insert(session_id.to_string(), (req_id.to_string(), Instant::now()));
    }
    pub async fn fresh_req_id(&self, session_id: &str) -> Option<String> {
        let g = self.inner.read().await;
        let (req_id, at) = g.get(session_id)?;
        if at.elapsed() > self.ttl {
            return None;
        }
        Some(req_id.clone())
    }
}

pub struct Sender<C: AibotChannel> {
    channel: Arc<C>,
    sessions: SessionMap,
}

impl<C: AibotChannel> Sender<C> {
    pub fn new(channel: Arc<C>, sessions: SessionMap) -> Self {
        Self { channel, sessions }
    }
    pub fn sessions(&self) -> &SessionMap {
        &self.sessions
    }

    pub async fn send_markdown(&self, target: &ReplyTarget, content: &str) -> anyhow::Result<()> {
        if let Some(req_id) = self.sessions.fresh_req_id(&target.session_id).await {
            let body = serde_json::to_value(RespondMarkdownBody::new(content))?;
            self.channel.respond(&req_id, body).await
        } else {
            let body = serde_json::to_value(SendMsgBody::markdown(
                target.external_conversation_key.clone(),
                content.into(),
            ))?;
            self.channel.send_msg(body).await
        }
    }
}
