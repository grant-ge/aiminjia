//! aibot WebSocket 连接管理层。
//!
//! 职责：
//! - 主动外连 `ws_url`（默认 `wss://openws.work.weixin.qq.com`）+ 发首帧 `aibot_subscribe` 认证
//! - 心跳：每 `heartbeat_interval` 发 `ping`，连续 `max_missed_pong` 次未收 pong 视为死连接
//! - 重连：物理 drop 走 ReconnectBackoff（max_reconnect_attempts）；认证失败独立计数
//!   （max_auth_failure_attempts）；收到 `disconnected_event` 不重连（KickedOut）
//! - 出站串行：同 req_id 出站帧按 FIFO 串行，前一帧 ack/超时后才发下一帧
//!
//! 调用方通过 mpsc::Sender<AibotEvent> 消费帧；通过 `respond` / `send_msg` 发送出站。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::aibot_protocol::*;

#[derive(Debug, Clone)]
pub struct AibotClientConfig {
    pub bot_id: String,
    pub secret: String,
    pub ws_url: String,
    pub heartbeat_interval: Duration,
    pub reply_ack_timeout: Duration,
    pub max_missed_pong: usize,
    pub max_reconnect_attempts: usize,
    pub max_auth_failure_attempts: usize,
    pub reconnect_base_delay: Duration,
}

impl AibotClientConfig {
    pub fn production(bot_id: String, secret: String) -> Self {
        Self {
            bot_id,
            secret,
            ws_url: "wss://openws.work.weixin.qq.com".into(),
            heartbeat_interval: Duration::from_secs(30),
            reply_ack_timeout: Duration::from_secs(10),
            max_missed_pong: 3,
            max_reconnect_attempts: 10,
            max_auth_failure_attempts: 5,
            reconnect_base_delay: Duration::from_secs(1),
        }
    }
}

#[derive(Debug)]
pub enum AibotEvent {
    Authenticated,
    /// 服务端推送的消息或事件帧（aibot_msg_callback / aibot_event_callback）。
    Inbound(WsFrame<serde_json::Value>),
    /// 收到 disconnected_event，服务端主动踢——调用方应停止重连。
    KickedOut(String),
    /// 物理连接断（网络 / 心跳超时）——client 内部会自动重连，调用方仅需 log。
    ConnectionDropped(String),
    /// 认证 ack errcode != 0——独立计数器，超限后 run() 退出。
    AuthFailed(i32, String),
    /// 重连前发出，attempt 从 1 起，方便上层 log。
    Reconnecting(usize),
}

/// 单个 req_id 的串行 ack 队列。
struct ReplyQueue {
    pending: VecDeque<(serde_json::Value, &'static str, oneshot::Sender<Result<()>>)>,
    in_flight: Option<oneshot::Sender<Result<()>>>,
}

pub struct AibotClient {
    cfg: AibotClientConfig,
    /// 出站发送：内部由 run() 持有 writer 半，外部通过 `outbound_tx` 投递。
    outbound_tx: Mutex<Option<mpsc::Sender<OutboundCmd>>>,
}

/// 跨 run() / send 边界的出站命令。
enum OutboundCmd {
    /// 走 ack 队列：(req_id, body, cmd, done)
    Reply(
        String,
        serde_json::Value,
        &'static str,
        oneshot::Sender<Result<()>>,
    ),
}

impl AibotClient {
    pub fn new(cfg: AibotClientConfig) -> Self {
        Self {
            cfg,
            outbound_tx: Mutex::new(None),
        }
    }

    /// 启动 client 主循环。
    /// `event_tx` 用来接收 AibotEvent；`cancel_token` 取消时主动关 WS + 退出 run()。
    pub async fn run(
        self: Arc<Self>,
        event_tx: mpsc::Sender<AibotEvent>,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let mut connection_attempts = 0usize;
        let mut auth_failure_attempts = 0usize;

        loop {
            if cancel_token.is_cancelled() {
                log::info!("[wecom-aibot] cancel before connect, exit");
                return Ok(());
            }
            if connection_attempts > 0 {
                let _ = event_tx
                    .send(AibotEvent::Reconnecting(connection_attempts))
                    .await;
            }

            match self
                .clone()
                .connect_and_pump(&event_tx, &cancel_token)
                .await
            {
                Ok(LoopExit::Kicked) => return Ok(()),
                Ok(LoopExit::Cancelled) => return Ok(()),
                Ok(LoopExit::AuthFailed(code, msg)) => {
                    auth_failure_attempts += 1;
                    let _ = event_tx
                        .send(AibotEvent::AuthFailed(code, msg.clone()))
                        .await;
                    if auth_failure_attempts >= self.cfg.max_auth_failure_attempts {
                        log::error!("[wecom-aibot] auth failure exhausted");
                        return Err(anyhow!("auth failure exhausted: {msg}"));
                    }
                }
                Ok(LoopExit::Dropped(reason)) => {
                    connection_attempts += 1;
                    let _ = event_tx.send(AibotEvent::ConnectionDropped(reason)).await;
                    if connection_attempts >= self.cfg.max_reconnect_attempts {
                        log::error!("[wecom-aibot] reconnect attempts exhausted");
                        return Err(anyhow!("reconnect attempts exhausted"));
                    }
                }
                Err(e) => {
                    connection_attempts += 1;
                    let _ = event_tx
                        .send(AibotEvent::ConnectionDropped(format!("{e:#}")))
                        .await;
                    if connection_attempts >= self.cfg.max_reconnect_attempts {
                        return Err(e);
                    }
                }
            }

            // 退避后重连
            let shift = connection_attempts.min(6) as u32;
            let delay = self.cfg.reconnect_base_delay * (1u32 << shift).max(1);
            tokio::select! {
                _ = sleep(delay) => {}
                _ = cancel_token.cancelled() => return Ok(()),
            }
        }
    }

    /// 投递一帧 respond_msg。`req_id` 来自收到的入站帧 headers.req_id。
    pub async fn respond(&self, req_id: &str, body: serde_json::Value) -> Result<()> {
        self.send_via_queue(req_id, body, "aibot_respond_msg").await
    }

    /// 投递一帧 send_msg（主动推送），生成新 req_id。
    pub async fn send_msg(&self, body: serde_json::Value) -> Result<()> {
        let req_id = generate_req_id("aibot_send_msg");
        self.send_via_queue(&req_id, body, "aibot_send_msg").await
    }

    async fn send_via_queue(
        &self,
        req_id: &str,
        body: serde_json::Value,
        cmd: &'static str,
    ) -> Result<()> {
        let tx = self
            .outbound_tx
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("aibot client not running"))?;
        let (done_tx, done_rx) = oneshot::channel();
        tx.send(OutboundCmd::Reply(req_id.to_string(), body, cmd, done_tx))
            .await
            .map_err(|_| anyhow!("aibot client outbound channel closed"))?;
        let wait = tokio::time::timeout(self.cfg.reply_ack_timeout, done_rx)
            .await
            .map_err(|_| anyhow!("ack timeout for req_id {req_id}"))?;
        wait.map_err(|_| anyhow!("ack pipe dropped"))?
    }

    async fn connect_and_pump(
        self: Arc<Self>,
        event_tx: &mpsc::Sender<AibotEvent>,
        cancel_token: &CancellationToken,
    ) -> Result<LoopExit> {
        let (ws_stream, _resp) = tokio_tungstenite::connect_async(&self.cfg.ws_url)
            .await
            .context("ws connect failed")?;
        let (mut writer, mut reader) = ws_stream.split();

        // ack 队列：req_id → ReplyQueue
        let queues: Arc<Mutex<HashMap<String, ReplyQueue>>> = Arc::new(Mutex::new(HashMap::new()));
        let (out_tx, mut out_rx) = mpsc::channel::<OutboundCmd>(64);
        *self.outbound_tx.lock().await = Some(out_tx.clone());

        // 发首帧认证
        let subscribe_req_id = generate_req_id("aibot_subscribe");
        let subscribe = WsFrame::<serde_json::Value> {
            cmd: Some(WsCmd::Subscribe),
            headers: FrameHeaders {
                req_id: subscribe_req_id.clone(),
                extra: Default::default(),
            },
            body: Some(serde_json::to_value(SubscribeBody {
                secret: self.cfg.secret.clone(),
                bot_id: self.cfg.bot_id.clone(),
            })?),
            errcode: None,
            errmsg: None,
        };
        writer
            .send(Message::Text(serde_json::to_string(&subscribe)?.into()))
            .await?;

        let mut authenticated = false;
        let mut missed_pong = 0usize;
        let mut last_ping_req_id: Option<String> = None;
        let mut heartbeat = tokio::time::interval(self.cfg.heartbeat_interval);
        heartbeat.tick().await; // 跳第一次立即触发

        loop {
            tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    let _ = writer.close().await;
                    *self.outbound_tx.lock().await = None;
                    return Ok(LoopExit::Cancelled);
                }
                Some(cmd) = out_rx.recv() => {
                    match cmd {
                        OutboundCmd::Reply(req_id, body, cmd_str, done) => {
                            let mut q = queues.lock().await;
                            let entry = q.entry(req_id.clone()).or_insert_with(|| ReplyQueue {
                                pending: VecDeque::new(),
                                in_flight: None,
                            });
                            entry.pending.push_back((body, cmd_str, done));
                            if entry.in_flight.is_none() {
                                // 立即出队发一条
                                if let Some((body, cmd_str, done)) = entry.pending.pop_front() {
                                    entry.in_flight = Some(done);
                                    let cmd_enum = match cmd_str {
                                        "aibot_respond_msg" => WsCmd::Respond,
                                        "aibot_send_msg" => WsCmd::SendMsg,
                                        _ => unreachable!(),
                                    };
                                    let f = WsFrame::<serde_json::Value> {
                                        cmd: Some(cmd_enum),
                                        headers: FrameHeaders { req_id: req_id.clone(), extra: Default::default() },
                                        body: Some(body),
                                        errcode: None, errmsg: None,
                                    };
                                    drop(q);
                                    if let Err(e) = writer.send(Message::Text(serde_json::to_string(&f)?.into())).await {
                                        *self.outbound_tx.lock().await = None;
                                        return Ok(LoopExit::Dropped(format!("write failed: {e}")));
                                    }
                                }
                            }
                        }
                    }
                }
                _ = heartbeat.tick() => {
                    if authenticated {
                        if last_ping_req_id.is_some() {
                            missed_pong += 1;
                            if missed_pong >= self.cfg.max_missed_pong {
                                let _ = writer.close().await;
                                *self.outbound_tx.lock().await = None;
                                return Ok(LoopExit::Dropped("heartbeat timeout".into()));
                            }
                        }
                        let req_id = generate_req_id("ping");
                        last_ping_req_id = Some(req_id.clone());
                        let f = WsFrame::<serde_json::Value> {
                            cmd: Some(WsCmd::Ping),
                            headers: FrameHeaders { req_id, extra: Default::default() },
                            body: None,
                            errcode: None, errmsg: None,
                        };
                        if let Err(e) = writer.send(Message::Text(serde_json::to_string(&f)?.into())).await {
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Dropped(format!("ping write failed: {e}")));
                        }
                    }
                }
                msg = reader.next() => {
                    let msg = match msg {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => {
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Dropped(format!("ws read err: {e}")));
                        }
                        None => {
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Dropped("ws closed".into()));
                        }
                    };
                    let text = match msg {
                        Message::Text(t) => t.to_string(),
                        Message::Close(_) => {
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Dropped("ws close frame".into()));
                        }
                        _ => continue,
                    };
                    let frame: WsFrame<serde_json::Value> = match serde_json::from_str(&text) {
                        Ok(f) => f,
                        Err(e) => { log::warn!("[wecom-aibot] bad frame: {e}; text={text}"); continue; }
                    };

                    // 1) 认证 ack（无 cmd + req_id == subscribe）
                    if frame.cmd.is_none() && frame.headers.req_id == subscribe_req_id {
                        let code = frame.errcode.unwrap_or(0);
                        if code == 0 {
                            authenticated = true;
                            let _ = event_tx.send(AibotEvent::Authenticated).await;
                        } else {
                            let msg = frame.errmsg.unwrap_or_default();
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::AuthFailed(code, msg));
                        }
                        continue;
                    }

                    // 2) 心跳 ack
                    if frame.cmd.is_none() && last_ping_req_id.as_deref() == Some(frame.headers.req_id.as_str()) {
                        last_ping_req_id = None;
                        missed_pong = 0;
                        continue;
                    }

                    // 3) 回复 ack（reply queue 内）
                    if frame.cmd.is_none() {
                        let mut q = queues.lock().await;
                        if let Some(entry) = q.get_mut(&frame.headers.req_id) {
                            if let Some(done) = entry.in_flight.take() {
                                let code = frame.errcode.unwrap_or(0);
                                if code == 0 {
                                    let _ = done.send(Ok(()));
                                } else {
                                    let _ = done.send(Err(anyhow!(
                                        "errcode={} errmsg={}",
                                        code,
                                        frame.errmsg.clone().unwrap_or_default()
                                    )));
                                }
                                // 出队下一条
                                if let Some((body, cmd_str, done)) = entry.pending.pop_front() {
                                    entry.in_flight = Some(done);
                                    let cmd_enum = match cmd_str {
                                        "aibot_respond_msg" => WsCmd::Respond,
                                        "aibot_send_msg" => WsCmd::SendMsg,
                                        _ => unreachable!(),
                                    };
                                    let f = WsFrame::<serde_json::Value> {
                                        cmd: Some(cmd_enum),
                                        headers: FrameHeaders { req_id: frame.headers.req_id.clone(), extra: Default::default() },
                                        body: Some(body),
                                        errcode: None, errmsg: None,
                                    };
                                    drop(q);
                                    if let Err(e) = writer.send(Message::Text(serde_json::to_string(&f)?.into())).await {
                                        *self.outbound_tx.lock().await = None;
                                        return Ok(LoopExit::Dropped(format!("write failed: {e}")));
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // 4) 服务端推送：先检测 disconnected_event，否则发 Inbound
                    if frame.cmd == Some(WsCmd::EventCallback) {
                        if frame.body.as_ref()
                            .and_then(|b| b.pointer("/event/eventtype"))
                            .and_then(|v| v.as_str()) == Some("disconnected_event")
                        {
                            let _ = event_tx.send(AibotEvent::KickedOut("server disconnected_event".into())).await;
                            let _ = writer.close().await;
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Kicked);
                        }
                    }
                    let _ = event_tx.send(AibotEvent::Inbound(frame)).await;
                }
            }
        }
    }
}

enum LoopExit {
    Cancelled,
    Kicked,
    AuthFailed(i32, String),
    Dropped(String),
}
