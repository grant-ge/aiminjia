//! IMConnector trait — the abstraction every IM platform implementation must
//! satisfy.
//!
//! See `docs/superpowers/specs/2026-05-18-im-connector-trait-phase0-design.md`
//! for the design rationale and the Phase 1+ platform plans (飞书 / 企微 /
//! Telegram / WhatsApp / 个微).

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::shared::ask_coordinator::IMAskCoordinator;
use super::shared::config_store::ChannelConfigStore;
use super::types::{
    ChannelMessage, ChannelRegistrationBeginResult, ChannelRegistrationPollResult, Platform,
};
use crate::runtime::pending::PendingQueueManager;
use crate::storage::crypto::SecureStorage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundModel {
    /// Long-lived push connection (dingtalk / lark). The connector spawns its
    /// own background task and yields events on the returned stream.
    Stream,
    /// HTTP webhook pushed by the platform (wecom).
    /// The connector registers a path with the shared webhook server.
    /// (Telegram uses long-poll = `Stream`; WhatsApp uses WebSocket = `Stream`.)
    Webhook,
    /// External native daemon (wechat). The connector spawns/manages the
    /// daemon process and reads from its stdout/IPC.
    Daemon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFlow {
    DeviceCode,
    OAuth,
    ApiKey,
    QRCode,
}

#[derive(Debug, Clone)]
pub struct ConnectorCapabilities {
    pub inbound: InboundModel,
    pub outbound_aicard: bool,
    /// Connector can stream a single text reply incrementally by editing a
    /// previously sent message (NOT by creating a native AI Card).
    ///
    /// Manager routing: when a connector has `outbound_aicard = true`, the
    /// manager routes `ReplyContent::AiCardChunk` through the native AI Card
    /// path. When `outbound_aicard = false`, the manager checks this field:
    /// if `true`, route as text-edit streaming; if `false`, fall back to
    /// final-only (silent accumulate then send once on `final_chunk`).
    ///
    /// Set by platform:
    /// - dingtalk / feishu: `false` (they have `outbound_aicard = true`,
    ///   so this field is ignored; explicitly `false` for clarity)
    /// - whatsapp: `true` (send_text + edit_message; see Phase 4 spec §6)
    /// - telegram: `true` (sendMessage + editMessageText draft preview)
    /// - wecom / wechat: `false` (final-only, no edit API used)
    pub outbound_text_streaming: bool,
    pub outbound_markdown: bool,
    pub supports_attachments: bool,
    pub supports_group_chat: bool,
    pub supports_private_chat: bool,
    pub auth_flow: AuthFlow,
}

/// Where to deliver an outbound reply. Platform-neutral — connectors look up
/// their own per-session credentials (webhook URL / target conversation /
/// robot_code) by `session_id` from an internal map populated at receive time.
#[derive(Debug, Clone)]
pub struct ReplyTarget {
    pub session_id: String,
    pub external_conversation_key: String,
}

/// Outbound reply payload, normalized so the connector internally decides how
/// to render (aicard / markdown / text / attachment).
#[derive(Debug, Clone)]
pub enum ReplyContent {
    Text(String),
    Markdown(String),
    /// Streaming AI Card delta. The connector accumulates state per (session,
    /// run) and decides when to call platform create / update APIs. Final chunk
    /// signals "no more deltas; finalize the card now".
    AiCardChunk {
        delta: String,
        final_chunk: bool,
    },
    /// AI run failed; tell the connector to mark the card as errored so the
    /// user sees an explicit fail state instead of a half-typed message.
    AiCardFail,
}

#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("transient error: {0}")]
    Transient(String),
    #[error("auth expired / kicked: {0}")]
    AuthExpired(String),
    #[error("fatal: {0}")]
    Fatal(String),
    #[error("shutdown requested")]
    ShutdownRequested,
    #[error("not supported: {0}")]
    NotSupported(&'static str),
}

/// Narrow capability surface injected into every connector by the manager.
/// **Do not** add `AppHandle` or any tauri type here — connectors must remain
/// transport-neutral.
#[derive(Clone)]
pub struct ConnectorContext {
    pub config_store: Arc<ChannelConfigStore>,
    pub secure_storage: Option<Arc<SecureStorage>>,
    pub ask_coordinator: Option<Arc<IMAskCoordinator>>,
    pub pending_manager: Arc<PendingQueueManager>,
    pub cancel_token: CancellationToken,
}

#[derive(Debug, Clone, Default)]
pub struct RegistrationRequest {
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct PollRequest {
    pub device_code: String,
    pub source: String,
}

pub type RegistrationBegin = ChannelRegistrationBeginResult;
pub type RegistrationPoll = ChannelRegistrationPollResult;

#[async_trait]
pub trait IMConnector: Send + Sync {
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> ConnectorCapabilities;

    /// Start the connector and return a stream of normalized `ChannelMessage`.
    ///
    /// Contract:
    /// - The connector MUST honor `ctx.cancel_token`; when cancelled, every
    ///   internal task / TCP connection / webhook handler must drop within 2s.
    /// - Stream end is interpreted by the manager as "connection lost"; the
    ///   manager applies reconnect-backoff if appropriate.
    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError>;

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError>;

    async fn stop(&self) -> Result<(), ConnectorError> {
        Ok(())
    }

    async fn begin_registration(
        &self,
        _req: &RegistrationRequest,
    ) -> Result<RegistrationBegin, ConnectorError> {
        Err(ConnectorError::NotSupported("begin_registration"))
    }

    async fn poll_registration(
        &self,
        _req: &PollRequest,
    ) -> Result<RegistrationPoll, ConnectorError> {
        Err(ConnectorError::NotSupported("poll_registration"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_can_be_constructed() {
        let c = ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_text_streaming: true,
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::DeviceCode,
        };
        // outbound_text_streaming is meaningful only when outbound_aicard=false
        // (text-edit streaming path, e.g. WhatsApp).
        assert!(c.outbound_text_streaming);
    }

    #[test]
    fn connector_error_display_does_not_panic() {
        let err = ConnectorError::Transient("oops".into());
        assert!(err.to_string().contains("transient"));
        let err = ConnectorError::NotSupported("foo");
        assert!(err.to_string().contains("not supported"));
    }
}
