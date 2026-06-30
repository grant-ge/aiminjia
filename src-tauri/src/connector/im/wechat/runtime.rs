//! iLink long-poll runtime —— spawn 一个 `tokio::task` 持续 `getUpdates`，
//! 把入站消息塞进 `mpsc::Sender<ChannelMessage>`。
//!
//! 协议层：参考 openclaw-weixin-main/src/monitor/monitor.ts。
//! 关键点：
//! - `get_updates_buf` 是服务端给的同步游标。第一次发 `""`，每次响应里有新
//!   游标就保存下来，下次原样回传。
//! - 服务端会 hold ≤ `longpolling_timeout_ms`（默认 35s）；我们客户端比它多
//!   5s；超时视为 normal，re-poll。
//! - `errcode == -14` = session 过期，bot_token 失效，需要重新扫码登录。
//!   本期只 log + 等 5min 再重试（不退出 worker，避免 manager 状态紊乱）。
//! - 连续 N 次失败 backoff 30s，再继续轮询。
//! - **暂不落盘**：`get_updates_buf` 进程结束就丢；重启后丢前 35s 内的旧消息，
//!   足以应付测试阶段。Phase 5 PR 后续接 disk persistence。

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::connector::im::shared::dedup::MessageDedupSet;
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, ConversationType};

use super::api;
use super::types::WechatSessionTarget;

/// 服务端给的"会话过期"错误码。bot_token 失效 / 用户主动退出登录时返回。
const SESSION_EXPIRED_ERRCODE: i64 = -14;

/// 连续失败计数到达此值时 backoff 30s。openclaw 用 5；我们保持一致。
const MAX_CONSECUTIVE_FAILURES: u32 = 5;
const BACKOFF_AFTER_FAILURES_MS: u64 = 30_000;
const RETRY_DELAY_MS: u64 = 1_500;
/// `SESSION_EXPIRED_ERRCODE` 之后暂停轮询的时长 —— 用户需要重新扫码登录
/// 才能恢复。给个 5 分钟，是个人微信侧的常见限流恢复时长。
const SESSION_EXPIRED_PAUSE_MS: u64 = 5 * 60 * 1000;

/// `WechatConnector::start` 把这堆参数交给 worker 后就 disconnect。
pub struct WorkerConfig {
    pub base_url: String,
    pub bot_token: String,
    pub app_id: String,
    pub client_version: String,
    pub ilink_bot_id: String,
    /// 自己的 ilink_user_id，用来过滤 echo back 的 BOT 消息。
    pub self_user_id: String,
}

/// 把 worker 输出 `(ChannelMessage, context_token)` 一起给 manager —— manager
/// 在 `get_or_create_session` 之后调 `connector.remember_session()` 缓存
/// context_token。
pub struct WechatInboundEvent {
    pub message: ChannelMessage,
    pub context_token: Option<String>,
}

/// Spawn the long-poll loop. The handle is a no-op besides cancellation —
/// `cancel_token.cancel()` exits within ~timeout_secs。
pub fn spawn_long_poll(
    cfg: WorkerConfig,
    msg_tx: mpsc::Sender<WechatInboundEvent>,
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    cancel_token: CancellationToken,
) {
    tokio::spawn(async move {
        run_long_poll(cfg, msg_tx, on_status, cancel_token).await;
    });
}

async fn run_long_poll(
    cfg: WorkerConfig,
    msg_tx: mpsc::Sender<WechatInboundEvent>,
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    cancel_token: CancellationToken,
) {
    let client = match reqwest::Client::builder()
        .user_agent("aijia-wechat-ilink/0.1 (https://github.com/grant-ge/aiminjia)")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::error!("[wechat-runtime] build reqwest client failed: {e}");
            on_status(
                ChannelConnectionState::ConfigError,
                Some(format!("HTTP client init failed: {e}")),
            );
            return;
        }
    };

    // Connecting 由 manager 在 connect_wechat 入口处 emit；这里到达 first
    // getUpdates 成功（即便 msgs 为空也算）就升 Connected。
    let mut emitted_connected = false;
    let dedup = MessageDedupSet::with_default_cap();
    let mut get_updates_buf = String::new();
    let mut consecutive_failures: u32 = 0;
    let mut next_timeout_secs: u64 = 35;

    log::info!(
        "[wechat-runtime] long-poll loop starting base_url={} bot={} self_user_id={}",
        cfg.base_url,
        cfg.ilink_bot_id,
        cfg.self_user_id,
    );

    loop {
        if cancel_token.is_cancelled() {
            log::info!("[wechat-runtime] cancel observed, exiting loop");
            return;
        }

        let resp_result = api::get_updates(
            &client,
            &cfg.base_url,
            &cfg.bot_token,
            &cfg.app_id,
            &cfg.client_version,
            get_updates_buf.clone(),
            Some(next_timeout_secs),
        )
        .await;

        let resp = match resp_result {
            Ok(r) => r,
            Err(e) => {
                consecutive_failures += 1;
                log::warn!(
                    "[wechat-runtime] getUpdates failed ({}/{}): {e:#}",
                    consecutive_failures,
                    MAX_CONSECUTIVE_FAILURES
                );
                let delay = if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    consecutive_failures = 0;
                    BACKOFF_AFTER_FAILURES_MS
                } else {
                    RETRY_DELAY_MS
                };
                if cancellable_sleep(delay, &cancel_token).await {
                    return;
                }
                continue;
            }
        };

        // 业务错误 (errcode != 0 或 ret != 0)
        let is_api_error = resp.ret != 0 || resp.errcode.map(|c| c != 0).unwrap_or(false);
        if is_api_error {
            let is_session_expired = resp.errcode == Some(SESSION_EXPIRED_ERRCODE)
                || resp.ret == SESSION_EXPIRED_ERRCODE;
            if is_session_expired {
                log::error!(
                    "[wechat-runtime] session expired (errcode={:?}), pausing {}ms",
                    resp.errcode,
                    SESSION_EXPIRED_PAUSE_MS
                );
                on_status(
                    ChannelConnectionState::NeedsReauth,
                    Some("微信会话已过期，请重新扫码登录".into()),
                );
                if cancellable_sleep(SESSION_EXPIRED_PAUSE_MS, &cancel_token).await {
                    return;
                }
                continue;
            }
            consecutive_failures += 1;
            log::warn!(
                "[wechat-runtime] getUpdates business error ret={} errcode={:?} errmsg={:?} ({}/{})",
                resp.ret,
                resp.errcode,
                resp.errmsg,
                consecutive_failures,
                MAX_CONSECUTIVE_FAILURES
            );
            let delay = if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                consecutive_failures = 0;
                BACKOFF_AFTER_FAILURES_MS
            } else {
                RETRY_DELAY_MS
            };
            if cancellable_sleep(delay, &cancel_token).await {
                return;
            }
            continue;
        }

        consecutive_failures = 0;
        if !emitted_connected {
            emitted_connected = true;
            on_status(ChannelConnectionState::Connected, None);
            log::info!("[wechat-runtime] first getUpdates success, connector connected");
        }

        // [diag] 每轮成功响应的摘要，定位"消息没到/被过滤"用。
        log::info!(
            "[wechat-runtime] getUpdates ok ret={} errcode={:?} msgs={} buf_in_len={} buf_out_len={:?} longpolling_timeout_ms={:?}",
            resp.ret,
            resp.errcode,
            resp.msgs.len(),
            get_updates_buf.len(),
            resp.get_updates_buf.as_ref().map(|s| s.len()),
            resp.longpolling_timeout_ms,
        );

        // Update sync cursor + server-suggested next timeout.
        if let Some(new_buf) = resp.get_updates_buf.filter(|s| !s.is_empty()) {
            get_updates_buf = new_buf;
        }
        if let Some(ms) = resp.longpolling_timeout_ms.filter(|v| *v > 0) {
            next_timeout_secs = (ms / 1000).max(5).min(120);
        }

        // 处理消息
        for msg in resp.msgs {
            let diag_msg_id = msg
                .message_id
                .map(|n| n.to_string())
                .unwrap_or_else(|| "<no-id>".to_string());
            let diag_from = msg.from_user_id.clone().unwrap_or_default();
            log::info!(
                "[wechat-runtime] candidate msg_id={} from={} type={:?} state={:?} items={} ctx={}",
                diag_msg_id,
                diag_from,
                msg.message_type,
                msg.message_state,
                msg.item_list.len(),
                msg.context_token.is_some(),
            );
            // 过滤 bot 自己发的 echo（message_type=2）。
            if msg.message_type == Some(2) {
                log::info!(
                    "[wechat-runtime] skip msg_id={} reason=echo_message_type_2",
                    diag_msg_id
                );
                continue;
            }
            // 状态过滤：只关心终态 (FINISH=2) 和无状态字段的；GENERATING 跳过。
            if let Some(state) = msg.message_state {
                if state == 1 {
                    log::info!(
                        "[wechat-runtime] skip msg_id={} reason=state_generating(1)",
                        diag_msg_id
                    );
                    continue;
                }
            }
            // from_user_id 是回信 to_user_id 必需字段；缺失就跳过。
            let Some(from_user_id) = msg.from_user_id.filter(|s| !s.is_empty()) else {
                log::info!(
                    "[wechat-runtime] skip msg_id={} reason=from_user_id_empty",
                    diag_msg_id
                );
                continue;
            };
            // NOTE: 不用 `from_user_id == cfg.self_user_id` 过滤 echo —— 在
            // iLink 个人微信协议里，`from_user_id` 是 1v1 会话「对端」的 wxid。
            // 当用户用「扫码登录的那个微信号本人」给自己的 bot 发消息时，
            // from_user_id 就是 self_user_id，被这条过滤误伤会导致永远收不到
            // 自己发给 bot 的测试消息。Bot 自己发出去的 echo 走 message_type==2
            // 那条分支识别，已经在上面处理。
            let msg_id_str = msg
                .message_id
                .map(|n| n.to_string())
                .unwrap_or_else(|| format!("wechat-noid-{}", from_user_id));
            // connector-level dedup
            if !dedup.observe(&msg_id_str).await {
                log::info!(
                    "[wechat-runtime] skip msg_id={} reason=dedup_hit",
                    msg_id_str
                );
                continue;
            }

            let text_raw = api::flatten_item_list_to_text(&msg.item_list);
            let attachments = api::extract_attachments_from_item_list(&msg.item_list, &msg_id_str);
            // 当存在可下载附件时，把 text 里的占位串（`[图片]/[文件]/...`）清空，
            // 由下游 manager 用真附件构造给 LLM 看的 content。如果附件全部下载失败，
            // manager 会兜底走 "附件下载全部失败" 文本回信，不会让 LLM 看到空内容。
            // 占位串只在**没有**任何可下载附件（比如只发了 voice/video 这类本期不支持的）
            // 时保留，避免 build_compound_content 触发 Anthropic 400。
            let text = if attachments.is_empty() {
                text_raw
            } else {
                String::new()
            };
            if text.is_empty() && attachments.is_empty() {
                log::info!(
                    "[wechat-runtime] skip msg_id={} reason=text_empty from={} items={}",
                    msg_id_str,
                    from_user_id,
                    msg.item_list.len()
                );
                continue;
            }

            let channel_msg = ChannelMessage {
                msg_id: msg_id_str,
                native_message_id: None,
                conversation_type: ConversationType::Private,
                conversation_key: from_user_id.clone(),
                sender_id: from_user_id.clone(),
                sender_nick: from_user_id.clone(),
                text,
                // manager 拿 ilink_bot_id 当 router_key；这里不知道 bot id
                // 是哪个，留空让 manager 在 worker 那侧用 cfg-side bot id 覆盖。
                robot_code: cfg.ilink_bot_id.clone(),
                reply_group_id: from_user_id,
                attachments,
                session_webhook: None,
                created_at_ms: msg.create_time_ms,
            };

            log::info!(
                "[wechat-runtime] inbound msg_id={} text_len={} attachments={}",
                channel_msg.msg_id,
                channel_msg.text.len(),
                channel_msg.attachments.len()
            );
            let event = WechatInboundEvent {
                message: channel_msg,
                context_token: msg.context_token,
            };
            if let Err(e) = msg_tx.send(event).await {
                log::warn!("[wechat-runtime] msg_tx closed: {e}; exiting worker");
                return;
            }
        }
    }
}

/// 跟 cancel_token 协同的 sleep —— 返回 true 表示被取消，调用方应直接 return。
async fn cancellable_sleep(ms: u64, cancel_token: &CancellationToken) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(ms)) => false,
        _ = cancel_token.cancelled() => true,
    }
}

// Re-export the target type so connector.rs has a single import point.
pub use crate::connector::im::wechat::types::WechatSessionTarget as _WechatSessionTarget;
#[allow(dead_code)]
type _Unused = WechatSessionTarget;
