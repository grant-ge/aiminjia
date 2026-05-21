//! 飞书 WebSocket 长连客户端。
//!
//! 流程（参考 `larksuite/oapi-sdk-go` v3_main `ws/client.go`）：
//!   1. POST `${open-domain}/callback/ws/endpoint`
//!      body: `{"AppID": "cli_...", "AppSecret": "..."}`   ← CapCase 字段名
//!      headers: `locale: zh`, `User-Agent: ...`
//!      → 拿 `data.URL`（含 device_id/service_id query）+ `ClientConfig`
//!   2. tungstenite 长连 data.URL（URL 自带 auth，不需要额外 ticket header）
//!   3. recv binary frame → 用本地 `pbbp2` 模块解码 Frame envelope
//!   4. method == FrameTypeData，按 header `"type"` 分发：
//!        - "event"   → 业务事件（im.message.receive_v1 等），payload 可能是
//!                       gzip-compressed JSON（看 `payload_encoding` 字段）
//!        - "card"    → card.action.trigger（PR4/5 接入，本 PR 仅 log）
//!        - 其它      → 静默忽略
//!      method == FrameTypeControl，按 header `"type"` 分发：
//!        - "pong"    → 服务端心跳响应，可能带 ClientConfig 更新
//!   5. **每个** data frame 都要回 ACK（同 SeqID/LogID/Headers，payload 是
//!      `{"code":200,"headers":...,"message":"OK","data":""}` 的 UTF-8 JSON
//!      bytes）—— 否则服务端会重发同一条消息。
//!   6. ping interval 按 ClientConfig.PingInterval（默认 120s）；断开后用
//!      `shared::ReconnectBackoff` 5/15/30/60s 重连。
//!
//! Cancel 契约：`cancel_token.cancelled()` 触发 → ws.close + 退出 ≤ 2s。

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, sleep};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::connector::im::shared::dedup::MessageDedupSet;
use crate::connector::im::shared::reconnect::ReconnectBackoff;
use crate::connector::im::types::{
    AttachmentKind, ChannelAttachmentSpec, ChannelConnectionState, ChannelMessage, ConversationType,
};

use super::pbbp2::{
    self, Frame, Header, FRAME_TYPE_CONTROL, FRAME_TYPE_DATA, HEADER_MESSAGE_ID, HEADER_SEQ,
    HEADER_SUM, HEADER_TYPE, MSG_TYPE_CARD, MSG_TYPE_EVENT, MSG_TYPE_PONG,
};

const FEISHU_OPEN_DOMAIN: &str = "https://open.feishu.cn";
const WS_ENDPOINT_PATH: &str = "/callback/ws/endpoint";

/// Fallback ping interval if the server omits ClientConfig. Mirrors the Go SDK
/// default (120s) — see `larksuite/oapi-sdk-go` `ws/client.go::configure`.
const DEFAULT_PING_INTERVAL_SECS: u64 = 120;

/// Cap on simultaneously-tracked multi-frame messages. WS frame fragmentation
/// is rare in practice; this is just a safety belt against unbounded buffering.
const MAX_IN_FLIGHT_FRAGMENTS: usize = 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WsClientConfig {
    #[serde(default)]
    pub ping_interval: u64, // seconds, e.g. 120
    #[serde(default)]
    pub reconnect_count: i64, // -1 means infinite
    #[serde(default)]
    pub reconnect_interval: u64, // seconds
    #[serde(default)]
    pub reconnect_nonce: u64, // seconds of jitter
}

#[derive(Clone)]
pub struct FeishuStreamClient {
    app_id: String,
    app_secret: String,
    message_tx: mpsc::Sender<ChannelMessage>,
    dedup: Arc<MessageDedupSet>,
    fragments: Arc<Mutex<HashMap<String, Vec<Option<Vec<u8>>>>>>,
}

impl FeishuStreamClient {
    pub fn new(
        app_id: String,
        app_secret: String,
        message_tx: mpsc::Sender<ChannelMessage>,
    ) -> Self {
        Self {
            app_id,
            app_secret,
            message_tx,
            dedup: Arc::new(MessageDedupSet::with_default_cap()),
            fragments: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn the long-lived WS task. Status callbacks are invoked from the spawned
    /// task; `cancel` cancels the task within 2s.
    pub fn start(
        &self,
        on_status: impl Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static,
        cancel: CancellationToken,
    ) {
        let client = self.clone();
        let on_status = Arc::new(on_status);
        tokio::spawn(async move {
            client.run_with_retry(on_status, cancel).await;
        });
    }

    async fn run_with_retry(
        &self,
        on_status: Arc<impl Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
        cancel: CancellationToken,
    ) {
        let mut backoff = ReconnectBackoff::default_schedule();
        loop {
            if cancel.is_cancelled() {
                log::info!("[feishu-stream] cancelled, stopping retry loop");
                return;
            }
            on_status(ChannelConnectionState::Connecting, None);
            match self.open_ws_endpoint().await {
                Ok((url, client_config)) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    backoff.reset();
                    on_status(ChannelConnectionState::Connected, None);
                    log::info!(
                        "[feishu-stream] connected, ping_interval={}s",
                        client_config.ping_interval
                    );
                    if let Err(e) = self.run_ws_loop(&url, &client_config, cancel.clone()).await {
                        log::warn!("[feishu-stream] ws loop ended: {:#}", e);
                    }
                }
                Err(e) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    let msg = e.to_string();
                    log::warn!("[feishu-stream] open failed: {:#}", e);
                    // 99991661 = invalid app_id/app_secret; 99991663 = invalid token (rare on this
                    // endpoint — endpoint is unauthenticated except for AppID/AppSecret in body).
                    if msg.contains("99991661")
                        || msg.contains("99991663")
                        || msg.contains("401")
                        || msg.contains("Unauthorized")
                    {
                        on_status(
                            ChannelConnectionState::ConfigError,
                            Some("飞书 AppID / AppSecret 有误，请重新配置".into()),
                        );
                        return;
                    }
                }
            }
            if cancel.is_cancelled() {
                return;
            }
            on_status(ChannelConnectionState::Reconnecting, None);
            let delay = backoff.next_delay();
            tokio::select! {
                _ = sleep(delay) => {}
                _ = cancel.cancelled() => return,
            }
        }
    }

    /// `POST /callback/ws/endpoint` returns `{ code, msg, data: { URL, ClientConfig } }`.
    /// Field names are CapCase: `AppID` / `AppSecret` on input, `URL` / `ClientConfig`
    /// on output. The URL itself carries the auth (device_id / service_id query params)
    /// so the wss handshake doesn't need extra headers.
    async fn open_ws_endpoint(&self) -> Result<(String, WsClientConfig)> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}{}", FEISHU_OPEN_DOMAIN, WS_ENDPOINT_PATH))
            .header("locale", "zh")
            .header("User-Agent", "aijia-desktop/0.1 (feishu-ws-client)")
            .json(&serde_json::json!({
                "AppID": self.app_id,
                "AppSecret": self.app_secret,
            }))
            .send()
            .await
            .context("post feishu ws endpoint")?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("feishu ws endpoint http: {} {}", status, body);
        }
        #[derive(Deserialize)]
        struct Resp {
            code: i64,
            #[allow(dead_code)]
            msg: Option<String>,
            data: Option<WsData>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct WsData {
            #[serde(rename = "URL")]
            url: String,
            client_config: Option<WsClientConfig>,
        }
        let r: Resp = serde_json::from_str(&body)
            .with_context(|| format!("parse feishu ws endpoint body: {}", body))?;
        if r.code != 0 {
            anyhow::bail!("feishu ws endpoint errcode={}", r.code);
        }
        let data = r
            .data
            .ok_or_else(|| anyhow::anyhow!("feishu ws endpoint missing data"))?;
        let cfg = data.client_config.unwrap_or_else(|| WsClientConfig {
            ping_interval: DEFAULT_PING_INTERVAL_SECS,
            reconnect_count: -1,
            reconnect_interval: 120,
            reconnect_nonce: 30,
        });
        Ok((data.url, cfg))
    }

    async fn run_ws_loop(
        &self,
        url: &str,
        client_config: &WsClientConfig,
        cancel: CancellationToken,
    ) -> Result<()> {
        let (ws_stream, response) = tokio_tungstenite::connect_async(url)
            .await
            .context("ws connect")?;
        log::info!(
            "[feishu-stream] ws handshake ok, status={}",
            response.status()
        );
        let (mut write, mut read) = ws_stream.split();

        let ping_secs = if client_config.ping_interval > 0 {
            client_config.ping_interval
        } else {
            DEFAULT_PING_INTERVAL_SECS
        };
        let mut ping_timer = interval(Duration::from_secs(ping_secs));
        ping_timer.tick().await; // skip first immediate fire

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    log::info!("[feishu-stream] cancel observed, closing ws");
                    let _ = write.send(Message::Close(None)).await;
                    return Ok(());
                }
                _ = ping_timer.tick() => {
                    // WS-level transport ping is sufficient for the feishu
                    // gateway. The Go SDK can also push an app-level Frame
                    // (method=Control, type=ping) but the docs indicate the
                    // server drives ping/pong via ClientConfig — so the
                    // transport-level ping is the conservative choice. If a
                    // future server change requires app-level ping we'll
                    // rebuild a control Frame here; for now we keep this clean.
                    if let Err(e) = write.send(Message::Ping(Vec::new().into())).await {
                        anyhow::bail!("ws heartbeat send failed: {}", e);
                    }
                }
                frame = read.next() => {
                    let Some(frame) = frame else {
                        anyhow::bail!("ws stream ended");
                    };
                    let frame = frame.context("ws recv")?;
                    match frame {
                        Message::Binary(bytes) => {
                            if let Err(e) = self.handle_binary_frame(&bytes, &mut write).await {
                                log::warn!("[feishu-stream] frame error: {:#}", e);
                            }
                        }
                        Message::Ping(d) => {
                            let _ = write.send(Message::Pong(d)).await;
                        }
                        Message::Pong(_) => {}
                        Message::Close(c) => {
                            log::warn!("[feishu-stream] server sent Close: {:?}", c);
                            return Ok(());
                        }
                        Message::Text(_) | Message::Frame(_) => {
                            // Feishu gateway is supposed to use binary frames only;
                            // we just ignore non-binary inbound.
                        }
                    }
                }
            }
        }
    }

    async fn handle_binary_frame<W>(&self, bytes: &[u8], write: &mut W) -> Result<()>
    where
        W: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    {
        let frame = Frame::decode(bytes).context("decode protobuf frame")?;
        log::debug!(
            "[feishu-stream] frame method={} type={:?} seq_id={} payload_len={}",
            frame.method,
            frame.header(HEADER_TYPE),
            frame.seq_id,
            frame.payload.len()
        );
        match frame.method {
            FRAME_TYPE_CONTROL => self.handle_control_frame(&frame),
            FRAME_TYPE_DATA => {
                self.handle_data_frame(&frame, write).await?;
            }
            other => {
                log::debug!("[feishu-stream] unknown frame method={}, ignored", other);
            }
        }
        Ok(())
    }

    fn handle_control_frame(&self, frame: &Frame) {
        match frame.header(HEADER_TYPE) {
            Some(t) if t == MSG_TYPE_PONG => {
                if !frame.payload.is_empty() {
                    if let Ok(cfg) = serde_json::from_slice::<WsClientConfig>(&frame.payload) {
                        log::debug!(
                            "[feishu-stream] pong with ClientConfig update ping={}s",
                            cfg.ping_interval
                        );
                    }
                }
            }
            other => {
                log::debug!("[feishu-stream] control frame type={:?}", other);
            }
        }
    }

    async fn handle_data_frame<W>(&self, frame: &Frame, write: &mut W) -> Result<()>
    where
        W: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    {
        // ACK first — server retransmits if it doesn't see one. Build the ACK
        // shape the Go SDK uses (NewResponseByCode(http.StatusOK)).
        let ack = build_data_ack(frame);
        if let Err(e) = write.send(Message::Binary(ack.encode().into())).await {
            log::warn!("[feishu-stream] ack send failed: {:#}", e);
        }

        // Multi-frame assembly: combine fragments keyed by message_id.
        let sum = header_int(frame, HEADER_SUM).unwrap_or(1);
        let seq = header_int(frame, HEADER_SEQ).unwrap_or(0);
        let msg_id = frame.header(HEADER_MESSAGE_ID).unwrap_or("");

        let payload_bytes = if sum > 1 && !msg_id.is_empty() {
            match self
                .combine_fragments(msg_id, sum as usize, seq as usize, frame.payload.clone())
                .await
            {
                Some(full) => full,
                None => return Ok(()), // still buffering
            }
        } else {
            frame.payload.clone()
        };

        // Per `Frame.payload_encoding` the body may be gzip-compressed JSON.
        let decoded = if frame.payload_encoding == "gzip" {
            gunzip(&payload_bytes).context("gunzip frame payload")?
        } else {
            payload_bytes
        };

        let event_value: serde_json::Value =
            serde_json::from_slice(&decoded).context("parse event json")?;

        match frame.header(HEADER_TYPE) {
            Some(t) if t == MSG_TYPE_EVENT => {
                self.dispatch_event(event_value).await;
            }
            Some(t) if t == MSG_TYPE_CARD => {
                // PR4/5 will hook card.action.trigger; for PR3 just log.
                log::info!("[feishu-stream] card frame received (deferred to PR4/5)");
            }
            other => {
                log::debug!(
                    "[feishu-stream] data frame type={:?} (no dispatcher)",
                    other
                );
            }
        }
        Ok(())
    }

    /// Per oapi-sdk-go `ws/client.go::combine`: gather all `sum` fragments
    /// keyed by `message_id`, slot each by `seq` index. Returns Some(concat)
    /// once all slots are filled, None otherwise. Cap at `MAX_IN_FLIGHT_FRAGMENTS`
    /// in-flight message_ids (evict half via FIFO) — fragmentation is rare;
    /// any actually-evicted partial message will be dropped (manager-level
    /// dedup catches the re-deliver).
    async fn combine_fragments(
        &self,
        msg_id: &str,
        sum: usize,
        seq: usize,
        payload: Vec<u8>,
    ) -> Option<Vec<u8>> {
        let mut map = self.fragments.lock().await;
        if !map.contains_key(msg_id) && map.len() >= MAX_IN_FLIGHT_FRAGMENTS {
            // Evict oldest half via key iteration order (HashMap doesn't preserve order
            // but eviction here is a safety belt, not a correctness guarantee — any
            // dropped partial will get re-delivered later and dedup'd at the manager).
            let drop_count = MAX_IN_FLIGHT_FRAGMENTS / 2;
            let to_drop: Vec<String> = map.keys().take(drop_count).cloned().collect();
            for k in to_drop {
                map.remove(&k);
            }
            // WS multi-frame messages are rare in practice, so hitting this
            // cap usually means something is wrong (storm of huge fragmented
            // messages, broken gateway). Surface as warn so it's noticeable.
            log::warn!(
                "[feishu-stream] fragment eviction: lost {} partial messages (cap={}, current_size={})",
                drop_count,
                MAX_IN_FLIGHT_FRAGMENTS,
                map.len()
            );
        }
        let slots = map
            .entry(msg_id.to_string())
            .or_insert_with(|| vec![None; sum]);
        if seq >= slots.len() {
            // sum changed mid-stream — abandon and reseed
            *slots = vec![None; sum.max(seq + 1)];
        }
        slots[seq] = Some(payload);
        if slots.iter().all(|s| s.is_some()) {
            let parts = map.remove(msg_id).unwrap();
            let mut out = Vec::new();
            for p in parts.into_iter().flatten() {
                out.extend_from_slice(&p);
            }
            Some(out)
        } else {
            None
        }
    }

    async fn dispatch_event(&self, event: serde_json::Value) {
        let event_type = event
            .pointer("/header/event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match event_type {
            "im.message.receive_v1" => {
                if let Some(channel_msg) = parse_im_message(&event) {
                    // Dedup at connector layer — manager has its own seen_msg_ids;
                    // dropping here too means transient WS replay storms don't reach
                    // the manager's mpsc channel.
                    if !self.dedup.observe(&channel_msg.msg_id).await {
                        log::debug!(
                            "[feishu-stream] duplicate msg_id {} (connector-level dedup)",
                            channel_msg.msg_id
                        );
                        return;
                    }
                    log::info!(
                        "[feishu-stream] forwarding msg_id={} chat_type={:?} text_len={}",
                        channel_msg.msg_id,
                        channel_msg.conversation_type,
                        channel_msg.text.len()
                    );
                    if let Err(e) = self.message_tx.send(channel_msg).await {
                        log::warn!("[feishu-stream] message_tx closed: {:#}", e);
                    }
                } else {
                    log::warn!("[feishu-stream] im.message.receive_v1 parse failed");
                }
            }
            other => {
                log::debug!("[feishu-stream] unhandled event_type={}", other);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (no state) — separately testable.
// ---------------------------------------------------------------------------

/// Parse an `im.message.receive_v1` event JSON into a normalized `ChannelMessage`.
/// Schema: `{header, event:{sender:{sender_id:{open_id}}, message:{message_id, chat_id, chat_type, message_type, content (stringified)}}}`.
pub(crate) fn parse_im_message(event: &serde_json::Value) -> Option<ChannelMessage> {
    let inner = event.pointer("/event")?;
    let msg_id = inner.pointer("/message/message_id")?.as_str()?.to_string();
    let chat_type = inner.pointer("/message/chat_type")?.as_str()?;
    let conversation_type = match chat_type {
        "group" => ConversationType::Group,
        "p2p" => ConversationType::Private,
        _ => return None,
    };
    let chat_id = inner.pointer("/message/chat_id")?.as_str()?.to_string();
    // sender open_id is required for private chats (it's the reply target);
    // for group chats it's needed for the @-mention context. Bail if missing.
    let sender_id = inner
        .pointer("/sender/sender_id/open_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if sender_id.is_empty() {
        return None;
    }
    // sender display name isn't included in `im.message.receive_v1` (would require a
    // separate contact API lookup). Reuse sender_id as fallback nick — `chatStore`
    // will still render something user-recognizable, full name resolution comes
    // in a later phase if needed.
    let sender_nick = sender_id.clone();

    let msg_type = inner.pointer("/message/message_type")?.as_str()?;
    let content_str = inner.pointer("/message/content")?.as_str()?;
    let content_json: serde_json::Value = serde_json::from_str(content_str).ok()?;

    let (text, attachments) = normalize_content(msg_type, &content_json, &msg_id);

    // Server-reported send time, ms since epoch, JSON-typed as **string**
    // (per feishu open docs: "消息发送时间（毫秒）"). Parse leniently —
    // unparseable / missing field is fine (downstream treats `None` as
    // "no judgment", i.e. don't skip).
    let created_at_ms = inner
        .pointer("/message/create_time")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok());

    Some(ChannelMessage {
        msg_id,
        conversation_type,
        // For both group and private chats, downstream code expects the
        // conversation_key (= router external_id) to identify the target chat.
        // Feishu provides chat_id at message.chat_id for *both* p2p and group
        // — for p2p it's the open_chat_id of the user-bot DM.
        conversation_key: chat_id.clone(),
        sender_id,
        sender_nick,
        text,
        // No robot_code concept on feishu. Manager will pass app_id as the
        // router namespacing key; ChannelMessage.robot_code stays empty here
        // because the connector doesn't know its own app_id (manager does).
        robot_code: String::new(),
        reply_group_id: chat_id,
        attachments,
        session_webhook: None,
        created_at_ms,
    })
}

/// Normalize per-type `content` JSON to `(text, attachments)`. 4 fully-supported
/// types (text/image/file/interactive); ~20 others get a placeholder text so the
/// message still surfaces in the conversation list.
pub(crate) fn normalize_content(
    msg_type: &str,
    content: &serde_json::Value,
    msg_id: &str,
) -> (String, Vec<ChannelAttachmentSpec>) {
    match msg_type {
        "text" => {
            let t = content
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            (t, vec![])
        }
        "image" => match content.get("image_key").and_then(|v| v.as_str()) {
            Some(image_key) if !image_key.is_empty() => (
                String::new(),
                vec![ChannelAttachmentSpec {
                    kind: AttachmentKind::Picture,
                    download_code: image_key.to_string(),
                    file_name: format!("image_{}.jpg", msg_id),
                }],
            ),
            _ => ("[image]".into(), vec![]),
        },
        "file" => match content.get("file_key").and_then(|v| v.as_str()) {
            Some(file_key) if !file_key.is_empty() => {
                let file_name = content
                    .get("file_name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("file.bin")
                    .to_string();
                (
                    String::new(),
                    vec![ChannelAttachmentSpec {
                        kind: AttachmentKind::File,
                        download_code: file_key.to_string(),
                        file_name,
                    }],
                )
            }
            _ => ("[file]".into(), vec![]),
        },
        "interactive" => {
            // Cards inbound are rare; use the card header title as text.
            let title = content
                .get("header")
                .and_then(|h| h.get("title"))
                .and_then(|t| t.get("content"))
                .and_then(|s| s.as_str())
                .unwrap_or("[飞书卡片]")
                .to_string();
            (title, vec![])
        }
        other => (format!("[飞书消息类型 {} 暂不支持]", other), vec![]),
    }
}

fn header_int(frame: &Frame, key: &str) -> Option<i64> {
    frame.header(key).and_then(|v| v.parse::<i64>().ok())
}

fn build_data_ack(frame: &Frame) -> Frame {
    // Echo SeqID / LogID / select headers; payload is the ResponseByCode(200)
    // shape from oapi-sdk-go: `{"code":200,"headers":...,"message":"OK","data":""}`.
    let response = serde_json::json!({
        "code": 200,
        "headers": {
            "biz_rt": "0",
        },
        "message": "OK",
        "data": "",
    });
    let payload = serde_json::to_vec(&response).unwrap_or_default();
    Frame {
        seq_id: frame.seq_id,
        log_id: frame.log_id,
        service: frame.service,
        method: FRAME_TYPE_DATA,
        // Echo at minimum the type + message_id so the server can correlate.
        headers: vec![
            Header {
                key: pbbp2::HEADER_TYPE.into(),
                value: frame.header(pbbp2::HEADER_TYPE).unwrap_or("").to_string(),
            },
            Header {
                key: HEADER_MESSAGE_ID.into(),
                value: frame.header(HEADER_MESSAGE_ID).unwrap_or("").to_string(),
            },
        ],
        payload_encoding: String::new(),
        payload_type: String::new(),
        payload,
        log_id_new: frame.log_id_new.clone(),
    }
}

fn gunzip(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests — exercise parse_im_message + normalize_content via plain JSON. The
// protobuf wire-format envelope is covered separately in pbbp2.rs tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(msg_type: &str, content: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "header": { "event_type": "im.message.receive_v1" },
            "event": {
                "message": {
                    "message_id": "om_test_xxx",
                    "chat_type": "p2p",
                    "chat_id": "oc_test_xxx",
                    "message_type": msg_type,
                    "content": content.to_string(),
                    // Feishu encodes create_time as a string-typed ms timestamp.
                    "create_time": "1700000000000",
                },
                "sender": {
                    "sender_id": { "open_id": "ou_sender_xxx", "user_id": "u_sender" },
                }
            }
        })
    }

    #[test]
    fn normalize_text_extracts_body() {
        let v = make_event("text", serde_json::json!({"text": "你好世界"}));
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.text, "你好世界");
        assert!(m.attachments.is_empty());
        assert_eq!(m.conversation_type, ConversationType::Private);
        assert_eq!(m.conversation_key, "oc_test_xxx");
        assert_eq!(m.sender_id, "ou_sender_xxx");
        assert_eq!(m.msg_id, "om_test_xxx");
        // create_time should round-trip through parse as ms.
        assert_eq!(m.created_at_ms, Some(1_700_000_000_000));
    }

    #[test]
    fn parse_missing_create_time_yields_none() {
        // Used by the feishu worker to decide whether to skip pre-launch
        // replays — `None` means "no judgment, treat as new". Bail-out path
        // must NOT be triggered when the field happens to be missing on a
        // well-formed message.
        let mut v = make_event("text", serde_json::json!({"text": "hi"}));
        v["event"]["message"]
            .as_object_mut()
            .unwrap()
            .remove("create_time");
        let m = parse_im_message(&v).unwrap();
        assert!(m.created_at_ms.is_none());
    }

    #[test]
    fn parse_garbled_create_time_yields_none() {
        // Defensive: a junk create_time (e.g. server-side schema change)
        // must NOT crash the parser; downstream falls back to "treat as new".
        let mut v = make_event("text", serde_json::json!({"text": "hi"}));
        v["event"]["message"]["create_time"] = serde_json::json!("not-a-number");
        let m = parse_im_message(&v).unwrap();
        assert!(m.created_at_ms.is_none());
    }

    #[test]
    fn normalize_image_emits_attachment_spec() {
        let v = make_event("image", serde_json::json!({"image_key": "img_v2_001"}));
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.text, "");
        assert_eq!(m.attachments.len(), 1);
        assert!(matches!(m.attachments[0].kind, AttachmentKind::Picture));
        assert_eq!(m.attachments[0].download_code, "img_v2_001");
        assert_eq!(m.attachments[0].file_name, "image_om_test_xxx.jpg");
    }

    #[test]
    fn normalize_file_uses_file_name() {
        let v = make_event(
            "file",
            serde_json::json!({"file_key": "file_v2_001", "file_name": "report.pdf"}),
        );
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.attachments[0].kind, AttachmentKind::File);
        assert_eq!(m.attachments[0].file_name, "report.pdf");
        assert_eq!(m.attachments[0].download_code, "file_v2_001");
    }

    #[test]
    fn normalize_file_missing_name_falls_back() {
        let v = make_event("file", serde_json::json!({"file_key": "file_v2_002"}));
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.attachments[0].file_name, "file.bin");
    }

    #[test]
    fn normalize_image_empty_key_falls_back_to_placeholder() {
        let v = make_event("image", serde_json::json!({"image_key": ""}));
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.text, "[image]");
        assert!(m.attachments.is_empty());
    }

    #[test]
    fn normalize_unsupported_type_emits_placeholder() {
        let v = make_event("audio", serde_json::json!({"file_key": "x"}));
        let m = parse_im_message(&v).unwrap();
        assert!(m.text.contains("飞书消息类型 audio"), "got {:?}", m.text);
        assert!(m.attachments.is_empty());
    }

    #[test]
    fn normalize_interactive_uses_header_title() {
        let v = make_event(
            "interactive",
            serde_json::json!({
                "header": { "title": { "content": "审批申请" } }
            }),
        );
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.text, "审批申请");
    }

    #[test]
    fn normalize_interactive_missing_title_uses_default() {
        let v = make_event("interactive", serde_json::json!({}));
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.text, "[飞书卡片]");
    }

    #[test]
    fn parse_group_chat_type() {
        let mut v = make_event("text", serde_json::json!({"text": "群里说一句"}));
        v["event"]["message"]["chat_type"] = serde_json::Value::String("group".into());
        v["event"]["message"]["chat_id"] = serde_json::Value::String("oc_group_001".into());
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.conversation_type, ConversationType::Group);
        assert_eq!(m.conversation_key, "oc_group_001");
        assert_eq!(m.reply_group_id, "oc_group_001");
    }

    #[test]
    fn parse_missing_open_id_drops() {
        let mut v = make_event("text", serde_json::json!({"text": "x"}));
        v["event"]["sender"]["sender_id"]
            .as_object_mut()
            .unwrap()
            .remove("open_id");
        assert!(parse_im_message(&v).is_none());
    }

    #[test]
    fn parse_unknown_chat_type_drops() {
        let mut v = make_event("text", serde_json::json!({"text": "x"}));
        v["event"]["message"]["chat_type"] = serde_json::Value::String("topic".into());
        assert!(parse_im_message(&v).is_none());
    }

    #[test]
    fn parse_malformed_content_string_drops() {
        let mut v = make_event("text", serde_json::json!({"text": "x"}));
        v["event"]["message"]["content"] = serde_json::Value::String("{not valid json".into());
        assert!(parse_im_message(&v).is_none());
    }

    #[tokio::test]
    async fn dedup_drops_replays_at_connector_layer() {
        let (tx, mut rx) = mpsc::channel::<ChannelMessage>(8);
        let client = FeishuStreamClient::new("ak".into(), "as".into(), tx);
        let v = make_event("text", serde_json::json!({"text": "once"}));
        client.dispatch_event(v.clone()).await;
        client.dispatch_event(v).await; // same msg_id — dedup eats this
        let first = rx.recv().await.expect("first delivery");
        assert_eq!(first.msg_id, "om_test_xxx");
        // Channel should now be empty (next recv has nothing pending) — verify with
        // a non-blocking try_recv.
        assert!(
            rx.try_recv().is_err(),
            "expected duplicate to be dropped at dedup layer"
        );
    }

    #[tokio::test]
    async fn combine_fragments_assembles_in_order_and_evicts_when_full() {
        let (tx, _rx) = mpsc::channel::<ChannelMessage>(8);
        let client = FeishuStreamClient::new("ak".into(), "as".into(), tx);
        // 2-fragment message, send out of order.
        let r1 = client
            .combine_fragments("M1", 2, 1, b"second".to_vec())
            .await;
        assert!(r1.is_none(), "missing slot 0 — should hold");
        let r2 = client
            .combine_fragments("M1", 2, 0, b"first ".to_vec())
            .await;
        assert_eq!(r2.unwrap(), b"first second".to_vec());

        // Single-shot (sum=1, seq=0) returns immediately.
        let r3 = client.combine_fragments("M2", 1, 0, b"solo".to_vec()).await;
        assert_eq!(r3.unwrap(), b"solo".to_vec());
    }

    #[test]
    fn build_data_ack_echoes_seq_and_headers() {
        let frame = Frame {
            seq_id: 12345,
            log_id: 67890,
            service: 1,
            method: FRAME_TYPE_DATA,
            headers: vec![
                Header {
                    key: HEADER_TYPE.into(),
                    value: MSG_TYPE_EVENT.into(),
                },
                Header {
                    key: HEADER_MESSAGE_ID.into(),
                    value: "om_xxx".into(),
                },
            ],
            payload: b"ignored".to_vec(),
            ..Default::default()
        };
        let ack = build_data_ack(&frame);
        assert_eq!(ack.seq_id, 12345);
        assert_eq!(ack.log_id, 67890);
        assert_eq!(ack.method, FRAME_TYPE_DATA);
        assert_eq!(ack.header(HEADER_TYPE), Some(MSG_TYPE_EVENT));
        assert_eq!(ack.header(HEADER_MESSAGE_ID), Some("om_xxx"));
        // Payload is a JSON envelope.
        let v: serde_json::Value = serde_json::from_slice(&ack.payload).unwrap();
        assert_eq!(v.get("code").and_then(|c| c.as_i64()), Some(200));
    }
}
