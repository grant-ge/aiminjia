//! `WecomConnector` —— 实现 `IMConnector`，把 AibotClient 适配到 trait 中性层。
//!
//! PR5 (Phase 2):
//! - `start()` spawns the AibotClient run-loop + an event-pump task that maps
//!   `AibotEvent` → `ChannelMessage` for inbound user messages and
//!   `ChannelConnectionState` for connection lifecycle.
//! - `send()` routes `Text`/`Markdown` through the `Sender` (respond_msg when a
//!   fresh `req_id` is cached, else send_msg fallback). `AiCardChunk` /
//!   `AiCardFail` are downgraded to markdown via the shared `AiCardFallbackBuffer`
//!   (wecom 不支持 AI 卡片协议).
//! - `with_status_callback` mirrors `FeishuConnector` so the manager can drive
//!   `channel:platform-state` emission from real WS lifecycle events.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_stream::wrappers::ReceiverStream;

use super::aibot_client::{AibotClient, AibotClientConfig, AibotEvent};
use super::parser::{parse_inbound, ParsedInbound};
use super::sender::{Sender, SessionMap};
use super::types::WecomSessionTarget;
use crate::connector::im::shared::aicard_fallback::{AiCardFallbackBuffer, FallbackAction};
use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

pub struct WecomConnector {
    bot_id: String,
    aibot: Arc<AibotClient>,
    sender: Sender<AibotClient>,
    /// session_id → 流式 buffer（一次 AI 回复用一个 buffer 实例，final 时移除）
    fallback_buffers: Arc<Mutex<HashMap<String, AiCardFallbackBuffer>>>,
    /// 每会话的回复目标，由 manager worker 在收到首条入站消息时调用
    /// `remember_session` 写入；`WecomReplyForwarder` / `send()` 通过它把
    /// `session_id` 翻译为 `chat_id`（aibot 主动发消息需要的目标），对应
    /// 飞书侧的 `session_targets`。
    session_targets: Arc<RwLock<HashMap<String, WecomSessionTarget>>>,
    /// Streams real WS lifecycle states back to ChannelManager. `Connected`
    /// must mean the aibot subscribe ack arrived (Authenticated event), not
    /// merely that the run-loop task was spawned. Mirrors `FeishuConnector`.
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
}

impl WecomConnector {
    pub fn new(bot_id: String, secret: String) -> Self {
        Self::with_status_callback(bot_id, secret, Arc::new(|_state, _err| {}))
    }

    pub fn with_status_callback(
        bot_id: String,
        secret: String,
        on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    ) -> Self {
        let aibot = Arc::new(AibotClient::new(AibotClientConfig::production(
            bot_id.clone(),
            secret,
        )));
        let sessions = SessionMap::new(Duration::from_secs(300));
        let sender = Sender::new(aibot.clone(), sessions);
        Self {
            bot_id,
            aibot,
            sender,
            fallback_buffers: Arc::new(Mutex::new(HashMap::new())),
            session_targets: Arc::new(RwLock::new(HashMap::new())),
            on_status,
        }
    }

    /// Test-only ctor that lets integration tests inject a custom `AibotClient`
    /// (typically built with a mock ws_url pointing at a local TcpListener).
    /// Production code never calls this — use `new` / `with_status_callback`.
    #[doc(hidden)]
    pub fn for_test(aibot: Arc<AibotClient>) -> Self {
        let sessions = SessionMap::new(Duration::from_secs(300));
        let sender = Sender::new(aibot.clone(), sessions);
        Self {
            bot_id: "TEST-BOT".into(),
            aibot,
            sender,
            fallback_buffers: Arc::new(Mutex::new(HashMap::new())),
            session_targets: Arc::new(RwLock::new(HashMap::new())),
            on_status: Arc::new(|_state, _err| {}),
        }
    }

    /// Caller-visible bot_id; symmetric with feishu's `app_id` accessor.
    pub fn bot_id(&self) -> &str {
        &self.bot_id
    }

    /// 由 manager worker 在 `get_or_create_session` 之后调用，让 connector 记住
    /// `session_id → chat_id` 映射。后续 `WecomReplyForwarder` 可以靠 `has_session`
    /// 判定事件归属，`send()` 在 `ReplyTarget.external_conversation_key` 为空
    /// 时也能回退到 `session_targets` 取 `chat_id`。
    pub async fn remember_session(&self, session_id: String, target: WecomSessionTarget) {
        self.session_targets
            .write()
            .await
            .insert(session_id, target);
    }

    /// `WecomReplyForwarder` 用它过滤掉非 wecom 自己 remember 过的会话 ——
    /// 钉钉/飞书/桌面会话的事件不应该被回投到企微。
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.session_targets.read().await.contains_key(session_id)
    }

    async fn handle_aicard_chunk(
        &self,
        target: &ReplyTarget,
        delta: &str,
        final_chunk: bool,
    ) -> Result<(), ConnectorError> {
        let mut buffers = self.fallback_buffers.lock().await;
        let buf = buffers
            .entry(target.session_id.clone())
            .or_insert_with(|| AiCardFallbackBuffer::new(Duration::from_secs(240)));
        let action = buf.observe(delta, final_chunk);
        drop(buffers);

        match action {
            FallbackAction::Buffer => Ok(()),
            FallbackAction::SendPlaceholder { text } => self
                .sender
                .send_markdown(target, &text)
                .await
                .map_err(|e| ConnectorError::Transient(format!("{e:#}"))),
            FallbackAction::SendFinal { text } => {
                let r = self
                    .sender
                    .send_markdown(target, &text)
                    .await
                    .map_err(|e| ConnectorError::Transient(format!("{e:#}")));
                self.fallback_buffers
                    .lock()
                    .await
                    .remove(&target.session_id);
                r
            }
        }
    }
}

#[async_trait]
impl IMConnector for WecomConnector {
    fn platform(&self) -> Platform {
        Platform::Wecom
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_text_streaming: false,
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::ApiKey,
        }
    }

    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let (msg_tx, msg_rx) = mpsc::channel::<ChannelMessage>(256);
        let (evt_tx, mut evt_rx) = mpsc::channel::<AibotEvent>(64);

        // Surface "we're trying" to the manager before the WS handshake — the
        // Authenticated event below upgrades this to Connected. Mirrors how
        // FeishuConnector::start() lets FeishuStreamClient emit the real
        // Connected state from inside the WS callback.
        (self.on_status)(ChannelConnectionState::Connecting, None);

        let aibot = self.aibot.clone();
        let cancel = ctx.cancel_token.clone();
        tokio::spawn(async move {
            let _ = aibot.run(evt_tx, cancel).await;
        });

        let bot_id = self.bot_id.clone();
        let sessions = self.sender.sessions().clone();
        let on_status = Arc::clone(&self.on_status);
        tokio::spawn(async move {
            while let Some(evt) = evt_rx.recv().await {
                match evt {
                    AibotEvent::Authenticated => {
                        log::info!("[wecom-{}] authenticated", bot_id);
                        on_status(ChannelConnectionState::Connected, None);
                    }
                    AibotEvent::Inbound(frame) => {
                        let req_id = frame.headers.req_id.clone();
                        if let Some(parsed) = parse_inbound(&bot_id, &frame) {
                            if let ParsedInbound::Message(msg) = parsed {
                                // session_id 用 conversation_key (chatid for group,
                                // userid for single) — record fresh req_id so a
                                // follow-up send() can pick respond_msg over send_msg.
                                sessions.record(&msg.conversation_key, &req_id).await;
                                if msg_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    AibotEvent::KickedOut(reason) => {
                        log::warn!("[wecom-{}] kicked out: {reason}", bot_id);
                        // disconnected_event is a server-side "you're done" —
                        // map to NeedsReauth so the user sees the explicit
                        // re-register prompt rather than an auto-reconnect
                        // spinner.
                        on_status(ChannelConnectionState::NeedsReauth, Some(reason));
                        break;
                    }
                    AibotEvent::AuthFailed(code, msg) => {
                        log::error!("[wecom-{}] auth failed code={code} msg={msg}", bot_id);
                        on_status(
                            ChannelConnectionState::ConfigError,
                            Some(format!("auth failed: code={code} msg={msg}")),
                        );
                        // Note: AibotClient::run handles auth retries internally
                        // up to max_auth_failure_attempts. We surface state for
                        // each attempt; the run-loop exit ends the stream.
                    }
                    AibotEvent::ConnectionDropped(reason) => {
                        log::info!("[wecom-{}] connection dropped: {reason}", bot_id);
                        on_status(ChannelConnectionState::Reconnecting, Some(reason));
                    }
                    AibotEvent::Reconnecting(n) => {
                        log::info!("[wecom-{}] reconnecting attempt {n}", bot_id);
                        on_status(
                            ChannelConnectionState::Reconnecting,
                            Some(format!("reconnecting attempt {n}")),
                        );
                    }
                }
            }
        });

        Ok(ReceiverStream::new(msg_rx).boxed())
    }

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
        // Reply forwarder 走"事件总线 → 平台" 路径时只能拿到 `session_id`，
        // `external_conversation_key` 留空 —— 用 connector 自己缓存的
        // `session_targets` 还原。manager 的 dispatch 路径会传完整 target，
        // 直接保留。这两条路径都不破坏对方。
        let target = self.resolve_target(target).await;
        match content {
            ReplyContent::Text(t) | ReplyContent::Markdown(t) => self
                .sender
                .send_markdown(&target, &t)
                .await
                .map_err(|e| ConnectorError::Transient(format!("{e:#}"))),
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                self.handle_aicard_chunk(&target, &delta, final_chunk).await
            }
            ReplyContent::AiCardFail => self
                .sender
                .send_markdown(&target, "❌ 处理失败，请重试")
                .await
                .map_err(|e| ConnectorError::Transient(format!("{e:#}"))),
        }
    }
}

impl WecomConnector {
    async fn resolve_target(&self, mut target: ReplyTarget) -> ReplyTarget {
        if target.external_conversation_key.is_empty() {
            if let Some(t) = self.session_targets.read().await.get(&target.session_id) {
                target.external_conversation_key = t.chat_id.clone();
            }
        }
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_is_wecom() {
        let c = WecomConnector::new("BOTID".into(), "SECRET".into());
        assert_eq!(c.platform(), Platform::Wecom);
    }

    #[test]
    fn capabilities_reports_stream_markdown_attachments_apikey() {
        let c = WecomConnector::new("BOTID".into(), "SECRET".into());
        let caps = c.capabilities();
        assert!(matches!(caps.inbound, InboundModel::Stream));
        assert!(!caps.outbound_aicard, "wecom uses markdown fallback");
        assert!(caps.outbound_markdown);
        assert!(caps.supports_attachments);
        assert!(matches!(caps.auth_flow, AuthFlow::ApiKey));
    }

    #[test]
    fn bot_id_accessor_returns_stored_value() {
        let c = WecomConnector::new("MY-BOT".into(), "SECRET".into());
        assert_eq!(c.bot_id(), "MY-BOT");
    }
}
