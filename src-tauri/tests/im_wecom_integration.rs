//! Phase 2 PR5 integration: WecomConnector trait-surface contract.
//!
//! Two cases. Together they exercise the trait surface end-to-end against a
//! local mock aibot WS server (the same shape `im_wecom_aibot_client.rs` uses,
//! plus the parser / sender wired through `WecomConnector::start` →
//! `BoxStream<ChannelMessage>`):
//!
//!  (a) `end_to_end_inbound_text_then_send_markdown_uses_respond` — the mock
//!      server pushes one inbound text frame; the connector's start() stream
//!      yields a `ChannelMessage` with the right `text` / `robot_code` /
//!      `reply_group_id`. A follow-up `send(Markdown)` must route through
//!      `aibot_respond_msg` (not `aibot_send_msg`) because the inbound frame's
//!      `req_id` is fresh in the `SessionMap` cache.
//!
//!  (b) `cancel_ends_stream_within_two_seconds` — once start() is running and
//!      the inbound frame has been drained, calling `cancel_token.cancel()`
//!      must cause the BoxStream to end within 2.5s (the same trait-level
//!      contract that `im_connector_cancel_test.rs` pins for feishu).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use app_lib::connector::im::shared::config_store::ChannelConfigStore;
use app_lib::connector::im::trait_def::{ConnectorContext, IMConnector, ReplyContent, ReplyTarget};
use app_lib::connector::im::wecom::aibot_client::{AibotClient, AibotClientConfig};
use app_lib::connector::im::wecom::WecomConnector;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::SessionId;
use app_lib::runtime::pending::{ConvDirResolver, PendingConfig, PendingQueueManager};
use app_lib::runtime::run_registry::RuntimeRunRegistry;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

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

/// Mock aibot ws server. Replies to `aibot_subscribe` with errcode=0, optionally
/// pushes one inbound `aibot_msg_callback` text frame (when `push_inbound=true`),
/// and acks any `aibot_respond_msg` / `aibot_send_msg` it sees while recording
/// the full frame to `outbound` for assertions.
async fn spawn_mock(push_inbound: bool) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");
    let outbound = Arc::new(Mutex::new(Vec::<Value>::new()));
    let outbound_recorder = outbound.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (write, mut read) = ws.split();
        let write_handle = Arc::new(Mutex::new(write));

        let push_text = json!({
            "cmd": "aibot_msg_callback",
            "headers": { "req_id": "REQ_INBOUND_1" },
            "body": {
                "msgid": "M1",
                "aibotid": "BOTID",
                "chattype": "single",
                "from": { "userid": "U1" },
                "msgtype": "text",
                "text": { "content": "hi" }
            }
        });

        while let Some(Ok(msg)) = read.next().await {
            let text = match msg {
                Message::Text(t) => t,
                _ => continue,
            };
            let frame: Value = serde_json::from_str(&text).unwrap();
            let cmd = frame.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
            let req_id = frame
                .pointer("/headers/req_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            match cmd {
                "aibot_subscribe" => {
                    let ack =
                        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" });
                    let _ = write_handle
                        .lock()
                        .await
                        .send(Message::Text(ack.to_string().into()))
                        .await;
                    if push_inbound {
                        let _ = write_handle
                            .lock()
                            .await
                            .send(Message::Text(push_text.to_string().into()))
                            .await;
                    }
                }
                "ping" => {
                    let ack =
                        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" });
                    let _ = write_handle
                        .lock()
                        .await
                        .send(Message::Text(ack.to_string().into()))
                        .await;
                }
                "aibot_respond_msg" | "aibot_send_msg" => {
                    outbound_recorder.lock().await.push(frame.clone());
                    let ack =
                        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" });
                    let _ = write_handle
                        .lock()
                        .await
                        .send(Message::Text(ack.to_string().into()))
                        .await;
                }
                _ => continue,
            }
        }
    });
    (url, outbound)
}

fn test_aibot(ws_url: String) -> Arc<AibotClient> {
    let mut cfg = AibotClientConfig::production("BOTID".into(), "SECRET".into());
    cfg.ws_url = ws_url;
    // Long heartbeat so the test doesn't spam pings during its short window.
    cfg.heartbeat_interval = Duration::from_secs(60);
    cfg.reply_ack_timeout = Duration::from_secs(2);
    Arc::new(AibotClient::new(cfg))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn end_to_end_inbound_text_then_send_markdown_uses_respond() {
    let (ws_url, outbound) = spawn_mock(true).await;
    let aibot = test_aibot(ws_url);
    let conn = WecomConnector::for_test(aibot);

    let tmp = TempDir::new().unwrap();
    let cancel = CancellationToken::new();
    let ctx = build_ctx(&tmp, cancel.clone());

    let mut stream = conn.start(ctx).await.unwrap();

    // Consumer side: first inbound must reach us intact — text + ids preserved.
    let msg = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("stream should yield within 3s")
        .expect("stream should not end before first message");

    assert_eq!(msg.text, "hi");
    assert_eq!(
        msg.robot_code, "TEST-BOT",
        "for_test ctor uses TEST-BOT as bot_id"
    );
    assert_eq!(msg.reply_group_id, "U1");
    assert_eq!(msg.conversation_key, "U1");
    assert_eq!(msg.sender_id, "U1");

    // Reply via the trait `send`. With the inbound req_id freshly recorded in
    // SessionMap, the sender must pick respond_msg (req_id reuse) over send_msg.
    conn.send(
        ReplyTarget {
            session_id: msg.conversation_key.clone(),
            external_conversation_key: msg.reply_group_id.clone(),
        },
        ReplyContent::Markdown("answer".into()),
    )
    .await
    .expect("send should succeed");

    // Outbound recorder is filled inside the ws accept loop; give it a moment.
    let mut respond: Option<Value> = None;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let frames = outbound.lock().await;
        if let Some(f) = frames
            .iter()
            .find(|f| f.get("cmd").and_then(|v| v.as_str()) == Some("aibot_respond_msg"))
            .cloned()
        {
            respond = Some(f);
            break;
        }
    }
    let respond = respond.expect("must have emitted an aibot_respond_msg frame");
    assert_eq!(
        respond.pointer("/headers/req_id").and_then(|v| v.as_str()),
        Some("REQ_INBOUND_1"),
        "respond_msg must reuse the inbound req_id"
    );
    assert_eq!(
        respond
            .pointer("/body/markdown/content")
            .and_then(|v| v.as_str()),
        Some("answer")
    );
    // And critically: no aibot_send_msg fallback should have fired.
    let frames = outbound.lock().await;
    assert!(
        !frames
            .iter()
            .any(|f| f.get("cmd").and_then(|v| v.as_str()) == Some("aibot_send_msg")),
        "fresh req_id must route through respond_msg, never send_msg fallback"
    );

    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_ends_stream_within_two_seconds() {
    let (ws_url, _outbound) = spawn_mock(true).await;
    let aibot = test_aibot(ws_url);
    let conn = WecomConnector::for_test(aibot);

    let tmp = TempDir::new().unwrap();
    let cancel = CancellationToken::new();
    let ctx = build_ctx(&tmp, cancel.clone());

    let mut stream = conn.start(ctx).await.unwrap();

    // Drain the seed inbound so the stream is in its idle/sleep loop — that's
    // where the cancel observation must fire (matches the feishu integration
    // test's contract shape).
    let _ = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap();

    // After 200ms, cancel.
    let cancel_for_signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel_for_signal.cancel();
    });

    let start = std::time::Instant::now();
    while let Some(_) = stream.next().await {
        if start.elapsed() > Duration::from_secs(3) {
            panic!("wecom stream did not end within 3s after cancellation");
        }
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(2500),
        "wecom trait-surface cancel-to-stream-end exceeded 2.5s: actual = {elapsed:?}"
    );
}
