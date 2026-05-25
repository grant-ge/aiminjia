//! WebSocket 连接生命周期集成测试。
//!
//! 用 `tokio-tungstenite::accept_async` 起 server 端，按 aibot 协议响应：
//! - 收到 `aibot_subscribe` 帧 → 回 `{ headers: { req_id }, errcode: 0, errmsg: "ok" }`
//! - 收到 `ping` 帧 → 回 `{ headers: { req_id }, errcode: 0, errmsg: "ok" }`
//! - 可主动推 `aibot_msg_callback` / `aibot_event_callback` 帧

use std::sync::Arc;
use std::time::Duration;

use app_lib::connector::im::wecom::aibot_client::{AibotClient, AibotClientConfig, AibotEvent};
use app_lib::connector::im::wecom::aibot_protocol::*;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

/// 起一个 echo-style mock aibot server。回调 `on_subscribe` 决定认证 ack；
/// 通过返回的 `inbound_tx` 主动 push 服务端帧到客户端。
async fn spawn_mock_server(
    on_subscribe: impl Fn(&Value) -> Value + Send + Sync + 'static,
) -> (String, mpsc::Sender<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");

    let (inbound_tx, mut inbound_rx) = mpsc::channel::<Value>(16);
    let on_subscribe = Arc::new(on_subscribe);

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (write, mut read) = ws.split();

        // 从 inbound_rx 推送的帧 → 写到客户端
        let write_handle = Arc::new(Mutex::new(write));
        let w2 = write_handle.clone();
        tokio::spawn(async move {
            while let Some(frame) = inbound_rx.recv().await {
                let _ = w2
                    .lock()
                    .await
                    .send(Message::Text(frame.to_string().into()))
                    .await;
            }
        });

        // 读客户端帧，处理 subscribe / ping / respond
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

            let ack = match cmd {
                "aibot_subscribe" => on_subscribe(&frame),
                "ping" => json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" }),
                "aibot_respond_msg" | "aibot_send_msg" => {
                    json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
                }
                _ => continue,
            };
            let _ = write_handle
                .lock()
                .await
                .send(Message::Text(ack.to_string().into()))
                .await;
        }
    });

    (url, inbound_tx)
}

fn test_config(ws_url: String) -> AibotClientConfig {
    AibotClientConfig {
        bot_id: "BOTID".into(),
        secret: "SECRET".into(),
        ws_url,
        heartbeat_interval: Duration::from_millis(200),
        reply_ack_timeout: Duration::from_secs(2),
        max_missed_pong: 3,
        max_reconnect_attempts: 3,
        max_auth_failure_attempts: 2,
        reconnect_base_delay: Duration::from_millis(50),
    }
}

#[tokio::test]
async fn handshake_subscribes_and_emits_authenticated() {
    let (url, _push) = spawn_mock_server(|frame| {
        let req_id = frame
            .pointer("/headers/req_id")
            .and_then(|v| v.as_str())
            .unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    })
    .await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = Arc::new(AibotClient::new(test_config(url)));
    let cancel_for_task = cancel.clone();
    let c2 = client.clone();
    tokio::spawn(async move {
        let _ = c2.run(evt_tx, cancel_for_task).await;
    });

    let evt = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(evt, AibotEvent::Authenticated),
        "first event must be Authenticated, got {evt:?}"
    );

    cancel.cancel();
}

#[tokio::test]
async fn inbound_message_emits_event() {
    let (url, push) = spawn_mock_server(|frame| {
        let req_id = frame
            .pointer("/headers/req_id")
            .and_then(|v| v.as_str())
            .unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    })
    .await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = Arc::new(AibotClient::new(test_config(url)));
    let cancel_for_task = cancel.clone();
    let c2 = client.clone();
    tokio::spawn(async move {
        let _ = c2.run(evt_tx, cancel_for_task).await;
    });

    // 等认证完成
    let _ = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
        .await
        .unwrap();

    // 推一条消息帧
    push.send(json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "msg-1" },
        "body": {
            "msgid": "MSG1", "aibotid": "BOTID", "chattype": "single",
            "from": { "userid": "U1" }, "msgtype": "text",
            "text": { "content": "hi" }
        }
    }))
    .await
    .unwrap();

    let evt = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
        .await
        .unwrap()
        .unwrap();
    match evt {
        AibotEvent::Inbound(frame) => {
            assert_eq!(frame.cmd, Some(WsCmd::MsgCallback));
            assert_eq!(frame.headers.req_id, "msg-1");
        }
        other => panic!("expected Inbound, got {other:?}"),
    }
    cancel.cancel();
}

#[tokio::test]
async fn disconnected_event_emits_kicked_out_not_reconnect() {
    let (url, push) = spawn_mock_server(|frame| {
        let req_id = frame
            .pointer("/headers/req_id")
            .and_then(|v| v.as_str())
            .unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    })
    .await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = Arc::new(AibotClient::new(test_config(url)));
    let cancel_for_task = cancel.clone();
    let c2 = client.clone();
    let handle = tokio::spawn(async move { c2.run(evt_tx, cancel_for_task).await });

    let _ = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
        .await
        .unwrap(); // Authenticated

    push.send(json!({
        "cmd": "aibot_event_callback",
        "headers": { "req_id": "evt-1" },
        "body": {
            "msgid": "E1", "aibotid": "BOTID",
            "from": { "userid": "U1" }, "msgtype": "event",
            "event": { "eventtype": "disconnected_event" }
        }
    }))
    .await
    .unwrap();

    let evt = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(evt, AibotEvent::KickedOut(_)),
        "must emit KickedOut, got {evt:?}"
    );

    // run() 应在 2 秒内退出（不重连）
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("run() must exit after KickedOut, didn't")
        .unwrap()
        .ok();
}

#[tokio::test]
async fn auth_failure_emits_auth_failed_and_retries_until_exhausted() {
    let (url, _push) = spawn_mock_server(|frame| {
        let req_id = frame
            .pointer("/headers/req_id")
            .and_then(|v| v.as_str())
            .unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 40014, "errmsg": "invalid bot" })
    })
    .await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let mut cfg = test_config(url);
    cfg.max_auth_failure_attempts = 2;
    let client = Arc::new(AibotClient::new(cfg));
    let handle = tokio::spawn(async move { client.run(evt_tx, cancel).await });

    // 至少应见到 AuthFailed
    let mut saw_auth_failed = false;
    while let Ok(Some(evt)) = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await {
        if matches!(evt, AibotEvent::AuthFailed(40014, _)) {
            saw_auth_failed = true;
        }
    }
    assert!(saw_auth_failed);
    // run() 应已退出（attempts 用尽）且返回 Err（exhaustion 路径不能静默 Ok）
    let result = handle.await.expect("run() join");
    assert!(
        result.is_err(),
        "exhausted auth failures must return Err, got {result:?}"
    );
}

#[tokio::test]
async fn cancel_token_terminates_run_within_2s() {
    let (url, _push) = spawn_mock_server(|frame| {
        let req_id = frame
            .pointer("/headers/req_id")
            .and_then(|v| v.as_str())
            .unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    })
    .await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = Arc::new(AibotClient::new(test_config(url)));
    let cancel_clone = cancel.clone();
    let c2 = client.clone();
    let handle = tokio::spawn(async move { c2.run(evt_tx, cancel_clone).await });

    let _ = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
        .await
        .unwrap();
    cancel.cancel();
    let start = tokio::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("run() did not exit within 2s of cancel")
        .unwrap()
        .ok();
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn respond_serializes_under_ack_serial_order() {
    // 记录 server 收到的 aibot_respond_msg 帧 content 的 SEND 顺序，并对每个 ack 延迟 150ms。
    // 这强制第二个 respond 调用在前一帧 in_flight 期间进入 pending 队列；
    // 若实现把帧并行写到 wire，server 看到的顺序可能错乱。
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (write, mut read) = ws.split();
        let write_handle = Arc::new(Mutex::new(write));

        while let Some(Ok(msg)) = read.next().await {
            let text = match msg {
                Message::Text(t) => t,
                _ => continue,
            };
            let frame: Value = serde_json::from_str(&text).unwrap();
            let cmd = frame
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let req_id = frame
                .pointer("/headers/req_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            match cmd.as_str() {
                "aibot_subscribe" | "ping" => {
                    let ack =
                        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" });
                    let _ = write_handle
                        .lock()
                        .await
                        .send(Message::Text(ack.to_string().into()))
                        .await;
                }
                "aibot_respond_msg" => {
                    // 记录到达顺序（按 markdown content）
                    if req_id == "R1" {
                        if let Some(content) = frame
                            .pointer("/body/markdown/content")
                            .and_then(|v| v.as_str())
                        {
                            received_clone.lock().await.push(content.to_string());
                        }
                    }
                    let w = write_handle.clone();
                    // 延迟 ack：保证下一个 respond 必须进 pending 队列等待
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        let ack = json!({
                            "headers": { "req_id": req_id },
                            "errcode": 0,
                            "errmsg": "ok"
                        });
                        let _ = w
                            .lock()
                            .await
                            .send(Message::Text(ack.to_string().into()))
                            .await;
                    });
                }
                _ => {}
            }
        }
    });

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = Arc::new(AibotClient::new(test_config(url)));
    let c2 = client.clone();
    let cancel_for = cancel.clone();
    tokio::spawn(async move {
        let _ = c2.run(evt_tx, cancel_for).await;
    });
    let _ = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
        .await
        .unwrap();

    // 两个 respond 并发提交（不同 content，相同 req_id）。
    // 微小 stagger 保证 submit 顺序确定；mpsc 内部保留入队顺序。
    let c_a = client.clone();
    let c_b = client.clone();
    let h_a = tokio::spawn(async move {
        c_a.respond(
            "R1",
            serde_json::to_value(RespondMarkdownBody::new("a")).unwrap(),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    let h_b = tokio::spawn(async move {
        c_b.respond(
            "R1",
            serde_json::to_value(RespondMarkdownBody::new("b")).unwrap(),
        )
        .await
    });

    h_a.await.unwrap().unwrap();
    h_b.await.unwrap().unwrap();

    let recv = received.lock().await;
    let a_pos = recv.iter().position(|c| c == "a").expect("server saw 'a'");
    let b_pos = recv.iter().position(|c| c == "b").expect("server saw 'b'");
    assert!(
        a_pos < b_pos,
        "server must see frames in submit order: {recv:?}"
    );
    cancel.cancel();
}
