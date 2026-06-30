//! Telegram Bot API HTTP client (thin reqwest wrapper).
//!
//! Endpoints used:
//! - GET  /bot<token>/getMe                          → bot info（save 时验证 token）
//! - GET  /bot<token>/getUpdates?offset=N&timeout=25 → 长轮询入站
//! - POST /bot<token>/sendMessage                    → 出站
//! - POST /bot<token>/editMessageText                → 流式预览更新
//!
//! Bot API 不接受 token query string，token 在 URL path 里。MarkdownV2 解析失败
//! 在 sender 层做 plain text fallback。429 / 401 / 400 错误码映射给调用者。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::RwLock;

use crate::connector::im::shared::proxy::build_reqwest_client_with_proxy;

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";
const TELEGRAM_API_HOST: &str = "api.telegram.org";

/// HTTP client wrapping a single bot token. Construct one per connector.
pub struct TelegramApi {
    token: String,
    api_base: String,
    http: RwLock<reqwest::Client>,
    client_timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum TelegramApiError {
    #[error("invalid token / unauthorized: {0}")]
    Unauthorized(String),
    #[error("rate limited; retry after {retry_after:?}")]
    TooManyRequests { retry_after: Duration },
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// TCP 还没建好（DNS/connect refused/unreachable）。可安全重试。
    #[error("http connect: {0}")]
    TransportConnect(String),
    /// TCP 建好后断（reset/timeout/服务端中途断）。**不可重试**（消息可能已抵达）。
    #[error("http connected-then-broke: {0}")]
    TransportConnected(String),
    /// 本地附件超过 sendDocument 上限（Bot API 50MB）。
    #[error("file too big: {size} bytes (limit {limit})")]
    FileTooBig { size: u64, limit: u64 },
    /// 读本地文件失败（NotFound / permission）。
    #[error("io: {0}")]
    IoError(String),
    #[error("server error: {0}")]
    ServerError(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct BotInfo {
    pub id: i64,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgUser {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgPhotoSize {
    pub file_id: String,
    #[serde(default)]
    pub file_unique_id: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgDocument {
    pub file_id: String,
    #[serde(default)]
    pub file_unique_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgVoice {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgAudio {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgVideo {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgVideoNote {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgSticker {
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default)]
    pub set_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgAnimation {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgMessage {
    pub message_id: i64,
    #[serde(default)]
    pub from: Option<TgUser>,
    pub chat: TgChat,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub date: Option<i64>,
    /// 图片消息：服务端按多个 size 返回，最大尺寸是数组最后一个。MVP 下载最大一张。
    #[serde(default)]
    pub photo: Option<Vec<TgPhotoSize>>,
    /// 文件（任意类型，包括 zip / pdf / 二进制等）。区别于 photo / video / audio 三种
    /// 特化字段，document 是"通用附件"通道，可携带 file_name + mime_type。
    #[serde(default)]
    pub document: Option<TgDocument>,
    /// 图片/视频/文件可带的文字说明。当 photo/document 存在但 text 缺失时，caption
    /// 作为消息正文使用。
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub voice: Option<TgVoice>,
    #[serde(default)]
    pub audio: Option<TgAudio>,
    #[serde(default)]
    pub video: Option<TgVideo>,
    #[serde(default)]
    pub video_note: Option<TgVideoNote>,
    #[serde(default)]
    pub sticker: Option<TgSticker>,
    #[serde(default)]
    pub animation: Option<TgAnimation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgCallbackQuery {
    pub id: String,
    pub from: TgUser,
    #[serde(default)]
    pub message: Option<TgMessage>,
    #[serde(default)]
    pub data: Option<String>,
}

/// `getFile` 响应：包含 `file_path`，用来拼下载 URL
/// `https://api.telegram.org/file/bot<token>/<file_path>`。
#[derive(Debug, Clone, Deserialize)]
pub struct TgFile {
    pub file_id: String,
    #[serde(default)]
    pub file_unique_id: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    /// 相对路径形如 `photos/file_5.jpg` 或 `documents/file_3.bin`。可能缺失（罕见），
    /// 缺失时无法下载。
    #[serde(default)]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgUpdate {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<TgMessage>,
    #[serde(default)]
    pub callback_query: Option<TgCallbackQuery>,
}

#[derive(Debug, Serialize)]
struct SendMessageBody<'a> {
    chat_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ReactionEmoji<'a> {
    #[serde(rename = "type")]
    reaction_type: &'static str,
    emoji: &'a str,
}

#[derive(Debug, Serialize)]
struct SetMessageReactionBody<'a> {
    chat_id: i64,
    message_id: i64,
    reaction: Vec<ReactionEmoji<'a>>,
}

#[derive(Debug, Serialize)]
struct EditMessageTextBody<'a> {
    chat_id: i64,
    message_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TgInlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TgInlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<TgInlineKeyboardButton>>,
}

#[derive(Debug, Serialize)]
struct SendMessageWithMarkupBody<'a> {
    chat_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
    reply_markup: TgInlineKeyboardMarkup,
}

#[derive(Debug, Serialize)]
struct AnswerCallbackQueryBody<'a> {
    callback_query_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    show_alert: bool,
}

#[derive(Debug, Serialize)]
struct EditMessageReplyMarkupBody {
    chat_id: i64,
    message_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_markup: Option<TgInlineKeyboardMarkup>,
}

#[derive(Debug, Serialize)]
struct SendChatActionBody<'a> {
    chat_id: i64,
    action: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct Envelope<T> {
    ok: bool,
    #[serde(default = "Option::default")]
    result: Option<T>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    error_code: Option<i64>,
    #[serde(default)]
    parameters: Option<EnvelopeParams>,
}

#[derive(Debug, Deserialize)]
struct EnvelopeParams {
    #[serde(default)]
    retry_after: Option<u64>,
}

fn classify_reqwest_error(e: reqwest::Error) -> TelegramApiError {
    let s = e.to_string();
    if e.is_connect() || s.contains("Connection refused") || s.contains("connect error") {
        TelegramApiError::TransportConnect(s)
    } else {
        TelegramApiError::TransportConnected(s)
    }
}

impl TelegramApi {
    pub fn new(token: String) -> Result<Self> {
        let timeout = Duration::from_secs(35);
        // 走 shared::proxy 统一出口,跟 WhatsApp 同源。resolver 没拿到代理 →
        // 显式 .no_proxy(),避免 reqwest 自己读 HTTPS_PROXY(大写优先)绕过
        // 我们的优先级。
        let http = build_reqwest_client_with_proxy(timeout, TELEGRAM_API_HOST)?;
        Ok(Self {
            token,
            api_base: TELEGRAM_API_BASE.to_string(),
            http: RwLock::new(http),
            client_timeout: timeout,
        })
    }

    #[doc(hidden)]
    pub fn new_with_api_base_for_tests(token: String, api_base: String) -> Result<Self> {
        let timeout = Duration::from_secs(5);
        Ok(Self {
            token,
            api_base,
            http: RwLock::new(
                reqwest::Client::builder()
                    .no_proxy()
                    .timeout(timeout)
                    .build()?,
            ),
            client_timeout: timeout,
        })
    }

    /// 丢弃当前 reqwest client（释放 keep-alive），新建替换。
    /// Watchdog 检测到 stall 时调用，强制下次请求走新连接。
    pub async fn rebuild_client(&self) -> Result<()> {
        let new_client = if self.api_base != TELEGRAM_API_BASE {
            // mockito / 本地 test server,不走代理。
            reqwest::Client::builder()
                .no_proxy()
                .timeout(self.client_timeout)
                .build()?
        } else {
            // 跟 ::new() 同路径,重连后代理设置仍生效。
            build_reqwest_client_with_proxy(self.client_timeout, TELEGRAM_API_HOST)?
        };
        *self.http.write().await = new_client;
        Ok(())
    }

    fn url(&self, method: &str) -> String {
        // token 在 path 里；日志请勿打整 URL。
        format!("{}/bot{}/{}", self.api_base, self.token, method)
    }

    /// 验证 token + 拿 bot 元数据。`save` 调用一次。
    pub async fn get_me(&self) -> Result<BotInfo, TelegramApiError> {
        // clone-and-drop-guard: reqwest::Client 内部 Arc'd，clone 廉价。
        // 不持锁跨 `.send().await` 是为了让 watchdog 的 rebuild_client (write lock)
        // 不必等 25s 长轮询 send 完成才能抢到锁——那正是它要救的死锁场景。
        let client = self.http.read().await.clone();
        let resp = client
            .get(self.url("getMe"))
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        parse_envelope::<BotInfo>(resp).await
    }

    /// 长轮询拉 updates。`offset` = next_update_id（已消费的最大 id + 1）。
    pub async fn get_updates(
        &self,
        offset: i64,
        timeout_secs: u64,
    ) -> Result<Vec<TgUpdate>, TelegramApiError> {
        let url = format!(
            "{}?offset={}&timeout={}&allowed_updates=%5B%22message%22%2C%22callback_query%22%5D",
            self.url("getUpdates"),
            offset,
            timeout_secs,
        );
        let client = self.http.read().await.clone();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let status = resp.status();
        let body = resp.text().await.map_err(classify_reqwest_error)?;

        // DEV diagnostic：把非空 result（即有真实 update）的原始 JSON 打印到日志，
        // 用来排查"图片/文件消息为什么没识别"。空 result（long-poll 25s timeout 返回 []）
        // 会被忽略，避免日志泛滥。
        if let Ok(env) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(arr) = env.get("result").and_then(|v| v.as_array()) {
                if !arr.is_empty() {
                    log::info!(
                        "[telegram-api] getUpdates raw body (count={}): {}",
                        arr.len(),
                        body
                    );
                }
            }
        }

        // 走标准 envelope 解析（同 parse_envelope 流程，但已经把 body 读出来了，
        // 不能再用 parse_envelope；这里就地展开逻辑）。
        let env: Envelope<Vec<TgUpdate>> = serde_json::from_str(&body).map_err(|e| {
            TelegramApiError::TransportConnected(format!(
                "parse telegram envelope status={status} err={e} body={body}"
            ))
        })?;
        if env.ok {
            env.result.ok_or_else(|| {
                TelegramApiError::TransportConnected("ok=true but result missing".into())
            })
        } else {
            let desc = env.description.unwrap_or_default();
            match env.error_code.unwrap_or(0) {
                401 => Err(TelegramApiError::Unauthorized(desc)),
                403 => Err(TelegramApiError::Forbidden(desc)),
                429 => {
                    let secs = env.parameters.and_then(|p| p.retry_after).unwrap_or(1);
                    Err(TelegramApiError::TooManyRequests {
                        retry_after: Duration::from_secs(secs),
                    })
                }
                400 => Err(TelegramApiError::BadRequest(desc)),
                code if (500..600).contains(&code) => Err(TelegramApiError::ServerError(desc)),
                _ => Err(TelegramApiError::TransportConnected(format!(
                    "unknown error_code={} desc={}",
                    env.error_code.unwrap_or(0),
                    desc
                ))),
            }
        }
    }

    /// 出站。`parse_mode=None` 走 plain text。
    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<TgMessage, TelegramApiError> {
        self.send_message_with_reply(chat_id, text, parse_mode, None)
            .await
    }

    /// 出站，支持 `reply_to_message_id`。
    pub async fn send_message_with_reply(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
        reply_to_message_id: Option<i64>,
    ) -> Result<TgMessage, TelegramApiError> {
        let body = SendMessageBody {
            chat_id,
            text,
            parse_mode,
            reply_to_message_id,
        };
        let client = self.http.read().await.clone();
        let resp = client
            .post(self.url("sendMessage"))
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        parse_envelope::<TgMessage>(resp).await
    }

    /// 给指定入站消息设置或清空 reaction。失败由上层按 best-effort 处理。
    pub async fn set_message_reaction(
        &self,
        chat_id: i64,
        message_id: i64,
        emoji: Option<&str>,
    ) -> Result<(), TelegramApiError> {
        let emoji = emoji.and_then(|e| {
            let trimmed = e.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let body = SetMessageReactionBody {
            chat_id,
            message_id,
            reaction: emoji
                .map(|emoji| {
                    vec![ReactionEmoji {
                        reaction_type: "emoji",
                        emoji,
                    }]
                })
                .unwrap_or_default(),
        };
        let client = self.http.read().await.clone();
        let resp = client
            .post(self.url("setMessageReaction"))
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        parse_envelope::<bool>(resp).await.map(|_| ())
    }

    /// 编辑 bot 自己已发送的文本消息，用于 Telegram 流式预览。
    pub async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<(), TelegramApiError> {
        let body = EditMessageTextBody {
            chat_id,
            message_id,
            text,
            parse_mode,
        };
        let client = self.http.read().await.clone();
        let resp = client
            .post(self.url("editMessageText"))
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        parse_envelope::<serde_json::Value>(resp).await.map(|_| ())
    }

    pub async fn send_message_with_inline_keyboard(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
        reply_to_message_id: Option<i64>,
        reply_markup: TgInlineKeyboardMarkup,
    ) -> Result<TgMessage, TelegramApiError> {
        let body = SendMessageWithMarkupBody {
            chat_id,
            text,
            parse_mode,
            reply_to_message_id,
            reply_markup,
        };
        let client = self.http.read().await.clone();
        let resp = client
            .post(self.url("sendMessage"))
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        parse_envelope::<TgMessage>(resp).await
    }

    pub async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
        show_alert: bool,
    ) -> Result<(), TelegramApiError> {
        let body = AnswerCallbackQueryBody {
            callback_query_id,
            text,
            show_alert,
        };
        let client = self.http.read().await.clone();
        let resp = client
            .post(self.url("answerCallbackQuery"))
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let _ = parse_envelope::<bool>(resp).await?;
        Ok(())
    }

    pub async fn edit_message_reply_markup(
        &self,
        chat_id: i64,
        message_id: i64,
        reply_markup: Option<TgInlineKeyboardMarkup>,
    ) -> Result<(), TelegramApiError> {
        let body = EditMessageReplyMarkupBody {
            chat_id,
            message_id,
            reply_markup,
        };
        let client = self.http.read().await.clone();
        let resp = client
            .post(self.url("editMessageReplyMarkup"))
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let _ = parse_envelope::<serde_json::Value>(resp).await?;
        Ok(())
    }

    pub async fn send_chat_action(
        &self,
        chat_id: i64,
        action: &str,
    ) -> Result<(), TelegramApiError> {
        let body = SendChatActionBody { chat_id, action };
        let client = self.http.read().await.clone();
        let resp = client
            .post(self.url("sendChatAction"))
            .json(&body)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let _ = parse_envelope::<bool>(resp).await?;
        Ok(())
    }

    /// 上传本地文件到 Telegram（sendDocument multipart）。
    /// 超过 50MB 立即返回 `FileTooBig`；文件不存在返回 `IoError`。
    pub async fn send_document(
        &self,
        chat_id: i64,
        file_path: &std::path::Path,
        caption: Option<&str>,
        reply_to_message_id: Option<i64>,
    ) -> Result<TgMessage, TelegramApiError> {
        const MAX_BYTES: u64 = 50 * 1024 * 1024;

        let meta = tokio::fs::metadata(file_path)
            .await
            .map_err(|e| TelegramApiError::IoError(format!("stat {}: {e}", file_path.display())))?;
        let size = meta.len();
        if size > MAX_BYTES {
            return Err(TelegramApiError::FileTooBig {
                size,
                limit: MAX_BYTES,
            });
        }
        let bytes = tokio::fs::read(file_path)
            .await
            .map_err(|e| TelegramApiError::IoError(format!("read {}: {e}", file_path.display())))?;
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file.bin")
            .to_string();

        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part(
                "document",
                reqwest::multipart::Part::bytes(bytes).file_name(file_name),
            );
        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
            form = form.text("parse_mode", "HTML");
        }
        if let Some(reply_id) = reply_to_message_id {
            form = form.text("reply_to_message_id", reply_id.to_string());
        }

        let client = self.http.read().await.clone();
        let resp = client
            .post(self.url("sendDocument"))
            .multipart(form)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        parse_envelope::<TgMessage>(resp).await
    }

    /// 解析 file_id → 拿到 `file_path`，下游凭 `download_file_to` 真正取字节。
    /// `file_size` 由调用方决定是否拒绝（Telegram 上限 20MB free / 2GB premium，
    /// 但 Bot API 自己的 `getFile` 接口对 result 限制 20MB——超过会返回
    /// `400 Bad Request: file is too big`）。
    pub async fn get_file(&self, file_id: &str) -> Result<TgFile, TelegramApiError> {
        let url = format!(
            "{}?file_id={}",
            self.url("getFile"),
            urlencoding::encode(file_id)
        );
        let client = self.http.read().await.clone();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        parse_envelope::<TgFile>(resp).await
    }

    /// 下载 `file_path`（来自 `get_file`）对应的原始字节。返回字节流给调用方
    /// 自己写盘（让上层决定文件名 / sha256 / 落盘路径）。
    /// URL 形式：`https://api.telegram.org/file/bot<token>/<file_path>`。
    pub async fn download_file(&self, file_path: &str) -> Result<Vec<u8>, TelegramApiError> {
        // 注意 file_path 本身已经是 `photos/...` 这种相对路径，不再 encode。
        let url = format!("{}/file/bot{}/{}", self.api_base, self.token, file_path);

        // SSRF 防御（PR4）：file_path 来自上游 API 响应（理论上可信），但作为深度防御
        // 加 host + scheme 校验。仅对生产 api_base 严格；测试 api_base 跳过。
        let is_default_api = self.api_base == TELEGRAM_API_BASE;
        if is_default_api {
            let parsed = url::Url::parse(&url)
                .map_err(|e| TelegramApiError::TransportConnected(format!("invalid url: {e}")))?;
            if parsed.host_str() != Some("api.telegram.org") {
                return Err(TelegramApiError::TransportConnected(format!(
                    "SSRF rejected: host {:?} not allowed",
                    parsed.host_str()
                )));
            }
            if parsed.scheme() != "https" {
                return Err(TelegramApiError::TransportConnected(format!(
                    "SSRF rejected: scheme {} not allowed",
                    parsed.scheme()
                )));
            }
        }

        let client = self.http.read().await.clone();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return match status.as_u16() {
                401 => Err(TelegramApiError::Unauthorized(body)),
                403 => Err(TelegramApiError::Forbidden(body)),
                400 => Err(TelegramApiError::BadRequest(body)),
                429 => Err(TelegramApiError::TooManyRequests {
                    retry_after: Duration::from_secs(1),
                }),
                code if (500..600).contains(&code) => Err(TelegramApiError::ServerError(body)),
                _ => Err(TelegramApiError::TransportConnected(format!(
                    "download status={status} body={body}"
                ))),
            };
        }
        let bytes = resp.bytes().await.map_err(classify_reqwest_error)?;
        Ok(bytes.to_vec())
    }
}

async fn parse_envelope<T: for<'de> Deserialize<'de>>(
    resp: reqwest::Response,
) -> Result<T, TelegramApiError> {
    let status = resp.status();
    let body = resp.text().await.map_err(classify_reqwest_error)?;
    let env: Envelope<T> = serde_json::from_str(&body)
        .with_context(|| format!("parse telegram envelope status={status} body={body}"))
        .map_err(|e| TelegramApiError::TransportConnected(e.to_string()))?;
    if env.ok {
        env.result.ok_or_else(|| {
            TelegramApiError::TransportConnected("ok=true but result missing".to_string())
        })
    } else {
        let desc = env.description.unwrap_or_default();
        match env.error_code.unwrap_or(0) {
            401 => Err(TelegramApiError::Unauthorized(desc)),
            403 => Err(TelegramApiError::Forbidden(desc)),
            429 => {
                let secs = env.parameters.and_then(|p| p.retry_after).unwrap_or(1);
                Err(TelegramApiError::TooManyRequests {
                    retry_after: Duration::from_secs(secs),
                })
            }
            400 => Err(TelegramApiError::BadRequest(desc)),
            code if (500..600).contains(&code) => Err(TelegramApiError::ServerError(desc)),
            _ => Err(TelegramApiError::TransportConnected(format!(
                "unknown error_code={} desc={}",
                env.error_code.unwrap_or(0),
                desc
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_me_parses_username_and_first_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/botTESTTOKEN/getMe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "id": 8123, "username": "test_bot", "first_name": "Test Bot" }
            })))
            .mount(&server)
            .await;
        let api =
            TelegramApi::new_with_api_base_for_tests("TESTTOKEN".into(), server.uri()).unwrap();
        let info = api.get_me().await.unwrap();
        assert_eq!(info.id, 8123);
        assert_eq!(info.username.as_deref(), Some("test_bot"));
        assert_eq!(info.first_name, "Test Bot");
    }

    #[tokio::test]
    async fn unauthorized_envelope_maps_to_unauthorized_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/botBADTOKEN/getMe"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "ok": false, "error_code": 401, "description": "Unauthorized"
            })))
            .mount(&server)
            .await;
        let api =
            TelegramApi::new_with_api_base_for_tests("BADTOKEN".into(), server.uri()).unwrap();
        match api.get_me().await {
            Err(TelegramApiError::Unauthorized(_)) => {}
            other => panic!("expected Unauthorized, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn too_many_requests_returns_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/botT/sendMessage"))
            .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "ok": false, "error_code": 429, "description": "Too Many Requests",
                "parameters": { "retry_after": 7 }
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();
        match api.send_message(1, "hi", None).await {
            Err(TelegramApiError::TooManyRequests { retry_after }) => {
                assert_eq!(retry_after, Duration::from_secs(7));
            }
            other => panic!("expected TooManyRequests, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn get_updates_returns_empty_list() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/botT/getUpdates"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "result": []
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();
        let updates = api.get_updates(0, 25).await.unwrap();
        assert!(updates.is_empty());
    }

    #[tokio::test]
    async fn set_message_reaction_posts_single_emoji() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/botT/setMessageReaction"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "result": true
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();

        api.set_message_reaction(42, 777, Some("👀")).await.unwrap();

        let calls = server.received_requests().await.unwrap();
        assert_eq!(calls.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&calls[0].body).unwrap();
        assert_eq!(body["chat_id"], 42);
        assert_eq!(body["message_id"], 777);
        assert_eq!(
            body["reaction"],
            serde_json::json!([{ "type": "emoji", "emoji": "👀" }])
        );
    }

    #[tokio::test]
    async fn set_message_reaction_can_clear_reaction() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/botT/setMessageReaction"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "result": true
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();

        api.set_message_reaction(42, 777, None).await.unwrap();

        let calls = server.received_requests().await.unwrap();
        assert_eq!(calls.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&calls[0].body).unwrap();
        assert_eq!(body["chat_id"], 42);
        assert_eq!(body["message_id"], 777);
        assert_eq!(body["reaction"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn rebuild_client_does_not_break_subsequent_calls() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/botT/getMe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "id": 1, "first_name": "T" }
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();
        api.get_me().await.unwrap();
        api.rebuild_client().await.unwrap();
        api.get_me().await.unwrap();
    }

    #[tokio::test]
    async fn connect_to_nonexistent_port_classified_as_transport_connect() {
        let api =
            TelegramApi::new_with_api_base_for_tests("T".into(), "http://127.0.0.1:10".into())
                .unwrap();
        match api.send_message(1, "hi", None).await {
            Err(TelegramApiError::TransportConnect(_)) => {}
            other => panic!("expected TransportConnect, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn send_document_uploads_multipart_successfully() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/botT/sendDocument"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": {
                    "message_id": 777,
                    "chat": { "id": 42, "type": "private" }
                }
            })))
            .mount(&server)
            .await;

        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("report.xlsx");
        std::fs::write(&file_path, b"fake xlsx content").unwrap();

        let msg = api.send_document(42, &file_path, None, None).await.unwrap();
        assert_eq!(msg.message_id, 777);
        // TempDir drops here
    }

    #[tokio::test]
    async fn send_document_rejects_files_over_50mb() {
        let api =
            TelegramApi::new_with_api_base_for_tests("T".into(), "http://127.0.0.1:10".into())
                .unwrap();
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("big.bin");
        // Create a file with metadata size > 50MB by writing a sparse file
        {
            use std::fs::OpenOptions;
            use std::io::{Seek, SeekFrom, Write};
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .open(&file_path)
                .unwrap();
            // Seek past 51MB, write 1 byte to make the file 51MB+1
            f.seek(SeekFrom::Start(51 * 1024 * 1024)).unwrap();
            f.write_all(&[0u8]).unwrap();
        }
        match api.send_document(42, &file_path, None, None).await {
            Err(TelegramApiError::FileTooBig { size, limit }) => {
                assert!(size > limit);
            }
            other => panic!("expected FileTooBig, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn send_document_nonexistent_path_returns_io_error() {
        let api =
            TelegramApi::new_with_api_base_for_tests("T".into(), "http://127.0.0.1:10".into())
                .unwrap();
        let path = std::path::Path::new("/nonexistent/path/to/missing.xlsx");
        match api.send_document(42, path, None, None).await {
            Err(TelegramApiError::IoError(_)) => {}
            other => panic!("expected IoError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn download_file_with_normal_path_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file/botT/photos/file_1.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"jpegbytes".to_vec()))
            .mount(&server)
            .await;
        // 使用 mock server base URL（非 TELEGRAM_API_BASE），SSRF 检查跳过
        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();
        let res = api.download_file("photos/file_1.jpg").await;
        assert!(
            res.is_ok(),
            "normal path should not be rejected, got {:?}",
            res
        );
        assert_eq!(res.unwrap(), b"jpegbytes");
    }
}
