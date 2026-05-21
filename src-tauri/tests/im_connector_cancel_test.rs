//! Phase 0 trait contract: when `ConnectorContext.cancel_token` is cancelled,
//! the connector's stream must end within 2 seconds.
//!
//! This is the documented contract `IMConnector::start` makes:
//! > The connector MUST honor `ctx.cancel_token`; when cancelled, every
//! > internal task / TCP connection / webhook handler must drop within 2s.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use app_lib::connector::im::shared::config_store::ChannelConfigStore;
use app_lib::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use app_lib::connector::im::types::{ChannelMessage, Platform};
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

/// Pretends to be alive forever — sleeps in 1s ticks. It honours
/// `ctx.cancel_token` by selecting on it. The stream ends only when the token
/// fires. This is the minimum-realistic shape of every long-lived connector
/// (`Stream` mode) implementation.
struct SlowStreamConnector;

#[async_trait]
impl IMConnector for SlowStreamConnector {
    fn platform(&self) -> Platform {
        Platform::Dingtalk
    }
    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_markdown: true,
            outbound_text_streaming: false,
            supports_attachments: false,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::ApiKey,
        }
    }
    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let cancel = ctx.cancel_token.clone();
        // Hand-rolled stream: each `unfold` step either yields nothing because
        // 1s elapsed, or returns None because cancel fired. The latter ends the
        // stream. No `async-stream` crate needed.
        let stream = futures::stream::unfold(cancel, |cancel| async move {
            tokio::select! {
                _ = cancel.cancelled() => None,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    // Alive but produced no message. Loop until cancelled.
                    Some((None::<ChannelMessage>, cancel))
                }
            }
        })
        .filter_map(|opt| async move { opt });
        Ok(Box::pin(stream))
    }
    async fn send(&self, _t: ReplyTarget, _c: ReplyContent) -> Result<(), ConnectorError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_token_drops_stream_within_two_seconds() {
    let tempdir = TempDir::new().expect("tempdir");
    let cancel = CancellationToken::new();
    let ctx = build_ctx(&tempdir, cancel.clone());

    let connector = SlowStreamConnector;
    let mut stream = connector.start(ctx).await.expect("start");

    // After 200ms, cancel.
    let cancel_for_signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel_for_signal.cancel();
    });

    let start = Instant::now();
    while let Some(_) = stream.next().await {
        // SlowStreamConnector never yields messages; this loop won't run.
        if start.elapsed() > Duration::from_secs(3) {
            panic!("stream did not end within 3s after cancellation");
        }
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(2500),
        "cancel-to-stream-end exceeded 2.5s: actual = {elapsed:?}"
    );
}

/// Real `FeishuConnector` cancel-2s contract (PR3).
///
/// The connector's `start()` spawns a `FeishuStreamClient::run_with_retry` task
/// that POSTs to `https://open.feishu.cn/callback/ws/endpoint`. With invalid
/// credentials the POST either: (a) is reachable and returns errcode 99991661
/// (→ ConfigError, task exits), (b) is unreachable / network blocked (→ goes
/// to Reconnecting + sleep). Both paths must honour cancel within 2s.
///
/// We test the harder (b) case implicitly: the `tokio::select!` on
/// `cancel.cancelled()` inside `run_with_retry` ensures the task drops
/// regardless of which retry phase it's in. We assert the spawned task closes
/// the message channel, dropping the BoxStream, within 2.5s after cancel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feishu_connector_cancel_token_drops_stream_within_two_seconds() {
    use app_lib::connector::im::feishu::FeishuConnector;

    let tempdir = TempDir::new().expect("tempdir");
    let cancel = CancellationToken::new();
    let ctx = build_ctx(&tempdir, cancel.clone());

    // Fake credentials — the WS endpoint will reject them, but the cancel
    // contract is about timing not auth outcome.
    let connector = FeishuConnector::new("cli_fake".into(), "fake_secret".into());
    let mut stream = connector.start(ctx).await.expect("start");

    // After 200ms, cancel.
    let cancel_for_signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel_for_signal.cancel();
    });

    let start = Instant::now();
    // Drain until the stream ends. FeishuConnector never produces messages here
    // (no valid auth), so this exits when the spawned task drops msg_tx.
    while let Some(_) = stream.next().await {
        if start.elapsed() > Duration::from_secs(3) {
            panic!("feishu stream did not end within 3s after cancellation");
        }
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(2500),
        "feishu cancel-to-stream-end exceeded 2.5s: actual = {elapsed:?}"
    );
}
