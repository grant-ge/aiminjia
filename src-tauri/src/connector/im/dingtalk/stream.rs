//! 钉钉 Stream 模式 WebSocket 客户端

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::super::types::{
    AttachmentKind, ChannelAttachmentSpec, ChannelConnectionState, ChannelMessage, ConversationType,
};

const STREAM_OPEN_URL: &str = "https://api.dingtalk.com/v1.0/gateway/connections/open";
const ROBOT_CALLBACK_TOPIC: &str = "/v1.0/im/bot/messages/get";
const MAX_RETRY_DELAY_SECS: u64 = 60;

#[derive(Deserialize)]
struct StreamOpenResponse {
    endpoint: String,
    ticket: String,
}

#[derive(Deserialize)]
struct StreamFrame {
    #[serde(rename = "type")]
    frame_type: String,
    headers: StreamHeaders,
    data: Option<String>,
}

#[derive(Deserialize)]
struct StreamHeaders {
    #[serde(rename = "messageId")]
    message_id: Option<String>,
    topic: Option<String>,
}

#[derive(Deserialize)]
struct DingtalkImData {
    #[serde(rename = "msgtype")]
    msg_type: Option<String>,
    text: Option<TextContent>,
    content: Option<DingtalkImContent>,
    #[serde(rename = "senderNick")]
    sender_nick: Option<String>,
    #[serde(rename = "senderUserId")]
    sender_user_id: Option<String>,
    #[serde(rename = "senderId")]
    sender_id: Option<String>,
    #[serde(rename = "senderStaffId")]
    sender_staff_id: Option<String>,
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(rename = "conversationType")]
    conversation_type: Option<String>,
    #[serde(rename = "robotCode")]
    robot_code: Option<String>,
    #[serde(rename = "msgId")]
    msg_id: Option<String>,
    #[serde(rename = "sessionWebhook")]
    session_webhook: Option<String>,
}

#[derive(Deserialize)]
struct TextContent {
    content: String,
}

#[derive(Deserialize, Default)]
struct DingtalkImContent {
    #[serde(rename = "biz_custom_action_url")]
    biz_custom_action_url: Option<String>,
    #[serde(rename = "downloadCode")]
    download_code: Option<String>,
    #[serde(rename = "fileName")]
    file_name: Option<String>,
    recognition: Option<String>,
    #[serde(rename = "richText")]
    rich_text: Option<Vec<RichTextSegment>>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RichTextSegment {
    Picture {
        #[serde(rename = "downloadCode")]
        download_code: String,
    },
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

enum ParseResult {
    Forward(ChannelMessage),
    AutoReply {
        session_webhook: String,
        text: String,
    },
    Drop,
}

#[derive(Clone)]
pub struct DingtalkStreamClient {
    app_key: String,
    app_secret: String,
    robot_code: String,
    message_tx: mpsc::Sender<ChannelMessage>,
}

impl DingtalkStreamClient {
    pub fn new(
        app_key: String,
        app_secret: String,
        robot_code: String,
        message_tx: mpsc::Sender<ChannelMessage>,
    ) -> Self {
        Self {
            app_key,
            app_secret,
            robot_code,
            message_tx,
        }
    }

    /// 启动 Stream 连接后台 task，断线后指数退避重连。
    /// 返回 CancellationToken，调用 cancel() 可停止旧连接（供重新配置时使用）。
    pub fn start(
        &self,
        on_status: impl Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static,
    ) -> CancellationToken {
        let client = self.clone();
        let on_status = Arc::new(on_status);
        let token = CancellationToken::new();
        let token_clone = token.clone();
        tokio::spawn(async move {
            client.run_with_retry(on_status, token_clone).await;
        });
        token
    }

    async fn run_with_retry(
        &self,
        on_status: Arc<impl Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
        cancel: CancellationToken,
    ) {
        let mut delay_secs: u64 = 1;
        loop {
            if cancel.is_cancelled() {
                log::info!("[dingtalk-stream] cancelled, stopping retry loop");
                return;
            }
            on_status(ChannelConnectionState::Connecting, None);

            match self.open_stream_connection().await {
                Ok((endpoint, ticket)) => {
                    if cancel.is_cancelled() {
                        log::info!("[dingtalk-stream] cancelled after stream open");
                        return;
                    }
                    delay_secs = 1;
                    on_status(ChannelConnectionState::Connected, None);
                    log::info!("[dingtalk-stream] connected");

                    if let Err(e) = self.run_ws_loop(&endpoint, &ticket, cancel.clone()).await {
                        log::warn!("[dingtalk-stream] ws loop ended: {:#}", e);
                    }
                }
                Err(e) => {
                    if cancel.is_cancelled() {
                        log::info!("[dingtalk-stream] cancelled after stream open failure");
                        return;
                    }
                    let msg = e.to_string();
                    log::warn!("[dingtalk-stream] open failed: {:#}", e);
                    if msg.contains("401") || msg.contains("Unauthorized") {
                        on_status(
                            ChannelConnectionState::ConfigError,
                            Some("AppKey 或 AppSecret 有误，请检查配置".into()),
                        );
                        return;
                    }
                }
            }

            if cancel.is_cancelled() {
                log::info!("[dingtalk-stream] cancelled before reconnect state");
                return;
            }
            on_status(ChannelConnectionState::Reconnecting, None);
            tokio::select! {
                _ = sleep(Duration::from_secs(delay_secs)) => {}
                _ = cancel.cancelled() => {
                    log::info!("[dingtalk-stream] cancelled during reconnect wait");
                    return;
                }
            }
            delay_secs = (delay_secs * 2).min(MAX_RETRY_DELAY_SECS);
        }
    }

    async fn open_stream_connection(&self) -> Result<(String, String)> {
        let client = Client::new();
        let resp = client
            .post(STREAM_OPEN_URL)
            .header("Accept", "application/json")
            .json(&Self::stream_open_body(&self.app_key, &self.app_secret))
            .send()
            .await
            .context("Failed to POST stream open")?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("401 Unauthorized: invalid AppKey or AppSecret");
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Stream open failed: {} {}", status, body);
        }

        let data: StreamOpenResponse = resp
            .json()
            .await
            .context("Failed to parse stream open response")?;
        log::debug!("[dingtalk-stream] open response parsed");
        Ok((data.endpoint, data.ticket))
    }

    fn stream_open_body(app_key: &str, app_secret: &str) -> serde_json::Value {
        serde_json::json!({
            "clientId": app_key,
            "clientSecret": app_secret,
            "subscriptions": [
                { "type": "EVENT", "topic": "*" },
                { "type": "CALLBACK", "topic": ROBOT_CALLBACK_TOPIC }
            ],
            "ua": "aijia/1.0"
        })
    }

    async fn run_ws_loop(
        &self,
        endpoint: &str,
        ticket: &str,
        cancel: CancellationToken,
    ) -> Result<()> {
        let url = format!("{}?ticket={}", endpoint, ticket);
        log::info!("[dingtalk-stream] ws connecting");
        let (ws_stream, response) = tokio_tungstenite::connect_async(&url)
            .await
            .context("WebSocket connect failed")?;
        log::info!(
            "[dingtalk-stream] ws handshake ok, status={}",
            response.status()
        );

        let (mut write, mut read) = ws_stream.split();
        log::debug!("[dingtalk-stream] entering ws read loop");

        // 每 8 秒发一次 WebSocket Ping 保活
        let mut heartbeat = tokio::time::interval(Duration::from_secs(8));
        heartbeat.tick().await; // 跳过第一次立即触发

        loop {
            tokio::select! {
                msg = read.next() => {
                    let msg = match msg {
                        Some(m) => m.context("WebSocket read error")?,
                        None => anyhow::bail!("WebSocket stream ended"),
                    };
                    match msg {
                        Message::Text(text) => {
                            if let Ok(frame) = serde_json::from_str::<StreamFrame>(&text) {
                                log::debug!("[dingtalk-stream] frame type={} topic={:?}", frame.frame_type, frame.headers.topic);
                                if frame.frame_type == "SYSTEM" {
                                    match frame.headers.topic.as_deref() {
                                        Some("ping") => {
                                            let pong = serde_json::json!({
                                                "code": 200,
                                                "headers": {
                                                    "contentType": "application/json",
                                                    "messageId": frame.headers.message_id
                                                },
                                                "message": "OK",
                                                "data": frame.data.as_deref().unwrap_or("")
                                            });
                                            write.send(Message::Text(pong.to_string().into())).await.ok();
                                        }
                                        Some("CONNECTED") => {
                                            log::info!("[dingtalk-stream] CONNECTED ack received");
                                        }
                                        Some("REGISTERED") => {
                                            log::info!("[dingtalk-stream] REGISTERED — subscriptions active");
                                        }
                                        Some("disconnect") => {
                                            anyhow::bail!("Server sent disconnect");
                                        }
                                        other => {
                                            log::debug!("[dingtalk-stream] SYSTEM topic={:?}", other);
                                        }
                                    }
                                } else if frame.frame_type == "EVENT" || frame.frame_type == "CALLBACK" {
                                    let ack = serde_json::json!({
                                        "code": 200,
                                        "headers": {
                                            "contentType": "application/json",
                                            "messageId": frame.headers.message_id
                                        },
                                        "message": "OK",
                                        "data": ""
                                    });
                                    write.send(Message::Text(ack.to_string().into())).await.ok();

                                    if let Some(data_str) = &frame.data {
                                        log::debug!(
                                            "[dingtalk-stream] EVENT/CALLBACK topic={:?} payload={}",
                                            frame.headers.topic,
                                            data_str
                                        );
                                        match self.parse_im_message(data_str) {
                                            ParseResult::Forward(channel_msg) => {
                                                let msg_id = channel_msg.msg_id.clone();
                                                let send_result = self.message_tx.send(channel_msg).await;
                                                log::debug!(
                                                    "[dingtalk-stream] forwarded msg_id={} send_ok={}",
                                                    msg_id,
                                                    send_result.is_ok()
                                                );
                                            }
                                            ParseResult::AutoReply { session_webhook, text } => {
                                                tokio::spawn(send_session_webhook_text(
                                                    session_webhook,
                                                    text,
                                                ));
                                            }
                                            ParseResult::Drop => {}
                                        }
                                    }
                                }
                            }
                        }
                        Message::Close(frame) => {
                            log::warn!("[dingtalk-stream] Close frame received: {:?}", frame);
                            anyhow::bail!("WebSocket closed by server");
                        }
                        Message::Ping(data) => {
                            write.send(Message::Pong(data)).await.ok();
                        }
                        other => {
                            log::debug!("[dingtalk-stream] non-text frame: {:?}", other);
                        }
                    }
                }
                _ = heartbeat.tick() => {
                    if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                        anyhow::bail!("WebSocket heartbeat failed: {}", e);
                    }
                }
                _ = cancel.cancelled() => {
                    log::info!("[dingtalk-stream] cancelled during ws loop");
                    return Ok(());
                }
            }
        }
    }

    fn build_channel_message(
        &self,
        im: DingtalkImData,
        text: String,
        attachments: Vec<ChannelAttachmentSpec>,
    ) -> Option<ChannelMessage> {
        if text.trim().is_empty() && attachments.is_empty() {
            return None;
        }
        let sender_id = im.sender_user_id.or(im.sender_staff_id).or(im.sender_id)?;
        let sender_nick = im.sender_nick.unwrap_or_else(|| sender_id.clone());
        let msg_id = im.msg_id.unwrap_or_default();
        let robot_code = im.robot_code.unwrap_or_else(|| self.robot_code.clone());
        let (conversation_type, conversation_key, reply_group_id) =
            if im.conversation_type.as_deref() == Some("2") {
                let conv_id = im.conversation_id?;
                (ConversationType::Group, conv_id.clone(), conv_id)
            } else {
                (
                    ConversationType::Private,
                    sender_id.clone(),
                    sender_id.clone(),
                )
            };

        Some(ChannelMessage {
            msg_id,
            native_message_id: None,
            conversation_type,
            conversation_key,
            sender_id,
            sender_nick,
            text,
            robot_code,
            reply_group_id,
            attachments,
            session_webhook: im.session_webhook,
            created_at_ms: None,
        })
    }

    fn parse_im_message(&self, data_str: &str) -> ParseResult {
        let im: DingtalkImData = match serde_json::from_str(data_str) {
            Ok(v) => v,
            Err(_) => return ParseResult::Drop,
        };

        let msg_type = im.msg_type.clone().unwrap_or_default();
        match msg_type.as_str() {
            "text" => {
                let text = match im.text.as_ref() {
                    Some(t) => t.content.clone(),
                    None => return ParseResult::Drop,
                };
                self.build_channel_message(im, text, Vec::new())
                    .map(ParseResult::Forward)
                    .unwrap_or(ParseResult::Drop)
            }
            "picture" => {
                let msg_id = im.msg_id.clone().unwrap_or_else(|| "unknown".to_string());
                let download_code = match im.content.as_ref().and_then(|c| c.download_code.clone())
                {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => return ParseResult::Drop,
                };
                let attachments = vec![ChannelAttachmentSpec {
                    kind: AttachmentKind::Picture,
                    download_code,
                    file_name: format!("image_{}_0.jpg", msg_id),
                }];
                self.build_channel_message(im, String::new(), attachments)
                    .map(ParseResult::Forward)
                    .unwrap_or(ParseResult::Drop)
            }
            "file" => {
                let content = match im.content.as_ref() {
                    Some(v) => v,
                    None => return ParseResult::Drop,
                };
                let download_code = match content.download_code.clone() {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => return ParseResult::Drop,
                };
                let file_name = content
                    .file_name
                    .clone()
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or_else(|| {
                        let msg_id = im.msg_id.clone().unwrap_or_else(|| "unknown".to_string());
                        format!("file_{}.bin", msg_id)
                    });
                let attachments = vec![ChannelAttachmentSpec {
                    kind: AttachmentKind::File,
                    download_code,
                    file_name,
                }];
                self.build_channel_message(im, String::new(), attachments)
                    .map(ParseResult::Forward)
                    .unwrap_or(ParseResult::Drop)
            }
            "audio" => {
                let text = match im
                    .content
                    .as_ref()
                    .and_then(|c| c.recognition.clone())
                    .map(|v| v.trim().to_string())
                {
                    Some(v) if !v.is_empty() => v,
                    _ => return ParseResult::Drop,
                };
                self.build_channel_message(im, text, Vec::new())
                    .map(ParseResult::Forward)
                    .unwrap_or(ParseResult::Drop)
            }
            "richText" => {
                let msg_id = im.msg_id.clone().unwrap_or_else(|| "unknown".to_string());
                let segments = match im.content.as_ref().and_then(|c| c.rich_text.as_ref()) {
                    Some(v) => v,
                    None => return ParseResult::Drop,
                };
                let mut text_parts = Vec::new();
                let mut attachments = Vec::new();
                for (idx, segment) in segments.iter().enumerate() {
                    match segment {
                        RichTextSegment::Picture { download_code }
                            if !download_code.trim().is_empty() =>
                        {
                            attachments.push(ChannelAttachmentSpec {
                                kind: AttachmentKind::Picture,
                                download_code: download_code.clone(),
                                file_name: format!("image_{}_{}.jpg", msg_id, idx),
                            });
                        }
                        RichTextSegment::Text { text } => {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                text_parts.push(trimmed.to_string());
                            }
                        }
                        _ => {}
                    }
                }
                self.build_channel_message(im, text_parts.join(" "), attachments)
                    .map(ParseResult::Forward)
                    .unwrap_or(ParseResult::Drop)
            }
            "interactiveCard" => {
                let url = im
                    .content
                    .as_ref()
                    .and_then(|c| c.biz_custom_action_url.as_deref())
                    .unwrap_or("");
                if is_dingtalk_doc_or_drive_url(url) {
                    if let Some(webhook) = im.session_webhook {
                        return ParseResult::AutoReply {
                            session_webhook: webhook,
                            text: "暂不支持直接读取钉钉文档/钉盘文件，请打开文档后导出为 PDF/Word/Markdown，再把导出的文件发给我。".to_string(),
                        };
                    }
                }
                ParseResult::Drop
            }
            other => {
                log::warn!(
                    "[dingtalk-stream] drop unknown msgtype={} msgId={:?}",
                    other,
                    im.msg_id
                );
                ParseResult::Drop
            }
        }
    }
}

fn is_dingtalk_doc_or_drive_url(url: &str) -> bool {
    // dingtalk://...?route=previewDentry&spaceId=...&fileId=... 涵盖钉盘 / 钉钉文档分享卡
    url.contains("yunpan") || url.contains("/doc") || url.contains("previewDentry")
}

pub async fn send_session_webhook_text(session_webhook: String, text: String) {
    let body = serde_json::json!({
        "msgtype": "text",
        "text": { "content": text }
    });
    match Client::new()
        .post(&session_webhook)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            log::info!(
                "[dingtalk-stream] sessionWebhook auto-reply status={}",
                resp.status()
            );
        }
        Err(e) => {
            log::warn!(
                "[dingtalk-stream] sessionWebhook auto-reply failed: {:#}",
                e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::types::AttachmentKind;
    use super::*;
    use tokio::sync::mpsc;

    impl ParseResult {
        fn unwrap_forward(self) -> ChannelMessage {
            match self {
                ParseResult::Forward(m) => m,
                ParseResult::AutoReply { .. } => panic!("expected Forward, got AutoReply"),
                ParseResult::Drop => panic!("expected Forward, got Drop"),
            }
        }

        fn is_drop(&self) -> bool {
            matches!(self, ParseResult::Drop)
        }
    }

    fn make_client() -> (DingtalkStreamClient, mpsc::Receiver<ChannelMessage>) {
        let (tx, rx) = mpsc::channel(8);
        let client = DingtalkStreamClient::new(
            "test-key".into(),
            "test-secret".into(),
            "test-robot".into(),
            tx,
        );
        (client, rx)
    }

    #[test]
    fn stream_open_subscribes_to_official_robot_callback_topic() {
        let body = DingtalkStreamClient::stream_open_body("app-key", "app-secret");
        assert_eq!(body["subscriptions"][0]["type"], "EVENT");
        assert_eq!(body["subscriptions"][0]["topic"], "*");
        assert_eq!(body["subscriptions"][1]["type"], "CALLBACK");
        assert_eq!(
            body["subscriptions"][1]["topic"],
            "/v1.0/im/bot/messages/get"
        );
    }

    #[test]
    fn parse_text_group_message() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "text",
            "text": { "content": "hello world" },
            "senderNick": "张三",
            "senderUserId": "user001",
            "conversationId": "cid123",
            "conversationType": "2",
            "robotCode": "robot001",
            "msgId": "msg001"
        }"#;

        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.text, "hello world");
        assert_eq!(msg.sender_nick, "张三");
        assert_eq!(msg.conversation_type, ConversationType::Group);
        assert_eq!(msg.conversation_key, "cid123");
        assert_eq!(msg.msg_id, "msg001");
    }

    #[test]
    fn parse_text_private_message() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "text",
            "text": { "content": "private msg" },
            "senderNick": "李四",
            "senderUserId": "user002",
            "conversationId": "cid-private",
            "conversationType": "1",
            "robotCode": "robot001",
            "msgId": "msg002"
        }"#;

        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.conversation_type, ConversationType::Private);
        assert_eq!(msg.conversation_key, "user002");
    }

    #[test]
    fn ignores_non_text_messages() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "image",
            "senderUserId": "user001",
            "conversationType": "1"
        }"#;
        assert!(client.parse_im_message(data).is_drop());
    }

    #[test]
    fn dingtalk_doc_card_triggers_auto_reply() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "interactiveCard",
            "content": {
                "biz_custom_action_url": "dingtalk://dingtalkclient/page/yunpan?route=previewDentry&spaceId=21256706750&fileId=218662958695&type=file"
            },
            "senderUserId": "user001",
            "conversationType": "1",
            "sessionWebhook": "https://oapi.dingtalk.com/robot/sendBySession?session=abc"
        }"#;
        match client.parse_im_message(data) {
            ParseResult::AutoReply {
                session_webhook,
                text,
            } => {
                assert!(session_webhook.contains("sendBySession"));
                assert!(text.contains("钉钉文档"));
            }
            other => panic!(
                "expected AutoReply, got {:?}",
                match other {
                    ParseResult::Forward(_) => "Forward",
                    ParseResult::Drop => "Drop",
                    _ => "?",
                }
            ),
        }
    }

    #[test]
    fn parse_picture_single() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "picture",
            "content": { "downloadCode": "pic-code-1" },
            "senderNick": "张三",
            "senderUserId": "user001",
            "conversationType": "1",
            "robotCode": "robot001",
            "msgId": "msg-pic-1",
            "sessionWebhook": "https://oapi.dingtalk.com/robot/sendBySession?session=abc"
        }"#;

        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.text, "");
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].kind, AttachmentKind::Picture);
        assert_eq!(msg.attachments[0].download_code, "pic-code-1");
        assert_eq!(msg.attachments[0].file_name, "image_msg-pic-1_0.jpg");
        assert_eq!(
            msg.session_webhook.as_deref(),
            Some("https://oapi.dingtalk.com/robot/sendBySession?session=abc")
        );
    }

    #[test]
    fn uses_robot_code_from_client_when_missing_in_message() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "text",
            "text": { "content": "hi" },
            "senderUserId": "user001",
            "conversationType": "1"
        }"#;
        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.robot_code, "test-robot");
    }

    #[test]
    fn parsed_message_does_not_carry_client_credentials() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "text",
            "text": { "content": "hi" },
            "senderUserId": "user001",
            "conversationType": "1"
        }"#;

        let msg = format!("{:?}", client.parse_im_message(data).unwrap_forward());
        assert!(!msg.contains("test-key"));
        assert!(!msg.contains("test-secret"));
    }

    #[test]
    fn parse_file_single() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "file",
            "content": { "downloadCode": "file-code-1", "fileName": "report.xlsx" },
            "senderUserId": "user001",
            "conversationType": "1",
            "msgId": "msg-file-1"
        }"#;
        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.text, "");
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].kind, AttachmentKind::File);
        assert_eq!(msg.attachments[0].download_code, "file-code-1");
        assert_eq!(msg.attachments[0].file_name, "report.xlsx");
    }

    #[test]
    fn parse_audio_with_recognition() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "audio",
            "content": { "recognition": "帮我总结一下" },
            "senderUserId": "user001",
            "conversationType": "1",
            "msgId": "msg-audio-1"
        }"#;
        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.text, "帮我总结一下");
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn parse_audio_empty_recognition_drops() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "audio",
            "content": { "recognition": "   " },
            "senderUserId": "user001",
            "conversationType": "1",
            "msgId": "msg-audio-empty"
        }"#;
        assert!(client.parse_im_message(data).is_drop());
    }

    #[test]
    fn parse_richtext_pictures_and_text() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "richText",
            "content": { "richText": [
                { "type": "picture", "downloadCode": "pic-1" },
                { "type": "text", "text": "\n" },
                { "type": "picture", "downloadCode": "pic-2" },
                { "type": "text", "text": " 你好 " }
            ]},
            "senderUserId": "user001",
            "conversationType": "1",
            "msgId": "msg-rich-1"
        }"#;
        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.text, "你好");
        assert_eq!(msg.attachments.len(), 2);
        assert_eq!(msg.attachments[0].file_name, "image_msg-rich-1_0.jpg");
        assert_eq!(msg.attachments[1].file_name, "image_msg-rich-1_2.jpg");
    }

    #[test]
    fn parse_richtext_unknown_segment_type() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "richText",
            "content": { "richText": [
                { "type": "video", "downloadCode": "skip-me" },
                { "type": "text", "text": "保留文字" },
                { "type": "picture", "downloadCode": "pic-ok" }
            ]},
            "senderUserId": "user001",
            "conversationType": "1",
            "msgId": "msg-rich-2"
        }"#;
        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.text, "保留文字");
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].download_code, "pic-ok");
    }

    #[test]
    fn parse_richtext_picture_only() {
        let (client, _rx) = make_client();
        let data = r#"{
            "msgtype": "richText",
            "content": { "richText": [
                { "type": "picture", "downloadCode": "pic-1" },
                { "type": "picture", "downloadCode": "pic-2" }
            ]},
            "senderUserId": "user001",
            "conversationType": "1",
            "msgId": "msg-rich-pic-only"
        }"#;
        let msg = client.parse_im_message(data).unwrap_forward();
        assert_eq!(msg.text, "");
        assert_eq!(msg.attachments.len(), 2);
    }
}
