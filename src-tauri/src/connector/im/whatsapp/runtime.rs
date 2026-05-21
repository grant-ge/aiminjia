//! Bot 构造 + event handler 闭包。spec v3 §3.6 + §4 + §8。
//!
//! 拆出来避免 connector.rs 太长。本模块只暴露 `start_bot(...)` 一个入口；
//! 内部构造 wa-rs Bot 并起 bot.run()，返回 `JoinHandle<()>` 给 connector
//! 存到 `bot_handle` 字段。
//!
//! Verified use paths (wa-rs 0.2.0):
//! - `wa_rs::bot::Bot`                           — bot.rs re-exported
//! - `wa_rs::store::SqliteStore`                 — store/mod.rs re-exports under feature "sqlite-storage"
//! - `wa_rs::transport::TokioWebSocketTransportFactory` — transport.rs re-exports
//! - `wa_rs::transport::UreqHttpClient`          — transport.rs re-exports
//! - `wa_rs::types::events::{Event, PairSuccess, PairError, Connected}` — types/mod.rs re-exports wa_rs_core::types::*

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use wa_rs::bot::Bot;
use wa_rs::client::Client;
use wa_rs::store::SqliteStore;
use wa_rs::types::events::Event;

use super::config::{self, WhatsAppChannelConfig};
use super::proxy_transport::{ProxyHttpClient, ProxyWebSocketTransportFactory};
use super::session::WhatsAppPaths;
use super::types::PairingState;
use crate::connector::im::shared::dedup::MessageDedupSet;
use crate::connector::im::types::ChannelMessage;

/// 构造 wa-rs Bot 并启动 `bot.run()`，返回 `JoinHandle<()>`。
///
/// 调用方负责把 JoinHandle 存进 connector 字段，后续 `stop()` 时 abort。
/// PairingState 在 spawn 前先置 `AwaitingQr`；QR 码 / 配对结果通过
/// `on_event` 闭包异步推入。
/// `on_status` 回调在 Connected / PairSuccess / PairError 时通知 manager
/// 更新 connection state，让前端看到正确状态。
/// `inbound_tx` 和 `dedup` 是 PR4 入站消息管道。
/// `bot_client_slot` 是 PR5 出站句柄：bot.run() 前存入，stop() 时清空。
/// `downloader` 是 PR7 媒体下载句柄，传入 handle_event 供 IMAGE / DOCUMENT 下载。
pub async fn start_bot(
    paths: WhatsAppPaths,
    pairing_state: Arc<Mutex<PairingState>>,
    on_status: Arc<
        dyn Fn(crate::connector::im::types::ChannelConnectionState, Option<String>)
            + Send
            + Sync
            + 'static,
    >,
    inbound_tx: Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>>,
    dedup: Arc<MessageDedupSet>,
    bot_client_slot: Arc<Mutex<Option<Arc<Client>>>>,
    downloader: Arc<super::download::WhatsAppMediaDownloader>,
) -> anyhow::Result<JoinHandle<()>> {
    let db_path = paths.session_db();
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("session.db path is not valid UTF-8: {db_path:?}"))?;
    let backend = Arc::new(SqliteStore::new(db_path_str).await?);

    let paths_for_closure = paths.clone();
    let state_for_closure = Arc::clone(&pairing_state);
    let on_status_for_closure = Arc::clone(&on_status);
    let inbound_for_closure = Arc::clone(&inbound_tx);
    let dedup_for_closure = Arc::clone(&dedup);
    let downloader_for_closure = Arc::clone(&downloader);

    let mut bot = Bot::builder()
        .with_backend(backend)
        .with_transport_factory(ProxyWebSocketTransportFactory::new())
        .with_http_client(ProxyHttpClient::new()?)
        .skip_history_sync()
        .on_event(move |event, _client| {
            let paths = paths_for_closure.clone();
            let pairing_state = Arc::clone(&state_for_closure);
            let on_status = Arc::clone(&on_status_for_closure);
            let inbound_tx = Arc::clone(&inbound_for_closure);
            let dedup = Arc::clone(&dedup_for_closure);
            let downloader = Arc::clone(&downloader_for_closure);
            async move {
                handle_event(
                    event,
                    &paths,
                    pairing_state,
                    on_status,
                    inbound_tx,
                    dedup,
                    downloader,
                )
                .await;
            }
        })
        .build()
        .await?;

    // 先把状态推到 AwaitingQr，再 spawn bot loop
    {
        let mut state = pairing_state.lock().await;
        *state = PairingState::AwaitingQr {
            started_at: Instant::now(),
        };
    }

    // 存 client 句柄，stop() 和 send() 通过 bot_client_slot 读取。
    *bot_client_slot.lock().await = Some(bot.client());

    bot.run().await
}

/// 处理 wa-rs Event，更新 `PairingState`，写 config.json（PairSuccess 时）。
///
/// PR3 关心配对相关的 4 个 event；PR4 新增 Event::Message / LoggedOut / StreamReplaced。
/// `on_status` 在 Connected / PairSuccess / PairError / LoggedOut / StreamReplaced 时
/// 通知 manager 更新 connection state，让前端看到正确状态。
/// PR7：`downloader` 用于 IMAGE / DOCUMENT 媒体下载。
pub(crate) async fn handle_event(
    event: Event,
    paths: &WhatsAppPaths,
    pairing_state: Arc<Mutex<PairingState>>,
    on_status: Arc<
        dyn Fn(crate::connector::im::types::ChannelConnectionState, Option<String>)
            + Send
            + Sync
            + 'static,
    >,
    inbound_tx: Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>>,
    dedup: Arc<MessageDedupSet>,
    downloader: Arc<super::download::WhatsAppMediaDownloader>,
) {
    use crate::connector::im::types::ChannelConnectionState;
    match event {
        Event::PairingQrCode { code, timeout } => {
            log::info!("[whatsapp] received PairingQrCode (timeout={:?})", timeout);
            let mut state = pairing_state.lock().await;
            *state = PairingState::QrIssued {
                code,
                expires_at: Instant::now() + timeout,
            };
        }
        Event::PairSuccess(success) => {
            let jid = success.id.to_string();
            let push_name = success.business_name.clone();
            log::info!("[whatsapp] PairSuccess jid={} push_name={}", jid, push_name);
            let cfg = WhatsAppChannelConfig {
                schema_version: 1,
                jid: jid.clone(),
                push_name: push_name.clone(),
                paired_at: chrono::Utc::now().to_rfc3339(),
                allow_from: None,
            };
            if let Err(e) = config::write(&paths.config_path(), &cfg) {
                log::error!("[whatsapp] failed to write config.json: {e:#}");
            }
            let mut state = pairing_state.lock().await;
            *state = PairingState::Connected { jid, push_name };
            drop(state);
            on_status(ChannelConnectionState::Connected, None);
        }
        Event::PairError(err) => {
            log::warn!("[whatsapp] PairError: {}", err.error);
            on_status(
                ChannelConnectionState::ConfigError,
                Some(format!("pairing failed: {}", err.error)),
            );
        }
        Event::Connected(_) => {
            // Connected fires on startup when a session.db is already paired.
            // Recover PairingState from config.json so poll_registration can return
            // the correct status without waiting for another PairSuccess.
            log::info!("[whatsapp] Connected event");
            let mut state = pairing_state.lock().await;
            if matches!(*state, PairingState::Idle | PairingState::AwaitingQr { .. }) {
                if let Ok(Some(cfg)) = config::read(&paths.config_path()) {
                    *state = PairingState::Connected {
                        jid: cfg.jid,
                        push_name: cfg.push_name,
                    };
                }
            }
            drop(state);
            on_status(ChannelConnectionState::Connected, None);
        }

        Event::Message(msg, info) => {
            // 1. dedup — MessageId is String alias, pass directly
            let msg_id = info.id.clone();
            if !dedup.observe(&msg_id).await {
                log::debug!("[whatsapp] duplicate msg_id {}, dropping", msg_id);
                return;
            }
            // 2. 读 config.json 拿 allow_from（每次都读，简单且自动生效最新配置）
            let cfg = match config::read(&paths.config_path()) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("[whatsapp] failed to read config.json: {e}, allowing all");
                    None
                }
            };
            // 3. parser normalize (PR7: with downloader for IMAGE / DOCUMENT)
            let cm =
                match super::parser::normalize_async(&msg, &info, cfg.as_ref(), Some(&downloader))
                    .await
                {
                    Some(c) => c,
                    None => return, // dropped by parser (group / is_from_me / allow_from)
                };
            // 4. push 到 sink（如果有 receiver）
            if let Some(tx) = inbound_tx.lock().await.as_ref() {
                if let Err(e) = tx.try_send(cm) {
                    log::warn!("[whatsapp] inbound channel send failed: {e}");
                }
            } else {
                log::trace!("[whatsapp] no inbound receiver, dropping msg {}", msg_id);
            }
        }

        Event::LoggedOut(lo) => {
            log::warn!(
                "[whatsapp] LoggedOut on_connect={} reason={:?}",
                lo.on_connect,
                lo.reason
            );
            *inbound_tx.lock().await = None;
            on_status(
                ChannelConnectionState::NeedsReauth,
                Some(format!("WhatsApp 已登出: {:?}", lo.reason)),
            );
        }

        Event::StreamReplaced(_) => {
            log::warn!("[whatsapp] StreamReplaced — another device took over");
            *inbound_tx.lock().await = None;
            on_status(
                ChannelConnectionState::NeedsReauth,
                Some("已在其他设备登录".into()),
            );
        }

        // 显式 drop 这些 noisy events，避免走 catch-all 造成无意义日志
        Event::Receipt(_) | Event::Presence(_) | Event::ChatPresence(_) => {}

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;
    use wa_rs::types::events::{Connected, LoggedOut, PairError, PairSuccess, StreamReplaced};
    use wa_rs::wa_rs_proto::whatsapp as wa;

    use crate::connector::im::types::ChannelConnectionState;
    use wa_rs::types::message::MessageInfo;

    fn tmp_paths() -> (TempDir, WhatsAppPaths) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("channels").join("whatsapp");
        let paths = WhatsAppPaths::new(&base);
        paths.ensure_base_dir().unwrap();
        (dir, paths)
    }

    fn no_op_status() -> Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>
    {
        Arc::new(|_state, _err| {})
    }

    fn no_inbound_tx() -> Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>>
    {
        Arc::new(tokio::sync::Mutex::new(None))
    }

    fn no_dedup() -> Arc<MessageDedupSet> {
        Arc::new(MessageDedupSet::with_default_cap())
    }

    fn make_downloader_for_test(
    ) -> Arc<crate::connector::im::whatsapp::download::WhatsAppMediaDownloader> {
        Arc::new(
            crate::connector::im::whatsapp::download::WhatsAppMediaDownloader::new(
                Arc::new(tokio::sync::Mutex::new(None)),
                std::path::PathBuf::from("/tmp/whatsapp_test_dl"),
            ),
        )
    }

    // PairSuccess needs a Jid; wa_rs::Jid implements Display and From<&str> via Default,
    // but the cleanest way to make one for tests is using the Default + to_string().
    // Actually wa_rs re-exports wa_rs_binary::jid::Jid which has Default.
    fn make_pair_success(jid_str: &str, push_name: &str) -> PairSuccess {
        use wa_rs::Jid;
        PairSuccess {
            id: {
                let mut j = Jid::default();
                j.user = jid_str.split('@').next().unwrap_or("").to_string();
                j.server = jid_str
                    .split('@')
                    .nth(1)
                    .unwrap_or("s.whatsapp.net")
                    .to_string();
                j
            },
            lid: Jid::default(),
            business_name: push_name.to_string(),
            platform: "android".to_string(),
        }
    }

    fn make_test_message_event(msg_id: &str, text: &str) -> Event {
        use wa_rs::Jid;
        let msg = Box::new({
            let mut m = wa::Message::default();
            m.conversation = Some(text.into());
            m
        });
        let info = {
            let mut i = MessageInfo::default();
            i.id = msg_id.to_string();
            i.source.is_group = false;
            i.source.is_from_me = false;
            i.source.chat = Jid {
                user: "8613912345678".into(),
                server: "s.whatsapp.net".into(),
                ..Default::default()
            };
            i.source.sender = i.source.chat.clone();
            i.push_name = "Alice".into();
            i
        };
        Event::Message(msg, info)
    }

    #[tokio::test]
    async fn handle_event_pairing_qr_code_sets_qr_issued() {
        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::AwaitingQr {
            started_at: Instant::now(),
        }));
        let event = Event::PairingQrCode {
            code: "1@test_qr".into(),
            timeout: Duration::from_secs(60),
        };
        handle_event(
            event,
            &paths,
            Arc::clone(&state),
            no_op_status(),
            no_inbound_tx(),
            no_dedup(),
            make_downloader_for_test(),
        )
        .await;
        let s = state.lock().await;
        match &*s {
            PairingState::QrIssued { code, .. } => assert_eq!(code, "1@test_qr"),
            other => panic!("expected QrIssued, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_event_pair_success_writes_config_and_sets_connected() {
        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::QrIssued {
            code: "1@test".into(),
            expires_at: Instant::now() + Duration::from_secs(60),
        }));

        let success = make_pair_success("8613912345678@s.whatsapp.net", "Alice");
        let event = Event::PairSuccess(success);
        handle_event(
            event,
            &paths,
            Arc::clone(&state),
            no_op_status(),
            no_inbound_tx(),
            no_dedup(),
            make_downloader_for_test(),
        )
        .await;

        // Config should be written
        let cfg = config::read(&paths.config_path())
            .expect("read ok")
            .expect("config exists after PairSuccess");
        assert_eq!(cfg.push_name, "Alice");
        assert_eq!(cfg.schema_version, 1);
        assert!(cfg.allow_from.is_none());

        // State should be Connected
        let s = state.lock().await;
        match &*s {
            PairingState::Connected { push_name, .. } => {
                assert_eq!(push_name, "Alice");
            }
            other => panic!("expected Connected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_event_connected_recovers_state_from_config() {
        let (_dir, paths) = tmp_paths();
        // Pre-write a config.json to simulate a session that was previously paired
        let cfg = WhatsAppChannelConfig {
            schema_version: 1,
            jid: "8613912345678@s.whatsapp.net".into(),
            push_name: "Bob".into(),
            paired_at: "2026-05-19T10:00:00Z".into(),
            allow_from: None,
        };
        config::write(&paths.config_path(), &cfg).unwrap();

        // State starts as Idle (fresh connector restart)
        let state = Arc::new(Mutex::new(PairingState::Idle));
        // Connected is a unit struct
        let event = Event::Connected(Connected);
        handle_event(
            event,
            &paths,
            Arc::clone(&state),
            no_op_status(),
            no_inbound_tx(),
            no_dedup(),
            make_downloader_for_test(),
        )
        .await;

        let s = state.lock().await;
        match &*s {
            PairingState::Connected { jid, push_name } => {
                assert_eq!(jid, "8613912345678@s.whatsapp.net");
                assert_eq!(push_name, "Bob");
            }
            other => panic!("expected Connected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_event_pair_error_does_not_change_state() {
        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::QrIssued {
            code: "1@test".into(),
            expires_at: Instant::now() + Duration::from_secs(60),
        }));

        // PairError has: id, lid, business_name, platform, error
        use wa_rs::Jid;
        let err = PairError {
            id: Jid::default(),
            lid: Jid::default(),
            business_name: String::new(),
            platform: String::new(),
            error: "timeout".to_string(),
        };
        let event = Event::PairError(err);
        handle_event(
            event,
            &paths,
            Arc::clone(&state),
            no_op_status(),
            no_inbound_tx(),
            no_dedup(),
            make_downloader_for_test(),
        )
        .await;

        // State must remain QrIssued — PairError doesn't mutate it
        let s = state.lock().await;
        assert!(
            matches!(&*s, PairingState::QrIssued { code, .. } if code == "1@test"),
            "expected QrIssued unchanged, got {s:?}"
        );
    }

    // ---- PR4 新增测试 ----

    #[tokio::test]
    async fn handle_event_message_pushes_to_inbound_when_attached() {
        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::Idle));
        let dedup = no_dedup();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<ChannelMessage>(16);
        let inbound_tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));

        let event = make_test_message_event("M_TEST_PUSH", "hello");
        handle_event(
            event,
            &paths,
            Arc::clone(&state),
            no_op_status(),
            Arc::clone(&inbound_tx),
            dedup,
            make_downloader_for_test(),
        )
        .await;

        let got = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("should not timeout")
            .expect("channel should not be closed");
        assert_eq!(got.msg_id, "M_TEST_PUSH");
        assert_eq!(got.text, "hello");
    }

    #[tokio::test]
    async fn handle_event_message_dropped_when_no_inbound_tx() {
        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::Idle));
        let inbound_tx = no_inbound_tx(); // None — no receiver
        let dedup = no_dedup();

        let event = make_test_message_event("M_NO_RX", "silent drop");
        // Should not panic — just silently drop
        handle_event(
            event,
            &paths,
            Arc::clone(&state),
            no_op_status(),
            inbound_tx,
            dedup,
            make_downloader_for_test(),
        )
        .await;
        // Reaching here means no panic
    }

    #[tokio::test]
    async fn handle_event_dedup_drops_repeat() {
        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::Idle));
        let dedup = no_dedup();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<ChannelMessage>(16);
        let inbound_tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));

        // Send the same msg_id twice
        for _ in 0..2 {
            let event = make_test_message_event("M_DEDUP", "dup");
            handle_event(
                event,
                &paths,
                Arc::clone(&state),
                no_op_status(),
                Arc::clone(&inbound_tx),
                Arc::clone(&dedup),
                make_downloader_for_test(),
            )
            .await;
        }

        // Only one message should arrive
        let got = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("should not timeout")
            .expect("channel should not be closed");
        assert_eq!(got.msg_id, "M_DEDUP");

        // Second recv should timeout (no second message)
        let second = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(
            second.is_err(),
            "second recv should timeout — dedup dropped it"
        );
    }

    #[tokio::test]
    async fn handle_event_logged_out_drops_tx_and_emits_needs_reauth() {
        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::Idle));

        let (tx, _rx) = tokio::sync::mpsc::channel::<ChannelMessage>(16);
        let inbound_tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));

        // Spy callback that captures the last (state, msg) pair
        let captured: Arc<Mutex<Option<(ChannelConnectionState, Option<String>)>>> =
            Arc::new(Mutex::new(None));
        let captured_clone = Arc::clone(&captured);
        let spy_status: Arc<
            dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static,
        > = Arc::new(move |s, m| {
            // Store synchronously via try_lock — tests run in single-threaded context
            if let Ok(mut g) = captured_clone.try_lock() {
                *g = Some((s, m));
            }
        });

        use wa_rs::types::events::ConnectFailureReason;
        let lo = LoggedOut {
            on_connect: false,
            reason: ConnectFailureReason::LoggedOut,
        };
        let event = Event::LoggedOut(lo);
        handle_event(
            event,
            &paths,
            Arc::clone(&state),
            spy_status,
            Arc::clone(&inbound_tx),
            no_dedup(),
            make_downloader_for_test(),
        )
        .await;

        // inbound_tx should now be None
        assert!(
            inbound_tx.lock().await.is_none(),
            "LoggedOut must drop inbound_tx"
        );

        // on_status should have fired with NeedsReauth
        let cap = captured.lock().await;
        let (conn_state, msg) = cap.as_ref().expect("on_status should have been called");
        assert_eq!(*conn_state, ChannelConnectionState::NeedsReauth);
        assert!(
            msg.as_deref().unwrap_or("").contains("已登出"),
            "message should mention logout, got: {:?}",
            msg
        );

        // Also verify StreamReplaced path compiles and runs correctly via a quick smoke test
        drop(cap);
        let (tx2, _rx2) = tokio::sync::mpsc::channel::<ChannelMessage>(16);
        let inbound_tx2 = Arc::new(tokio::sync::Mutex::new(Some(tx2)));
        let event2 = Event::StreamReplaced(StreamReplaced);
        let state2 = Arc::new(Mutex::new(PairingState::Idle));
        handle_event(
            event2,
            &paths,
            Arc::clone(&state2),
            no_op_status(),
            Arc::clone(&inbound_tx2),
            no_dedup(),
            make_downloader_for_test(),
        )
        .await;
        assert!(
            inbound_tx2.lock().await.is_none(),
            "StreamReplaced must drop inbound_tx"
        );
    }
}
