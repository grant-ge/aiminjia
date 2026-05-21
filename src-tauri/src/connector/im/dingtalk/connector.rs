//! `DingtalkConnector` — implements `IMConnector` by delegating to the existing
//! `DingtalkStreamClient` + `DingtalkReplyManager` + registration helpers.
//!
//! In Phase 0 PR4 this is **dead code in production** — `ChannelManager` still
//! drives `DingtalkStreamClient` directly. PR5 will rewire the manager to call
//! `IMConnector::start` and route through this implementation.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::stream::DingtalkStreamClient;
use super::token::TokenCache;
use crate::connector::im::shared::reply_manager::DingtalkReplyManager;
use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    PollRequest, RegistrationBegin, RegistrationPoll, RegistrationRequest, ReplyContent,
    ReplyTarget,
};
use crate::connector::im::types::{
    ChannelConnectionState, ChannelMessage, ChannelRegistrationPollState, Platform,
};

/// Status callback handed to the underlying DingtalkStreamClient. Manager
/// uses this to drive `channel:platform-state` emission. Boxed so the
/// connector can be `Arc<dyn IMConnector>`-erased.
pub type StatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;

/// 钉钉特定的"每会话"回复参数。Manager 在 worker loop 收到一条 inbound 消息后
/// 通过 [`DingtalkConnector::remember_session`] 喂进来，`send(Text|Markdown)`
/// 时按 session_id 查回去 —— 让 trait-level `ReplyTarget` 保持平台中性。
#[derive(Debug, Clone)]
pub struct DingtalkSessionTarget {
    pub robot_code: String,
    pub reply_group_id: String,
    pub session_webhook: Option<String>,
}

/// Build a `DingtalkConnector` ready for registration. Status callbacks fired
/// by the underlying `DingtalkStreamClient` are routed through `on_status`;
/// pass a no-op closure if the caller does not care.
pub struct DingtalkConnector {
    app_key: String,
    app_secret: String,
    robot_code: String,
    reply_manager: Arc<DingtalkReplyManager>,
    #[allow(dead_code)]
    // Held for future direct token operations; reply_manager owns the active TokenCache.
    token_cache: Arc<TokenCache>,
    on_status: StatusCallback,
    /// session_id → 该会话用来 reply 的钉钉特定字段。
    /// Manager 在收到消息后通过 `remember_session` 喂；send() 路径按需查。
    session_targets:
        Arc<tokio::sync::RwLock<std::collections::HashMap<String, DingtalkSessionTarget>>>,
}

impl DingtalkConnector {
    pub fn new(
        app_key: String,
        app_secret: String,
        robot_code: String,
        reply_manager: Arc<DingtalkReplyManager>,
        token_cache: Arc<TokenCache>,
    ) -> Self {
        Self::with_status_callback(
            app_key,
            app_secret,
            robot_code,
            reply_manager,
            token_cache,
            Arc::new(|_state, _err| {}),
        )
    }

    /// Construct a `DingtalkConnector` with a custom status callback. The
    /// callback is forwarded to `DingtalkStreamClient::start` so that the
    /// connector can mirror connection-state changes (Connecting / Connected /
    /// Reconnecting / Disconnected) without taking a `tauri::AppHandle`
    /// dependency.
    pub fn with_status_callback(
        app_key: String,
        app_secret: String,
        robot_code: String,
        reply_manager: Arc<DingtalkReplyManager>,
        token_cache: Arc<TokenCache>,
        on_status: StatusCallback,
    ) -> Self {
        Self {
            app_key,
            app_secret,
            robot_code,
            reply_manager,
            token_cache,
            on_status,
            session_targets: Arc::new(tokio::sync::RwLock::new(Default::default())),
        }
    }

    /// 记下某 session 后续 reply 时所需的钉钉特定字段。
    /// Manager worker loop 在 dedup 后、入队/直发前调用一次。
    pub async fn remember_session(&self, session_id: String, target: DingtalkSessionTarget) {
        self.session_targets
            .write()
            .await
            .insert(session_id, target);
    }
}

#[async_trait]
impl IMConnector for DingtalkConnector {
    fn platform(&self) -> Platform {
        Platform::Dingtalk
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: true,
            outbound_text_streaming: false, // AI Card path; this field is only meaningful when outbound_aicard=false
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::DeviceCode,
        }
    }

    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let (msg_tx, msg_rx) = mpsc::channel::<ChannelMessage>(256);
        let client = DingtalkStreamClient::new(
            self.app_key.clone(),
            self.app_secret.clone(),
            self.robot_code.clone(),
            msg_tx,
        );

        // Forward `on_status` to DingtalkStreamClient. Clone an `Arc` of the
        // closure into a move-capable wrapper that satisfies `Fn(.., ..)`.
        let on_status = Arc::clone(&self.on_status);
        let client_token = client.start(move |state, err| on_status(state, err));

        // Relay external cancel into the stream client's own token.
        let relay_token = ctx.cancel_token.clone();
        let relay_target = client_token.clone();
        tokio::spawn(async move {
            relay_token.cancelled().await;
            relay_target.cancel();
        });

        let stream = ReceiverStream::new(msg_rx).boxed();
        Ok(stream)
    }

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
        match content {
            ReplyContent::Text(text) | ReplyContent::Markdown(text) => {
                let webhook = {
                    let map = self.session_targets.read().await;
                    map.get(&target.session_id)
                        .and_then(|t| t.session_webhook.clone())
                };
                if let Some(webhook) = webhook {
                    super::stream::send_session_webhook_text(webhook, text).await;
                    Ok(())
                } else {
                    Err(ConnectorError::Fatal(format!(
                        "DingtalkConnector::send(Text|Markdown) requires session_webhook (session {})",
                        target.session_id
                    )))
                }
            }
            ReplyContent::AiCardChunk { delta, final_chunk } => self
                .reply_manager
                .dispatch_chunk(&target.session_id, &delta, final_chunk)
                .await
                .map_err(|e| ConnectorError::Transient(format!("aicard chunk: {e:#}"))),
            ReplyContent::AiCardFail => self
                .reply_manager
                .dispatch_fail(&target.session_id)
                .await
                .map_err(|e| ConnectorError::Transient(format!("aicard fail: {e:#}"))),
        }
    }

    async fn stop(&self) -> Result<(), ConnectorError> {
        Ok(())
    }

    async fn begin_registration(
        &self,
        _req: &RegistrationRequest,
    ) -> Result<RegistrationBegin, ConnectorError> {
        let begin = super::registration::begin_registration()
            .await
            .map_err(|e| ConnectorError::Fatal(format!("{e:#}")))?;
        Ok(RegistrationBegin {
            device_code: begin.device_code,
            user_code: begin.user_code,
            verification_uri_complete: begin.verification_uri_complete,
            verification_uri: begin.verification_uri,
            interval_seconds: begin.interval_seconds,
            expires_in_seconds: begin.expires_in_seconds,
            source: begin.source,
        })
    }

    async fn poll_registration(
        &self,
        req: &PollRequest,
    ) -> Result<RegistrationPoll, ConnectorError> {
        let poll = super::registration::poll_registration(&req.device_code)
            .await
            .map_err(|e| ConnectorError::Fatal(format!("{e:#}")))?;
        let state = match poll.state {
            super::registration::RegistrationPollState::Waiting => {
                ChannelRegistrationPollState::Waiting
            }
            super::registration::RegistrationPollState::Success => {
                ChannelRegistrationPollState::Success
            }
            super::registration::RegistrationPollState::Fail => ChannelRegistrationPollState::Fail,
            super::registration::RegistrationPollState::Expired => {
                ChannelRegistrationPollState::Expired
            }
            super::registration::RegistrationPollState::Unknown => {
                ChannelRegistrationPollState::Unknown
            }
        };
        // Phase 0 contract: trait returns the public-facing shape, but does not
        // perform side effects (save_config_and_connect lives in ChannelManager).
        // Manager's poll_dingtalk_registration owns the save-and-connect path;
        // this trait method exposes only the device-code transcript so future
        // platforms can plug their own flow in without duplicating manager state.
        Ok(RegistrationPoll {
            state,
            client_id: poll.client_id,
            robot_code: poll.robot_code,
            config: None,
            platform_state: None,
            fail_reason: poll.fail_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_connector() -> DingtalkConnector {
        DingtalkConnector::new(
            "ak".into(),
            "as".into(),
            "rc".into(),
            Arc::new(DingtalkReplyManager::new()),
            Arc::new(TokenCache::new()),
        )
    }

    #[test]
    fn platform_is_dingtalk() {
        let c = make_connector();
        assert_eq!(c.platform(), Platform::Dingtalk);
    }

    #[test]
    fn capabilities_reports_stream_and_aicard_and_attachments() {
        let c = make_connector();
        let caps = c.capabilities();
        assert!(matches!(caps.inbound, InboundModel::Stream));
        assert!(caps.outbound_aicard);
        assert!(caps.outbound_markdown);
        assert!(caps.supports_attachments);
        assert!(caps.supports_group_chat);
        assert!(caps.supports_private_chat);
        assert!(matches!(caps.auth_flow, AuthFlow::DeviceCode));
    }
}
