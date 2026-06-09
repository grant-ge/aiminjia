//! `WechatConnector` — `IMConnector` implementation for iLink HTTP API.
//!
//! Phase 5 PR0 was scaffold + login. PR1 (this commit) wires:
//!   - `start()` 起 long-poll worker (`super::runtime::spawn_long_poll`)，
//!     把入站消息以 `ChannelMessage` 形态 push 到 `mpsc::Receiver`，再
//!     `tokio_stream::wrappers::ReceiverStream` 包成 `BoxStream` 返回给 manager。
//!   - `send()` 拿 `session_targets` 缓存的 `to_user_id` + `context_token`，
//!     调 `api::send_message` POST `ilink/bot/sendmessage`。
//!   - `remember_session` / `has_session` 给 manager worker 跟 reply forwarder
//!     用，跟 wecom / feishu 对称。
//!
//! Registration (扫码登录) 路径仍由 manager 直接调
//! `super::registration::begin_registration / poll_registration`，不走 trait
//! 方法 —— trait 的 begin/poll 留 NotSupported。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;

use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    PollRequest, RegistrationBegin, RegistrationPoll, RegistrationRequest, ReplyContent,
    ReplyTarget,
};
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

use super::api;
use super::runtime::{spawn_long_poll, WechatInboundEvent, WorkerConfig};
use super::types::WechatSessionTarget;

pub struct WechatConnector {
    bot_token: String,
    ilink_bot_id: String,
    ilink_user_id: String,
    base_url: String,
    app_id: String,
    client_version: String,
    /// `session_id → WechatSessionTarget` 缓存。manager worker 在
    /// `get_or_create_session` 之后调 `remember_session` 写入；`send()` /
    /// `WechatReplyForwarder` 读取拿 `to_user_id` + `context_token`。
    session_targets: Arc<RwLock<HashMap<String, WechatSessionTarget>>>,
    /// `from_user_id → 最近一次 context_token` 的旁路缓存。入站消息的
    /// context_token 由 `start()` 暴露的 stream pipeline 写入；manager worker
    /// 拿到新 session_id 后调 `remember_session` 时从这里查最新值塞进
    /// `session_targets`。两个 map 分两层是因为入站时还不知道 session_id
    /// （那是 router 在 manager 那侧算的）。
    latest_context_tokens: Arc<RwLock<HashMap<String, String>>>,
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    client: reqwest::Client,
}

impl WechatConnector {
    pub fn new(
        bot_token: String,
        ilink_bot_id: String,
        ilink_user_id: String,
        base_url: String,
        app_id: String,
        client_version: String,
        on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("aijia-wechat-ilink/0.1 (https://github.com/grant-ge/aiminjia)")
            .build()
            .expect("reqwest client builder");
        Self {
            bot_token,
            ilink_bot_id,
            ilink_user_id,
            base_url,
            app_id,
            client_version,
            session_targets: Arc::new(RwLock::new(HashMap::new())),
            latest_context_tokens: Arc::new(RwLock::new(HashMap::new())),
            on_status,
            client,
        }
    }

    pub fn ilink_bot_id(&self) -> &str {
        &self.ilink_bot_id
    }

    /// Manager worker uses this to remember `(to_user_id, context_token)` per
    /// session_id —— symmetric with `wecom::WecomConnector::remember_session`。
    /// 这里特意接受**不含 context_token** 的 target，由本方法从
    /// `latest_context_tokens[to_user_id]` 拉最新的一次 token 填进去。
    pub async fn remember_session(&self, session_id: String, mut target: WechatSessionTarget) {
        if target.context_token.is_none() {
            target.context_token = self
                .latest_context_tokens
                .read()
                .await
                .get(&target.to_user_id)
                .cloned();
        }
        self.session_targets
            .write()
            .await
            .insert(session_id, target);
    }

    /// `WechatReplyForwarder` filter — skip events whose session isn't ours.
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.session_targets.read().await.contains_key(session_id)
    }

    async fn resolve_target(&self, mut target: ReplyTarget) -> ReplyTarget {
        if target.external_conversation_key.is_empty() {
            if let Some(t) = self.session_targets.read().await.get(&target.session_id) {
                target.external_conversation_key = t.to_user_id.clone();
            }
        }
        target
    }
}

#[async_trait]
impl IMConnector for WechatConnector {
    fn platform(&self) -> Platform {
        Platform::Wechat
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_text_streaming: false,
            outbound_markdown: false,
            supports_attachments: true,
            supports_group_chat: false,
            supports_private_chat: true,
            auth_flow: AuthFlow::QRCode,
        }
    }

    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let (msg_tx, raw_rx) = mpsc::channel::<WechatInboundEvent>(256);
        let cfg = WorkerConfig {
            base_url: self.base_url.clone(),
            bot_token: self.bot_token.clone(),
            app_id: self.app_id.clone(),
            client_version: self.client_version.clone(),
            ilink_bot_id: self.ilink_bot_id.clone(),
            self_user_id: self.ilink_user_id.clone(),
        };
        let on_status = Arc::clone(&self.on_status);
        spawn_long_poll(cfg, msg_tx, on_status, ctx.cancel_token.clone());

        // 把 WechatInboundEvent stream 拆成 ChannelMessage stream，副作用是
        // 把 context_token 写到 connector 的 `latest_context_tokens` 旁路 +
        // 对所有匹配 from_user_id 的现有 session 也刷新 token。manager
        // worker 第一次见到这个 from_user_id 时调 `remember_session` 自然
        // 会从旁路里把 token 复制进 session_targets。
        let latest = Arc::clone(&self.latest_context_tokens);
        let sessions = Arc::clone(&self.session_targets);
        let stream = ReceiverStream::new(raw_rx).then(move |event| {
            let latest = Arc::clone(&latest);
            let sessions = Arc::clone(&sessions);
            async move {
                let from = event.message.conversation_key.clone();
                if let Some(tok) = event.context_token.clone() {
                    latest.write().await.insert(from.clone(), tok);
                }
                {
                    let mut guard = sessions.write().await;
                    for target in guard.values_mut() {
                        if target.to_user_id == from {
                            target.context_token = event.context_token.clone();
                        }
                    }
                }
                event.message
            }
        });
        Ok(stream.boxed())
    }

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
        let target = self.resolve_target(target).await;
        if target.external_conversation_key.is_empty() {
            return Err(ConnectorError::Fatal(format!(
                "WechatConnector::send no session target for {}",
                target.session_id
            )));
        }
        let text = match content {
            ReplyContent::Text(t) | ReplyContent::Markdown(t) => t,
            ReplyContent::AiCardChunk {
                delta: _,
                final_chunk: _,
            } => {
                // wechat outbound_aicard=false：流式 chunk 不投递，等
                // MessagePersisted 的整段 markdown 来。
                return Ok(());
            }
            ReplyContent::AiCardFail => "❌ 处理失败，请重试".to_string(),
        };
        if text.trim().is_empty() {
            // 同 wecom：不发空内容，避免 iLink 退 400 之类。
            return Ok(());
        }

        let context_token = {
            let guard = self.session_targets.read().await;
            guard
                .get(&target.session_id)
                .and_then(|t| t.context_token.clone())
        };

        let client_id = format!("aijia-{}", uuid::Uuid::new_v4().simple());
        let req = api::build_text_send_req(
            &target.external_conversation_key,
            &text,
            &client_id,
            context_token.as_deref(),
            &self.client_version,
        );
        api::send_message(
            &self.client,
            &self.base_url,
            &self.bot_token,
            &self.app_id,
            &self.client_version,
            &req,
        )
        .await
        .map_err(|e| ConnectorError::Transient(format!("wechat send: {e:#}")))
    }

    async fn begin_registration(
        &self,
        _req: &RegistrationRequest,
    ) -> Result<RegistrationBegin, ConnectorError> {
        Err(ConnectorError::NotSupported(
            "wechat trait-level begin_registration (use manager.begin_wechat_registration)",
        ))
    }

    async fn poll_registration(
        &self,
        _req: &PollRequest,
    ) -> Result<RegistrationPoll, ConnectorError> {
        Err(ConnectorError::NotSupported(
            "wechat trait-level poll_registration (use manager.poll_wechat_registration)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_connector() -> WechatConnector {
        WechatConnector::new(
            "tk-test".into(),
            "bot-test".into(),
            "wxid_self".into(),
            "https://example.com".into(),
            "bot".into(),
            "0.5.30".into(),
            Arc::new(|_state, _err| {}),
        )
    }

    #[test]
    fn platform_is_wechat() {
        assert_eq!(make_connector().platform(), Platform::Wechat);
    }

    #[test]
    fn capabilities_match_mvp_shape() {
        let c = make_connector().capabilities();
        assert_eq!(c.auth_flow, AuthFlow::QRCode);
        assert!(!c.supports_group_chat);
        assert!(c.supports_private_chat);
        assert!(c.supports_attachments);
        assert!(!c.outbound_aicard);
        assert!(!c.outbound_markdown);
    }

    #[tokio::test]
    async fn send_without_known_session_returns_fatal() {
        let c = make_connector();
        let err = c
            .send(
                ReplyTarget {
                    session_id: "missing".into(),
                    external_conversation_key: String::new(),
                },
                ReplyContent::Text("hi".into()),
            )
            .await
            .unwrap_err();
        match err {
            ConnectorError::Fatal(msg) => {
                assert!(
                    msg.contains("no session target") && msg.contains("missing"),
                    "expected Fatal/no session target, got: {msg}"
                );
            }
            other => panic!("expected Fatal, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn remember_session_inserts_target() {
        let c = make_connector();
        c.remember_session(
            "sess-1".into(),
            WechatSessionTarget {
                to_user_id: "wxid_alice".into(),
                context_token: Some("ctx-1".into()),
            },
        )
        .await;
        assert!(c.has_session("sess-1").await);
        assert!(!c.has_session("nope").await);
    }

    #[tokio::test]
    async fn remember_session_pulls_latest_context_token_from_sidecar() {
        let c = make_connector();
        // First simulate a stream event with context_token for wxid_alice.
        c.latest_context_tokens
            .write()
            .await
            .insert("wxid_alice".into(), "ctx-1".into());
        // Now manager-side remember_session WITHOUT context_token — should be
        // filled from latest_context_tokens automatically.
        c.remember_session(
            "sess-1".into(),
            WechatSessionTarget {
                to_user_id: "wxid_alice".into(),
                context_token: None,
            },
        )
        .await;
        let guard = c.session_targets.read().await;
        assert_eq!(
            guard.get("sess-1").unwrap().context_token.as_deref(),
            Some("ctx-1")
        );
    }
}
