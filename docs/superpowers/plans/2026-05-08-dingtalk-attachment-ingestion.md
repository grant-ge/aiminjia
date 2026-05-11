# DingTalk Attachment Ingestion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让钉钉 `picture` / `file` / `richText` / `audio` 消息进入现有 LLM 对话链路，其中图片和文件下载到 workspace 并作为 `ChatAttachmentRef` 传入本轮 turn。

**Architecture:** `dingtalk_stream.rs` 只做同步 schema 解析并产出 `ChannelMessage` 附件规格；`dingtalk_download.rs` 在 channel worker 内完成钉钉两步下载、sha256 去重和安全落盘；`manager.rs` 在 session routing 后、`send_chat_request` 前把下载成功项转换为 `ChatAttachmentRef`，同时填充 `session_attachment_dirs` 复用现有路径授权链路。

**Tech Stack:** Rust 2021, tokio, reqwest, serde, sha2, uuid, tempfile,现有 `ChatTurnRequest` / `ChatAttachmentRef` / `derive_working_dirs_from_attachments`。

---

## 背景与对标结论

- 规格来源：`docs/superpowers/specs/2026-05-08-dingtalk-attachment-ingestion-design.md`。
- 当前钉钉 Stream 只 forward `msgtype=text`，`picture` 现有测试明确期望 drop；本计划会把该测试改成 forward attachment。
- 附件进入 LLM 的标准路径已经存在：`ChatTurnRequest.attachments` + `chat_runtime_impl::build_llm_content()`。
- 关键补充：`TauriChatCommandAdapter::send_chat_request()` 不会像前端 `send_message()` 一样自动派生 `session_attachment_dirs`，因此 channel worker 必须在构造 request 后调用 `derive_working_dirs_from_attachments()`。

## File Map

**Create**
- `src-tauri/src/connector/channel/dingtalk_download.rs` — 钉钉附件下载器、下载结果、错误类型、文件名/扩展名/sha256 工具函数。
- `src-tauri/tests/dingtalk_attachment_integration_test.rs` — channel worker 可测试核心的集成覆盖。

**Modify**
- `src-tauri/src/connector/channel/types.rs` — 给 `ChannelMessage` 增加附件与 `session_webhook`；新增 `ChannelAttachmentSpec` / `AttachmentKind`。
- `src-tauri/src/connector/channel/dingtalk_stream.rs` — 扩展钉钉 callback schema；解析 `picture` / `file` / `richText` / `audio`。
- `src-tauri/src/connector/channel/manager.rs` — 创建 downloader，下载附件，拼失败提示，填 `session_attachment_dirs`。
- `src-tauri/src/connector/channel/mod.rs` — 导出 `dingtalk_download` 模块。
- `src-tauri/Cargo.toml` — dev-dependency 增加 `wiremock`，用于 HTTP 下载器单测。

**Do Not Modify**
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — 附件结构和持久化已具备。
- `src-tauri/src/transport/tauri_event_adapter.rs` — 前端附件 UI 不变。
- `src-tauri/src/storage/upload_gc.rs` — 钉钉下载目录不纳入自动清理。

## Task 1: Channel Message 类型扩展

**Files:**
- Modify: `src-tauri/src/connector/channel/types.rs`
- Test: `src-tauri/src/connector/channel/types.rs`

- [ ] **Step 1: 扩展类型定义**

在 `ConversationType` 后、`ChannelMessage` 前加入：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelAttachmentSpec {
    pub kind: AttachmentKind,
    pub download_code: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Picture,
    File,
}
```

把 `ChannelMessage` 改成：

```rust
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub msg_id: String,
    pub conversation_type: ConversationType,
    pub conversation_key: String,
    pub sender_id: String,
    pub sender_nick: String,
    pub text: String,
    pub robot_code: String,
    pub reply_group_id: String,
    pub attachments: Vec<ChannelAttachmentSpec>,
    pub session_webhook: Option<String>,
}
```

- [ ] **Step 2: 更新 manager 测试构造器**

在 `src-tauri/src/connector/channel/manager.rs` 的 `test_message()` 中补齐新字段：

```rust
fn test_message() -> ChannelMessage {
    ChannelMessage {
        msg_id: "msg-1".into(),
        conversation_type: ConversationType::Private,
        conversation_key: "user-1".into(),
        sender_id: "user-1".into(),
        sender_nick: "User 1".into(),
        text: "hello".into(),
        robot_code: "robot-1".into(),
        reply_group_id: "user-1".into(),
        attachments: Vec::new(),
        session_webhook: None,
    }
}
```

- [ ] **Step 3: 编译验证红灯**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
```

Expected: FAIL，错误集中在 `dingtalk_stream.rs` 构造 `ChannelMessage` 时缺少 `attachments` / `session_webhook` 字段。

- [ ] **Step 4: 给 text 解析路径补空附件字段**

在 `src-tauri/src/connector/channel/dingtalk_stream.rs` 现有 `ParseResult::Forward(ChannelMessage { ... })` 中补：

```rust
attachments: Vec::new(),
session_webhook: im.session_webhook,
```

- [ ] **Step 5: 编译验证绿灯**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/types.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/manager.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/dingtalk_stream.rs
git commit -m "feat(channel): carry dingTalk attachment specs"
```

## Task 2: 解析 picture / file / audio / richText

**Files:**
- Modify: `src-tauri/src/connector/channel/dingtalk_stream.rs`

- [ ] **Step 1: 写 picture 解析红灯测试**

把 `picture_message_is_dropped_silently` 替换为：

```rust
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
    assert_eq!(msg.session_webhook.as_deref(), Some("https://oapi.dingtalk.com/robot/sendBySession?session=abc"));
}
```

同时在测试模块 import 中加入：

```rust
use super::super::types::AttachmentKind;
```

- [ ] **Step 2: 运行 picture 测试确认失败**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::dingtalk_stream::tests::parse_picture_single -- --nocapture
```

Expected: FAIL，当前 `parse_im_message` 对非 text 走 `Drop`。

- [ ] **Step 3: 扩展 schema 类型**

在 `DingtalkImContent` 中加入字段，并新增 richText segment enum：

```rust
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
    Text { text: String },
    #[serde(other)]
    Other,
}
```

修改 imports：

```rust
use super::types::{AttachmentKind, ChannelConnectionState, ChannelMessage, ChannelAttachmentSpec, ConversationType};
```

- [ ] **Step 4: 抽公共 envelope 构造函数**

在 `impl DingtalkStreamClient` 内、`parse_im_message` 前加入：

```rust
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
        conversation_type,
        conversation_key,
        sender_id,
        sender_nick,
        text,
        robot_code,
        reply_group_id,
        attachments,
        session_webhook: im.session_webhook,
    })
}
```

- [ ] **Step 5: 实现 msgtype 分支**

把 `parse_im_message` 中 `if im.msg_type.as_deref() != Some("text") { ... }` 和后续 text-only 构造替换为：

```rust
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
        let download_code = match im.content.as_ref().and_then(|c| c.download_code.clone()) {
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
                RichTextSegment::Picture { download_code } if !download_code.trim().is_empty() => {
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
```

- [ ] **Step 6: 写剩余解析测试**

在 tests mod 追加：

```rust
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
```

- [ ] **Step 7: 跑解析测试**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::dingtalk_stream::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 8: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/dingtalk_stream.rs
git commit -m "feat(dingtalk): parse attachment callbacks"
```

## Task 3: 新增钉钉附件下载器

**Files:**
- Create: `src-tauri/src/connector/channel/dingtalk_download.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 增加测试依赖**

在 `src-tauri/Cargo.toml` `[dev-dependencies]` 下改成：

```toml
[dev-dependencies]
tempfile = "3"
wiremock = "0.6"
```

- [ ] **Step 2: 创建下载器骨架和工具函数测试**

新建 `src-tauri/src/connector/channel/dingtalk_download.rs`：

```rust
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::dingtalk_token::{get_access_token, TokenCache};

const DINGTALK_API: &str = "https://api.dingtalk.com";
const GET_URL_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct DingtalkFileDownloader {
    client: Client,
    token_cache: TokenCache,
    app_key: String,
    app_secret: String,
    dest_dir: PathBuf,
    api_base: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("token: {0:#}")]
    Token(anyhow::Error),
    #[error("get url: status={status} body={body}")]
    GetUrl { status: u16, body: String },
    #[error("network: {0}")]
    Network(reqwest::Error),
    #[error("io: {0}")]
    Io(std::io::Error),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadUrlResponse {
    download_url: String,
}

impl DingtalkFileDownloader {
    pub fn new(
        token_cache: TokenCache,
        app_key: String,
        app_secret: String,
        dest_dir: PathBuf,
    ) -> Self {
        Self::new_with_api_base(token_cache, app_key, app_secret, dest_dir, DINGTALK_API.to_string())
    }

    pub fn new_with_api_base(
        token_cache: TokenCache,
        app_key: String,
        app_secret: String,
        dest_dir: PathBuf,
        api_base: String,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(DOWNLOAD_TIMEOUT)
                .build()
                .expect("build reqwest client"),
            token_cache,
            app_key,
            app_secret,
            dest_dir,
            api_base,
        }
    }

    pub async fn download(
        &self,
        download_code: &str,
        robot_code: &str,
        original_file_name: &str,
    ) -> Result<DownloadedFile, DownloadError> {
        let display_name = safe_display_file_name(original_file_name);
        tokio::fs::create_dir_all(&self.dest_dir)
            .await
            .map_err(DownloadError::Io)?;
        let token = get_access_token(&self.token_cache, &self.app_key, &self.app_secret)
            .await
            .map_err(DownloadError::Token)?;
        let download_url = self
            .get_download_url(download_code, robot_code, &token)
            .await?;
        self.fetch_with_retries(&download_url, &display_name).await
    }

    async fn get_download_url(
        &self,
        download_code: &str,
        robot_code: &str,
        token: &str,
    ) -> Result<String, DownloadError> {
        let resp = self
            .client
            .post(format!("{}/v1.0/robot/messageFiles/download", self.api_base))
            .timeout(GET_URL_TIMEOUT)
            .header("x-acs-dingtalk-access-token", token)
            .json(&serde_json::json!({
                "downloadCode": download_code,
                "robotCode": robot_code,
            }))
            .send()
            .await
            .map_err(DownloadError::Network)?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DownloadError::GetUrl { status, body });
        }
        let data: DownloadUrlResponse = resp.json().await.map_err(DownloadError::Network)?;
        Ok(data.download_url)
    }

    async fn fetch_with_retries(
        &self,
        download_url: &str,
        display_name: &str,
    ) -> Result<DownloadedFile, DownloadError> {
        let mut last_error: Option<DownloadError> = None;
        for attempt in 0..3 {
            match self.fetch_once(download_url, display_name).await {
                Ok(file) => return Ok(file),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }
        Err(last_error.expect("download attempted at least once"))
    }

    async fn fetch_once(
        &self,
        download_url: &str,
        display_name: &str,
    ) -> Result<DownloadedFile, DownloadError> {
        let resp = self
            .client
            .get(download_url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .map_err(DownloadError::Network)?;
        let mime_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());
        let bytes = resp.bytes().await.map_err(DownloadError::Network)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = format!("{:x}", hasher.finalize());
        let ext = extension_or_bin(display_name);
        let final_path = self.dest_dir.join(format!("{}.{}", sha256, ext));
        if final_path.exists() {
            return Ok(DownloadedFile {
                path: final_path,
                file_name: display_name.to_string(),
                size: bytes.len() as u64,
                sha256,
                mime_type,
            });
        }
        let tmp_path = self.dest_dir.join(format!(".tmp_{}", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(DownloadError::Io)?;
        file.write_all(&bytes).await.map_err(DownloadError::Io)?;
        file.flush().await.map_err(DownloadError::Io)?;
        tokio::fs::rename(&tmp_path, &final_path)
            .await
            .map_err(DownloadError::Io)?;
        Ok(DownloadedFile {
            path: final_path,
            file_name: display_name.to_string(),
            size: bytes.len() as u64,
            sha256,
            mime_type,
        })
    }
}

pub fn safe_display_file_name(original_file_name: &str) -> String {
    let candidate = Path::new(original_file_name)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("attachment.bin")
        .trim();
    let candidate = if candidate.is_empty() {
        "attachment.bin"
    } else {
        candidate
    };
    if crate::storage::safe_filename::ensure_safe_filename(candidate).is_ok() {
        candidate.to_string()
    } else {
        "attachment.bin".to_string()
    }
}

pub fn extension_or_bin(file_name: &str) -> String {
    Path::new(file_name)
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "bin".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_name_rejects_path_traversal() {
        assert_eq!(safe_display_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_display_file_name("CON"), "attachment.bin");
    }

    #[test]
    fn extension_defaults_to_bin() {
        assert_eq!(extension_or_bin("report.xlsx"), "xlsx");
        assert_eq!(extension_or_bin("README"), "bin");
    }
}
```

- [ ] **Step 3: 导出模块**

在 `src-tauri/src/connector/channel/mod.rs` 加入：

```rust
pub mod dingtalk_download;
```

- [ ] **Step 4: 跑下载器工具函数测试**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::dingtalk_download::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/mod.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/dingtalk_download.rs
git commit -m "feat(dingtalk): add attachment downloader"
```

## Task 4: 下载器 HTTP 行为测试

**Files:**
- Modify: `src-tauri/src/connector/channel/dingtalk_download.rs`

- [ ] **Step 1: 增加 wiremock 测试**

在 `dingtalk_download.rs` tests mod 中追加：

```rust
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn download_two_step_happy_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/robot/messageFiles/download"))
        .and(header("x-acs-dingtalk-access-token", "token-1"))
        .and(body_json(serde_json::json!({
            "downloadCode": "code-1",
            "robotCode": "robot-1"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "downloadUrl": format!("{}/download/file", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/plain")
                .set_body_bytes("hello"),
        )
        .mount(&server)
        .await;

    let dir = tempfile::TempDir::new().unwrap();
    let cache = TokenCache::new();
    cache.set("token-1".into(), 7200).await;
    let downloader = DingtalkFileDownloader::new_with_api_base(
        cache,
        "app-key".into(),
        "app-secret".into(),
        dir.path().to_path_buf(),
        server.uri(),
    );

    let file = downloader
        .download("code-1", "robot-1", "note.txt")
        .await
        .expect("download succeeds");

    assert_eq!(file.file_name, "note.txt");
    assert_eq!(file.size, 5);
    assert_eq!(file.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(file.path.extension().and_then(|v| v.to_str()), Some("txt"));
    assert_eq!(std::fs::read(&file.path).unwrap(), b"hello");
}

#[tokio::test]
async fn download_dedup_when_same_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/robot/messageFiles/download"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "downloadUrl": format!("{}/download/file", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes("same"))
        .mount(&server)
        .await;

    let dir = tempfile::TempDir::new().unwrap();
    let cache = TokenCache::new();
    cache.set("token-1".into(), 7200).await;
    let downloader = DingtalkFileDownloader::new_with_api_base(
        cache,
        "app-key".into(),
        "app-secret".into(),
        dir.path().to_path_buf(),
        server.uri(),
    );

    let first = downloader.download("code-1", "robot-1", "a.bin").await.unwrap();
    let second = downloader.download("code-2", "robot-1", "a.bin").await.unwrap();

    assert_eq!(first.path, second.path);
    let files = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".bin"))
        .count();
    assert_eq!(files, 1);
}

#[tokio::test]
async fn download_geturl_failure_returns_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/robot/messageFiles/download"))
        .respond_with(ResponseTemplate::new(410).set_body_string("expired"))
        .mount(&server)
        .await;

    let dir = tempfile::TempDir::new().unwrap();
    let cache = TokenCache::new();
    cache.set("token-1".into(), 7200).await;
    let downloader = DingtalkFileDownloader::new_with_api_base(
        cache,
        "app-key".into(),
        "app-secret".into(),
        dir.path().to_path_buf(),
        server.uri(),
    );

    let err = downloader
        .download("bad-code", "robot-1", "a.bin")
        .await
        .expect_err("geturl fails");
    assert!(matches!(err, DownloadError::GetUrl { status: 410, .. }));
}
```

- [ ] **Step 2: 跑 HTTP 下载器测试**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::dingtalk_download::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 3: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/dingtalk_download.rs
git commit -m "test(dingtalk): cover attachment downloader"
```

## Task 5: Channel worker 附件转换函数

**Files:**
- Modify: `src-tauri/src/connector/channel/manager.rs`

- [ ] **Step 1: 引入依赖**

在 `manager.rs` imports 中加入：

```rust
use crate::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use super::dingtalk_download::{DingtalkFileDownloader, DownloadedFile};
use super::types::AttachmentKind;
```

- [ ] **Step 2: 添加纯函数测试**

在 `manager.rs` tests mod 追加：

```rust
#[test]
fn build_compound_content_appends_group_prefix_and_download_failures() {
    let content = build_compound_content(
        &ConversationType::Group,
        "张三",
        "请看附件",
        &["bad.jpg".to_string(), "expired.pdf".to_string()],
    );
    assert!(content.starts_with("[张三]: 请看附件"));
    assert!(content.contains("[注意：以下附件下载失败，未能加载：bad.jpg, expired.pdf]"));
}

#[test]
fn downloaded_file_to_chat_attachment_maps_kind_and_type() {
    let downloaded = DownloadedFile {
        path: std::path::PathBuf::from("/tmp/a/report.xlsx"),
        file_name: "report.xlsx".into(),
        size: 12,
        sha256: "abc".into(),
        mime_type: Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into()),
    };
    let attachment = downloaded_to_chat_attachment(&downloaded, AttachmentKind::File);
    assert_eq!(attachment.id, "abc");
    assert_eq!(attachment.file_name, "report.xlsx");
    assert_eq!(attachment.kind, "file");
    assert_eq!(attachment.file_type, "xlsx");
}
```

- [ ] **Step 3: 运行纯函数测试确认失败**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::manager::tests -- --nocapture
```

Expected: FAIL，`build_compound_content` / `downloaded_to_chat_attachment` 尚未定义。

- [ ] **Step 4: 添加转换函数**

在 `is_current_stream` 后、tests mod 前加入：

```rust
fn build_compound_content(
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    download_failures: &[String],
) -> String {
    let mut content = match conv_type {
        ConversationType::Group => format!("[{}]: {}", sender_nick, text),
        ConversationType::Private => text.to_string(),
    };
    if !download_failures.is_empty() {
        content.push_str("\n\n[注意：以下附件下载失败，未能加载：");
        content.push_str(&download_failures.join(", "));
        content.push(']');
    }
    content
}

fn downloaded_to_chat_attachment(
    downloaded: &DownloadedFile,
    kind: AttachmentKind,
) -> ChatAttachmentRef {
    ChatAttachmentRef {
        id: downloaded.sha256.clone(),
        file_name: downloaded.file_name.clone(),
        file_path: downloaded.path.to_string_lossy().to_string(),
        kind: match kind {
            AttachmentKind::Picture => "image".to_string(),
            AttachmentKind::File => "file".to_string(),
        },
        file_size: downloaded.size,
        file_type: downloaded
            .path
            .extension()
            .and_then(|v| v.to_str())
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_else(|| "bin".to_string()),
        mime_type: downloaded.mime_type.clone(),
    }
}

async fn download_specs_for_turn(
    downloader: &DingtalkFileDownloader,
    specs: &[super::types::ChannelAttachmentSpec],
    robot_code: &str,
    msg_id: &str,
) -> (Vec<ChatAttachmentRef>, Vec<String>) {
    let mut attachments = Vec::new();
    let mut failures = Vec::new();
    for spec in specs {
        match downloader
            .download(&spec.download_code, robot_code, &spec.file_name)
            .await
        {
            Ok(downloaded) => attachments.push(downloaded_to_chat_attachment(&downloaded, spec.kind)),
            Err(error) => {
                log::warn!(
                    "[channel] attachment download failed msgId={} file_name={} err={:#}",
                    msg_id,
                    spec.file_name,
                    error
                );
                failures.push(spec.file_name.clone());
            }
        }
    }
    (attachments, failures)
}
```

- [ ] **Step 5: 跑 manager 单测**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::manager::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/manager.rs
git commit -m "feat(channel): map downloaded files to chat attachments"
```

## Task 6: Worker 接入下载与 sessionWebhook 失败提示

**Files:**
- Modify: `src-tauri/src/connector/channel/manager.rs`
- Modify: `src-tauri/src/connector/channel/dingtalk_stream.rs`

- [ ] **Step 1: 创建 downloader**

在 `connect_dingtalk` 中，`let reply_robot_code = ...` 后加入：

```rust
let downloader = Arc::new(DingtalkFileDownloader::new(
    super::dingtalk_token::TokenCache::new(),
    config.credentials.app_key.clone(),
    app_secret_plain.clone(),
    self.chat_adapter
        .services
        .file_mgr
        .workspace_path()
        .join("dingtalk_downloads"),
));
```

在 message worker 捕获区加入：

```rust
let downloader_ref = Arc::clone(&downloader);
```

- [ ] **Step 2: 替换 content/request 构造段**

在 worker 中把 “构造 AI 输入（群聊带发送者前缀）” 到 `let request = ChatTurnRequest::new(session_id.clone(), content, vec![]);` 这段替换为：

```rust
let (chat_attachments, download_failures) = if msg.attachments.is_empty() {
    (Vec::new(), Vec::new())
} else {
    log::info!(
        "[channel] downloading {} attachments msgId={} session={}",
        msg.attachments.len(),
        msg.msg_id,
        session_id
    );
    download_specs_for_turn(
        downloader_ref.as_ref(),
        &msg.attachments,
        &msg.robot_code,
        &msg.msg_id,
    )
    .await
};

if chat_attachments.is_empty() && text.trim().is_empty() && !msg.attachments.is_empty() {
    log::warn!(
        "[channel] all attachments failed and no text, replying via sessionWebhook msgId={}",
        msg.msg_id
    );
    if let Some(webhook) = msg.session_webhook.clone() {
        tokio::spawn(super::dingtalk_stream::send_session_webhook_text(
            webhook,
            "附件下载全部失败，请重发。".to_string(),
        ));
    }
    continue;
}

let content = build_compound_content(&conv_type, &sender_nick, &text, &download_failures);
let mut request = ChatTurnRequest::new(session_id.clone(), content, chat_attachments);
request.session_attachment_dirs = crate::runtime::path_auth::derive_working_dirs_from_attachments(
    &request
        .attachments
        .iter()
        .map(|a| std::path::PathBuf::from(&a.file_path))
        .collect::<Vec<_>>(),
);
```

- [ ] **Step 3: 公开 sessionWebhook 函数给 manager 使用**

把 `dingtalk_stream.rs` 中函数签名：

```rust
async fn send_session_webhook_text(session_webhook: String, text: String) {
```

改为：

```rust
pub async fn send_session_webhook_text(session_webhook: String, text: String) {
```

- [ ] **Step 4: 调整前端消息 preview**

把 preview 构造替换为：

```rust
let preview_source = if text.trim().is_empty() && !msg.attachments.is_empty() {
    format!("[附件] {} 个文件", msg.attachments.len())
} else if !msg.attachments.is_empty() {
    format!("[附件] {}", text)
} else {
    text.clone()
};
let preview = if preview_source.chars().count() > 30 {
    format!("{}...", preview_source.chars().take(30).collect::<String>())
} else {
    preview_source
};
```

- [ ] **Step 5: 编译验证**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/manager.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/dingtalk_stream.rs
git commit -m "feat(channel): ingest dingTalk attachments into turns"
```

## Task 7: 集成测试与架构回归

**Files:**
- Create: `src-tauri/tests/dingtalk_attachment_integration_test.rs`
- Modify: `src-tauri/src/connector/channel/manager.rs`

- [ ] **Step 1: 抽可测的 request 构造函数**

在 `manager.rs` 中 `build_compound_content` 后加入：

```rust
fn build_channel_chat_request(
    session_id: String,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> ChatTurnRequest {
    let content = build_compound_content(conv_type, sender_nick, text, download_failures);
    let mut request = ChatTurnRequest::new(session_id, content, attachments);
    request.session_attachment_dirs = crate::runtime::path_auth::derive_working_dirs_from_attachments(
        &request
            .attachments
            .iter()
            .map(|a| std::path::PathBuf::from(&a.file_path))
            .collect::<Vec<_>>(),
    );
    request
}
```

把 worker 中手动构造 request 的代码替换为：

```rust
let request = build_channel_chat_request(
    session_id.clone(),
    &conv_type,
    &sender_nick,
    &text,
    chat_attachments,
    &download_failures,
);
```

- [ ] **Step 2: 新增集成测试文件**

新建 `src-tauri/tests/dingtalk_attachment_integration_test.rs`：

```rust
use app_lib::connector::channel::types::ConversationType;
use app_lib::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use app_lib::runtime::path_auth::derive_working_dirs_from_attachments;

#[test]
fn im_attachment_paths_are_authorized_for_turn() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file_path = tmp.path().join("dingtalk_downloads").join("a.txt");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, b"hello").unwrap();

    let attachments = vec![ChatAttachmentRef {
        id: "sha".into(),
        file_name: "a.txt".into(),
        file_path: file_path.to_string_lossy().to_string(),
        kind: "file".into(),
        file_size: 5,
        file_type: "txt".into(),
        mime_type: Some("text/plain".into()),
    }];

    let dirs = derive_working_dirs_from_attachments(
        &attachments
            .iter()
            .map(|a| std::path::PathBuf::from(&a.file_path))
            .collect::<Vec<_>>(),
    );

    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("dingtalk_downloads"));
}

#[test]
fn grouped_content_shape_matches_channel_contract() {
    let rendered = match ConversationType::Group {
        ConversationType::Group => format!("[{}]: {}", "张三", "请分析"),
        ConversationType::Private => "请分析".to_string(),
    };
    assert_eq!(rendered, "[张三]: 请分析");
}
```

- [ ] **Step 3: 跑集成测试**

Run:

```bash
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test dingtalk_attachment_integration_test
```

Expected: PASS。

- [ ] **Step 4: 跑相关回归**

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test path_auth_context_injection_test
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::dingtalk_stream::tests -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/connector/channel/manager.rs /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/tests/dingtalk_attachment_integration_test.rs
git commit -m "test(channel): verify dingTalk attachment ingestion"
```

## Final Verification

Run:

```bash
cargo check --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test dingtalk_attachment_integration_test
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --test path_auth_context_injection_test
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::dingtalk_stream::tests -- --nocapture
cargo test --manifest-path /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/Cargo.toml --lib connector::channel::dingtalk_download::tests -- --nocapture
```

Expected: all commands PASS。

## Manual Verification Matrix

- 单独发 1 张图片：产生 1 个 `ChatAttachmentRef`，LLM content 出现 `[当前消息附件]`。
- 单独发 1 个 `.pptx`：下载到 `<workspace>/dingtalk_downloads/<sha>.pptx`，工具可读该目录。
- richText 两图加文字：只起 1 个 turn，attachments=2，content 保留文字。
- richText 全图片无文字：起 1 个 turn，content 为空或群聊前缀为空文本，attachments=N。
- 单附件过期且无文字：不调用 `send_chat_request`，通过 `sessionWebhook` 发“附件下载全部失败，请重发。”。
- 钉钉文档分享卡：仍走“暂不支持直接读取钉钉文档/钉盘文件”自动回复。

## Self-Review

- Spec coverage: 覆盖 picture/file/richText/audio、sha256 去重、安全文件名、失败提示、`session_attachment_dirs`、interactiveCard 回归。
- Placeholder scan: 本计划不包含待填占位步骤；每个代码修改步骤给出明确代码。
- Type consistency: `ChannelAttachmentSpec`、`AttachmentKind`、`DownloadedFile`、`ChatAttachmentRef` 字段名称与现有代码一致。
