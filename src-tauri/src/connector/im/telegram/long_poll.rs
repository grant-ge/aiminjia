//! Telegram `getUpdates` long-poll main loop.
//!
//! Behavior:
//! - On start, read `lastOffset` from `telegram/state-{bot_id}.json`, default 0
//! - Each round: `get_updates(offset, timeout = 25s)`
//! - Dispatch updates through `parser::parse_update` → three branches:
//!   - PairingStart → reply via bot + `pairing.attempt_attach`
//!   - Message → allowlist gate → push to `msg_tx` for ChannelManager
//!   - Skip → drop
//! - After each update, advance offset = `update_id + 1`; flush state file
//!   every `FLUSH_BATCH` updates or `FLUSH_INTERVAL`
//! - On `cancel_token`: best-effort flush then return
//! - 401 Unauthorized → emit `NeedsReauth` and return (no reconnect)
//! - 429 TooManyRequests → sleep `retry_after`, no backoff bump
//! - Other errors → emit `Reconnecting`, sleep per `ReconnectBackoff`, continue
//!
//! NOTE (sub-part A scope): `ChannelConfigStore` does not yet expose
//! `telegram_is_in_allowlist` / `telegram_remove_allowlist_user`. They will be
//! added in sub-part B together with the storage layer. Until then the gate
//! is a TODO-stub that **defaults to allow** so manual integration testing
//! works once the manager is wired up. The pairing-start branch still writes
//! the pairer into `PairingCodeStore` so the desktop confirm flow is
//! exercisable.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use super::api::{TelegramApi, TelegramApiError};
use super::pairing::{AttachOutcome, PairerInfo, PairingCodeStore};
use super::parser::{parse_update, ParsedInbound, UnsupportedKind};
use super::sender::TelegramSender;
use super::types::TelegramSessionTarget;
use crate::connector::im::shared::config_store::ChannelConfigStore;
use crate::connector::im::shared::dedup::MessageDedupSet;
use crate::connector::im::shared::reconnect::ReconnectBackoff;
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

const LONG_POLL_TIMEOUT_SECS: u64 = 25;
const FLUSH_BATCH: usize = 10;
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

pub struct Params {
    pub api: Arc<TelegramApi>,
    pub bot_id: String,
    pub pairing: PairingCodeStore,
    pub sender: TelegramSender,
    pub session_targets: Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
    pub config_store: Arc<ChannelConfigStore>,
    pub msg_tx: mpsc::Sender<ChannelMessage>,
    pub on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    pub cancel: CancellationToken,
    /// Watchdog 共享：每次 getUpdates 完成（成功或失败）后写入 unix millis。
    pub last_get_updates_at: Arc<AtomicI64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateFile {
    #[serde(default)]
    last_offset: i64,
    #[serde(default)]
    saved_at: String,
}

pub async fn run(p: Params) {
    let dedup = MessageDedupSet::with_default_cap();
    let mut backoff = ReconnectBackoff::default_schedule();
    let state_path = state_path_for(&p.config_store, &p.bot_id);
    let mut offset = read_offset(&state_path).unwrap_or(0);
    let mut dirty_count = 0usize;
    let mut last_flush = Instant::now();

    let mut first_round = true;

    loop {
        if p.cancel.is_cancelled() {
            flush_offset(&state_path, offset);
            return;
        }
        let outcome = p.api.get_updates(offset, LONG_POLL_TIMEOUT_SECS).await;
        p.last_get_updates_at
            .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
        match outcome {
            Ok(updates) => {
                if first_round {
                    first_round = false;
                    (p.on_status)(ChannelConnectionState::Connected, None);
                }
                backoff.reset();
                for u in updates {
                    if !dedup
                        .observe(&format!("tg-{}-{}", p.bot_id, u.update_id))
                        .await
                    {
                        offset = u.update_id + 1;
                        continue;
                    }
                    match parse_update(&u, &p.bot_id) {
                        ParsedInbound::Skip(reason) => {
                            log::info!(
                                "[telegram-{}] skip update_id={} reason={:?}",
                                p.bot_id,
                                u.update_id,
                                reason
                            );
                        }
                        ParsedInbound::PairingStart {
                            code,
                            user_id,
                            first_name,
                            username,
                            chat_id,
                        } => {
                            handle_pairing_start(
                                code,
                                user_id,
                                first_name,
                                username,
                                chat_id,
                                &p.pairing,
                                &p.sender,
                                &p.config_store,
                            )
                            .await;
                        }
                        ParsedInbound::Message {
                            message,
                            user_id,
                            chat_id,
                            message_id,
                            ..
                        } => {
                            handle_message(
                                message,
                                user_id,
                                chat_id,
                                message_id,
                                &p.config_store,
                                &p.sender,
                                &p.msg_tx,
                                &p.session_targets,
                            )
                            .await;
                        }
                        ParsedInbound::Unsupported {
                            chat_id,
                            user_id,
                            message_id: _,
                            kind,
                        } => {
                            handle_unsupported(chat_id, user_id, kind, &p.config_store, &p.sender).await;
                        }
                    }
                    offset = u.update_id + 1;
                    dirty_count += 1;
                }
                if dirty_count >= FLUSH_BATCH || last_flush.elapsed() >= FLUSH_INTERVAL {
                    flush_offset(&state_path, offset);
                    dirty_count = 0;
                    last_flush = Instant::now();
                }
            }
            Err(TelegramApiError::Unauthorized(d)) => {
                log::error!("[telegram-{}] unauthorized: {d}", p.bot_id);
                (p.on_status)(ChannelConnectionState::NeedsReauth, Some(d));
                flush_offset(&state_path, offset);
                return;
            }
            Err(TelegramApiError::TooManyRequests { retry_after }) => {
                log::warn!("[telegram-{}] 429, sleeping {:?}", p.bot_id, retry_after);
                tokio::select! {
                    _ = tokio::time::sleep(retry_after) => {}
                    _ = p.cancel.cancelled() => {
                        flush_offset(&state_path, offset);
                        return;
                    }
                }
            }
            Err(e) => {
                let delay = backoff.next_delay();
                log::warn!(
                    "[telegram-{}] long-poll error: {e:?}, retry in {:?}",
                    p.bot_id,
                    delay
                );
                (p.on_status)(ChannelConnectionState::Reconnecting, Some(e.to_string()));
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = p.cancel.cancelled() => {
                        flush_offset(&state_path, offset);
                        return;
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)] // pairing/sender/config_store are all single-purpose deps; bundling adds noise.
async fn handle_pairing_start(
    code: Option<String>,
    user_id: i64,
    first_name: String,
    username: Option<String>,
    chat_id: i64,
    pairing: &PairingCodeStore,
    sender: &TelegramSender,
    config_store: &ChannelConfigStore,
) {
    // 已配对用户重发 /start：直接告诉用户已配对，避免误以为需要重新生成 code。
    let already_in_allowlist = config_store
        .telegram_is_in_allowlist(user_id)
        .unwrap_or(false);
    if already_in_allowlist {
        let _ = sender
            .send_plain(chat_id, "你已配对，可以直接发送消息。")
            .await;
        return;
    }
    let Some(code) = code else {
        let _ = sender
            .send_plain(chat_id, "请回到 AIjia 桌面端重新生成配对二维码并扫描。")
            .await;
        return;
    };
    let pairer = PairerInfo {
        user_id,
        first_name,
        username,
        chat_id,
        attached_at: chrono::Utc::now(),
    };
    match pairing.attempt_attach(&code, pairer).await {
        AttachOutcome::Attached | AttachOutcome::AlreadyAttached => {
            let _ = sender
                .send_plain(chat_id, "✓ 等待 AIjia 桌面端批准你的连接请求…")
                .await;
        }
        AttachOutcome::Conflict => {
            let _ = sender
                .send_plain(chat_id, "该配对码已被其他用户使用，请重新生成。")
                .await;
        }
        AttachOutcome::NotFound => {
            let _ = sender
                .send_plain(chat_id, "配对码已失效，请回到 AIjia 桌面端重��生成。")
                .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_message(
    message: ChannelMessage,
    user_id: i64,
    chat_id: i64,
    message_id: i64,
    config_store: &ChannelConfigStore,
    sender: &TelegramSender,
    msg_tx: &mpsc::Sender<ChannelMessage>,
    session_targets: &Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
) {
    // allowlist gate：只让已通过桌面端 confirm 的用户消息进入 LLM 链路。
    let in_allowlist = config_store
        .telegram_is_in_allowlist(user_id)
        .unwrap_or(false);
    if !in_allowlist {
        let _ = sender
            .send_plain(
                chat_id,
                "你还未与 AIjia 配对，请联系管理员在 AIjia 桌面端获取配对二维码。",
            )
            .await;
        let _ = user_id;
        return;
    }

    // 更新 session_targets 里该 chat 的 last_inbound_message_id；私聊 1:1 直接全扫
    // session_targets（数量 ~= 桌面端活跃数字员工 × 已配对用户，量级很小）。
    {
        let mut guard = session_targets.write().await;
        for target in guard.values_mut() {
            if target.chat_id == chat_id {
                target.last_inbound_message_id = Some(message_id);
            }
        }
    }

    let _ = user_id;
    if msg_tx.send(message).await.is_err() {
        log::warn!("[telegram] msg_tx closed; dropping update");
    }
}

fn state_path_for(config_store: &ChannelConfigStore, bot_id: &str) -> PathBuf {
    // Per-user channels/telegram/state-{bot_id}.json. The config_store knows
    // the active user's channels root; this avoids leaking state into the
    // global `users/channels/` (pre-PR4 path that didn't carry user scope).
    config_store
        .platform_dir(Platform::Telegram)
        .join(format!("state-{bot_id}.json"))
}

fn read_offset(path: &PathBuf) -> Option<i64> {
    let raw = std::fs::read_to_string(path).ok()?;
    let s: StateFile = serde_json::from_str(&raw).ok()?;
    Some(s.last_offset)
}

fn flush_offset(path: &PathBuf, offset: i64) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!(
                "[telegram] failed to create state dir {}: {e}",
                parent.display()
            );
            return;
        }
    }
    let s = StateFile {
        last_offset: offset,
        saved_at: chrono::Utc::now().to_rfc3339(),
    };
    let body = match serde_json::to_string(&s) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[telegram] failed to serialize state: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::write(path, &body) {
        log::warn!(
            "[telegram] failed to write state file {}: {e}",
            path.display()
        );
    }
}

/// Unsupported 消息路由：allowlist 内的用户发了我们不支持的类型 → 回一条提示。
/// 不在 allowlist 的用户不下发提示（与正常未配对消息逻辑一致：让 handle_message 走老路径）。
/// 故 unsupported 在 allowlist 外的情况，由 handle_unsupported 内部检查 allowlist 跳过。
async fn handle_unsupported(
    chat_id: i64,
    user_id: i64,
    kind: UnsupportedKind,
    config_store: &ChannelConfigStore,
    sender: &TelegramSender,
) {
    let in_allowlist = config_store
        .telegram_is_in_allowlist(user_id)
        .unwrap_or(false);
    if !in_allowlist {
        // 未配对用户不下发"不支持"提示，避免给陌生人额外的 bot 噪音
        return;
    }
    if let Err(e) = sender.send_plain(chat_id, kind.hint_text()).await {
        log::warn!(
            "[telegram] send unsupported hint failed (chat={chat_id}): {e:?}"
        );
    }
}

pub const STALL_TICK_INTERVAL: Duration = Duration::from_secs(30);
pub const STALL_TIMEOUT: Duration = Duration::from_secs(120);

pub struct WatchdogParams {
    pub api: Arc<TelegramApi>,
    pub bot_id: String,
    pub on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    pub last_get_updates_at: Arc<AtomicI64>,
    pub cancel: CancellationToken,
}

pub async fn run_watchdog(p: WatchdogParams) {
    let mut tick = tokio::time::interval(STALL_TICK_INTERVAL);
    tick.tick().await; // 跳过 immediate fire

    loop {
        tokio::select! {
            _ = p.cancel.cancelled() => return,
            _ = tick.tick() => {}
        }
        let last_ms = p.last_get_updates_at.load(Ordering::SeqCst);
        if last_ms == 0 {
            // 第一轮还没成功跑过，跳过
            continue;
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        let elapsed_ms = now_ms.saturating_sub(last_ms);
        if elapsed_ms as u64 >= STALL_TIMEOUT.as_millis() as u64 {
            log::warn!(
                "[telegram-{}] watchdog stall: last update {}ms ago, rebuilding client",
                p.bot_id,
                elapsed_ms
            );
            (p.on_status)(
                ChannelConnectionState::Reconnecting,
                Some("watchdog: long-poll stalled".into()),
            );
            if let Err(e) = p.api.rebuild_client().await {
                log::error!("[telegram-{}] watchdog rebuild_client failed: {e}", p.bot_id);
            }
            p.last_get_updates_at
                .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod watchdog_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[tokio::test(start_paused = true)]
    async fn watchdog_rebuilds_client_when_stalled_past_threshold() {
        // rebuild_client 只是重建 reqwest::Client，不做网络请求，不需要 MockServer
        let api = Arc::new(
            TelegramApi::new_with_api_base_for_tests("BOT".into(), "http://127.0.0.1:1".into())
                .unwrap(),
        );
        let last = Arc::new(AtomicI64::new(
            chrono::Utc::now().timestamp_millis() - 200_000,
        ));
        let status_calls = Arc::new(AtomicUsize::new(0));
        let status_calls_clone = status_calls.clone();
        let on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync> =
            Arc::new(move |state, _err| {
                if matches!(state, ChannelConnectionState::Reconnecting) {
                    status_calls_clone.fetch_add(1, Ordering::SeqCst);
                }
            });
        let cancel = CancellationToken::new();
        let handle = tokio::spawn({
            let params = WatchdogParams {
                api: api.clone(),
                bot_id: "BOT".into(),
                on_status,
                last_get_updates_at: last,
                cancel: cancel.clone(),
            };
            async move { run_watchdog(params).await }
        });

        // Yield once so the spawned task runs far enough to register its timer
        tokio::task::yield_now().await;
        // Advance past the 30s tick interval
        tokio::time::advance(Duration::from_secs(35)).await;
        // Give the watchdog task enough poll cycles to run the stall check
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        assert!(status_calls.load(Ordering::SeqCst) >= 1);
        cancel.cancel();
        let _ = handle.await;
    }

    #[tokio::test(start_paused = true)]
    async fn watchdog_does_not_fire_when_activity_recent() {
        let api = Arc::new(
            TelegramApi::new_with_api_base_for_tests(
                "BOT".into(),
                "http://127.0.0.1:1".into(),
            )
            .unwrap(),
        );
        let last = Arc::new(AtomicI64::new(chrono::Utc::now().timestamp_millis()));
        let status_calls = Arc::new(AtomicUsize::new(0));
        let status_calls_clone = status_calls.clone();
        let on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync> =
            Arc::new(move |_state, _err| {
                status_calls_clone.fetch_add(1, Ordering::SeqCst);
            });
        let cancel = CancellationToken::new();
        let handle = tokio::spawn({
            let params = WatchdogParams {
                api,
                bot_id: "BOT".into(),
                on_status,
                last_get_updates_at: last,
                cancel: cancel.clone(),
            };
            async move { run_watchdog(params).await }
        });
        // Yield once so the task registers its timer before we advance
        tokio::task::yield_now().await;
        // Advance 60s — less than STALL_TIMEOUT (120s), so no stall should fire
        tokio::time::advance(Duration::from_secs(60)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(status_calls.load(Ordering::SeqCst), 0);
        cancel.cancel();
        let _ = handle.await;
    }

    #[tokio::test(start_paused = true)]
    async fn watchdog_exits_on_cancel() {
        let api = Arc::new(
            TelegramApi::new_with_api_base_for_tests(
                "BOT".into(),
                "http://127.0.0.1:1".into(),
            )
            .unwrap(),
        );
        let last = Arc::new(AtomicI64::new(0));
        let on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync> =
            Arc::new(|_state, _err| {});
        let cancel = CancellationToken::new();
        let handle = tokio::spawn({
            let params = WatchdogParams {
                api,
                bot_id: "BOT".into(),
                on_status,
                last_get_updates_at: last,
                cancel: cancel.clone(),
            };
            async move { run_watchdog(params).await }
        });
        // Yield once so the task starts, then cancel immediately
        tokio::task::yield_now().await;
        cancel.cancel();
        let _ = handle.await;
    }
}
