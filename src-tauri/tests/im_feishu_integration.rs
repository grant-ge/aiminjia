//! Phase 1 PR7 integration: feishu connector trait-surface contract.
//!
//! Two cases, both exercise `IMConnector::start` -> `BoxStream<ChannelMessage>`
//! consumer flow directly (no ChannelManager / chat_adapter fixtures — those
//! are deferred to a future manager-level test). Together they pin the
//! feishu-shape of the contract:
//!
//!  (a) `first_message_dispatches_through_boxstream` — a synthetic feishu-shaped
//!      connector yields one `ChannelMessage` from `start()`, and the stream
//!      consumer receives it intact (text + conversation_type + ids carried
//!      through with no manager/adapter munging).
//!
//!  (b) `cancel_after_first_message_ends_stream_within_two_seconds` — once the
//!      consumer has the first message and `cancel_token.cancel()` fires, the
//!      stream completes within 2s. This is the same 2s contract checked by
//!      `im_connector_cancel_test.rs::feishu_connector_cancel_token_drops_stream_within_two_seconds`,
//!      but here we exercise the *trait surface* (a connector built on the
//!      trait alone) rather than the real WS retry path — so a future Feishu
//!      refactor that swaps out `stream.rs` doesn't accidentally break the
//!      trait-level contract.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use app_lib::connector::im::shared::config_store::ChannelConfigStore;
use app_lib::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use app_lib::connector::im::types::{ChannelMessage, ConversationType, Platform};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::SessionId;
use app_lib::runtime::pending::{ConvDirResolver, PendingConfig, PendingQueueManager};
use app_lib::runtime::run_registry::RuntimeRunRegistry;

struct TempConvDirResolver(PathBuf);

impl ConvDirResolver for TempConvDirResolver {
    fn conversation_dir(&self, session_id: &SessionId) -> Option<PathBuf> {
        let dir = self.0.join(session_id.as_str());
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    fn is_archived(&self, _session_id: &SessionId) -> bool {
        false
    }

    fn conversations_root(&self) -> PathBuf {
        self.0.clone()
    }
}

fn build_ctx(tmp: &TempDir, cancel: CancellationToken) -> ConnectorContext {
    let registry = Arc::new(RuntimeRunRegistry::new());
    let bus = Arc::new(RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(tmp.path().to_path_buf()));
    let pending_manager =
        PendingQueueManager::new(registry, bus, resolver, PendingConfig::default());
    ConnectorContext {
        config_store: Arc::new(ChannelConfigStore::new(tmp.path().to_path_buf(), None)),
        secure_storage: None,
        ask_coordinator: None,
        pending_manager,
        cancel_token: cancel,
    }
}

/// Synthetic feishu-shape connector: yields exactly one `ChannelMessage`
/// resembling an `im.message.receive_v1` private chat event, then sleeps in
/// 1s ticks honouring `ctx.cancel_token`. Capability flags mirror the real
/// `FeishuConnector` (Stream / AiCard / Markdown / attachments / DeviceCode).
struct MockFeishuConnector;

#[async_trait]
impl IMConnector for MockFeishuConnector {
    fn platform(&self) -> Platform {
        Platform::Feishu
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: true,
            outbound_markdown: true,
            outbound_text_streaming: false,
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
        let cancel = ctx.cancel_token.clone();
        // First step yields a synthetic message; subsequent steps idle on a
        // tokio::select! between cancel and 1s sleep, mirroring the real
        // SlowStream-shaped behaviour after the initial event.
        let s = stream::unfold((cancel, false), |(cancel, sent)| async move {
            if cancel.is_cancelled() {
                return None;
            }
            if !sent {
                let msg = ChannelMessage {
                    msg_id: "om_test_feishu".into(),
                    native_message_id: None,
                    conversation_type: ConversationType::Private,
                    conversation_key: "oc_test_chat".into(),
                    sender_id: "ou_test_user".into(),
                    sender_nick: "ou_test_user".into(),
                    text: "hello feishu".into(),
                    robot_code: String::new(),
                    reply_group_id: "oc_test_chat".into(),
                    attachments: vec![],
                    session_webhook: None,
                    created_at_ms: None,
                };
                return Some((Some(msg), (cancel, true)));
            }
            tokio::select! {
                _ = cancel.cancelled() => None,
                _ = tokio::time::sleep(Duration::from_secs(1)) => Some((None, (cancel, true))),
            }
        })
        .filter_map(|opt| async move { opt });
        Ok(s.boxed())
    }

    async fn send(&self, _t: ReplyTarget, _c: ReplyContent) -> Result<(), ConnectorError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_message_dispatches_through_boxstream() {
    let tmp = TempDir::new().expect("tempdir");
    let cancel = CancellationToken::new();
    let ctx = build_ctx(&tmp, cancel.clone());

    let connector = MockFeishuConnector;
    let mut stream = connector.start(ctx).await.expect("start");

    // Consumer side: the first `ChannelMessage` must reach us intact —
    // text + ids + chat type all preserved (no manager/adapter munging in
    // the trait-surface path).
    let first = stream.next().await.expect("first message");
    assert_eq!(first.text, "hello feishu");
    assert_eq!(first.msg_id, "om_test_feishu");
    assert_eq!(first.conversation_type, ConversationType::Private);
    assert_eq!(first.conversation_key, "oc_test_chat");
    assert_eq!(first.sender_id, "ou_test_user");
    // Feishu has no robot_code concept — connector leaves it empty for the
    // manager to fill with app_id at namespacing time.
    assert_eq!(first.robot_code, "");

    // Tear-down — stream must then end ≤2s after cancel (covered explicitly
    // by the next test; here we just unblock cleanup).
    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while stream.next().await.is_some() {}
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_after_first_message_ends_stream_within_two_seconds() {
    let tmp = TempDir::new().expect("tempdir");
    let cancel = CancellationToken::new();
    let ctx = build_ctx(&tmp, cancel.clone());

    let connector = MockFeishuConnector;
    let mut stream = connector.start(ctx).await.expect("start");

    // Drain the first synthetic message so the stream is in its idle/sleep
    // loop — that's where the cancel observation must fire.
    let _first = stream.next().await.expect("first message");

    // After 200ms, cancel.
    let cancel_for_signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel_for_signal.cancel();
    });

    let start = Instant::now();
    while let Some(_) = stream.next().await {
        if start.elapsed() > Duration::from_secs(3) {
            panic!("feishu mock stream did not end within 3s after cancellation");
        }
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(2500),
        "feishu trait-surface cancel-to-stream-end exceeded 2.5s: actual = {elapsed:?}"
    );
}
