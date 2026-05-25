//! iLink HTTP API client — `getUpdates` (long-poll) + `sendMessage`.
//!
//! 协议参考 openclaw-weixin-main/src/api/api.ts。所有业务接口都是 POST，
//! header `AuthorizationType: ilink_bot_token` + `Authorization: Bearer <token>`，
//! + 通用 `iLink-App-Id` / `iLink-App-ClientVersion` / `X-WECHAT-UIN`（由
//! `headers::build_headers` 统一拼装）。
//!
//! `base_url` 是 `LoginSession.effective_base_url` 落盘的值（IDC 路由后的
//! 实际地址，不是 endpoints::DEFAULT_BASE_URL）。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use super::endpoints::{GET_UPDATES, SEND_MESSAGE};
use super::headers::{build_headers, HeaderInputs};

/// 默认长轮询客户端超时。openclaw 默认 35s；我们 +5s 给上游处理消息留余量。
const DEFAULT_LONG_POLL_TIMEOUT_SECS: u64 = 40;
/// 业务 POST 默认超时（sendMessage 等非长轮询接口）。
const DEFAULT_BUSINESS_TIMEOUT_SECS: u64 = 15;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// 跟 openclaw 的 `BaseInfo` 对齐。本期只塞 `channel_version`；上行接口都带这个块。
#[derive(Debug, Clone, Serialize)]
struct BaseInfo {
    channel_version: String,
}

fn build_base_info(client_version: &str) -> BaseInfo {
    BaseInfo {
        channel_version: client_version.to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GetUpdatesReq {
    /// 上一次响应里的 `get_updates_buf`；第一次发 `""`。
    pub get_updates_buf: String,
    base_info: BaseInfo,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GetUpdatesResp {
    #[serde(default)]
    pub ret: i64,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    /// 服务端下发的新游标，后续 getUpdates 必须原样回传。
    #[serde(default)]
    pub get_updates_buf: Option<String>,
    /// 服务端建议的下次长轮询客户端超时（毫秒）。
    #[serde(default)]
    pub longpolling_timeout_ms: Option<u64>,
    #[serde(default)]
    pub msgs: Vec<WeixinMessage>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WeixinMessage {
    #[serde(default)]
    pub seq: Option<i64>,
    #[serde(default)]
    pub message_id: Option<i64>,
    #[serde(default)]
    pub from_user_id: Option<String>,
    #[serde(default)]
    pub to_user_id: Option<String>,
    /// 毫秒时间戳；服务端给。
    #[serde(default)]
    pub create_time_ms: Option<i64>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// `1` = USER (用户发给 bot)，`2` = BOT (我们自己回的，echo 回来要过滤)。
    #[serde(default)]
    pub message_type: Option<i64>,
    /// `0` NEW, `1` GENERATING, `2` FINISH —— 我们只 forward FINISH/NEW 的用户消息。
    #[serde(default)]
    pub message_state: Option<i64>,
    #[serde(default)]
    pub item_list: Vec<MessageItem>,
    /// 上下文 token。回信 sendMessage 必须把这个原样塞回去。
    #[serde(default)]
    pub context_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MessageItem {
    /// `1` TEXT, `2` IMAGE, `3` VOICE, `4` FILE, `5` VIDEO
    #[serde(default)]
    pub r#type: i64,
    #[serde(default)]
    pub text_item: Option<TextItem>,
    /// type=2 时承载图片下载元信息。
    #[serde(default)]
    pub image_item: Option<ImageItem>,
    /// type=4 时承载文件下载元信息。
    #[serde(default)]
    pub file_item: Option<FileItem>,
    /// 服务端用来标记媒体上传/转换是否完成；未完成的图片/文件 CDN 拉不到，
    /// 我们的 `extract_attachments_from_item_list` 跳过 false 的 item。
    #[serde(default)]
    pub is_completed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TextItem {
    #[serde(default)]
    pub text: String,
}

/// iLink CDN 下载元信息。`full_url` 已经带好 `encrypted_query_param` 的
/// query string，直接 GET 拉密文即可；`aes_key` 是 base64(16 raw bytes)
/// 或 base64(32 ASCII hex chars) 两种之一（详见 `media::parse_aes_key`）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CdnMedia {
    #[serde(default)]
    pub aes_key: String,
    #[serde(default)]
    pub full_url: String,
    /// 仅诊断用，本期不消费。`full_url` 已经包含这条 query string。
    #[serde(default)]
    pub encrypt_query_param: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ImageItem {
    /// 图片层的 aeskey，是裸 hex 32 字符。这是 openclaw 实现里优先用的字段，
    /// 比 `media.aes_key`（base64(hex)）更直接，少一次编码。两个值应等价。
    #[serde(default)]
    pub aeskey: Option<String>,
    #[serde(default)]
    pub media: Option<CdnMedia>,
    // 缩略图字段（thumb_*）本期不消费，省略。
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileItem {
    #[serde(default)]
    pub media: Option<CdnMedia>,
    /// 用户给的原始文件名。可能含中文，写盘前会经过 `extension_or_bin` 推扩展
    /// 名 + sha256 内容寻址，文件名本身只作为 display_name 透给 LLM。
    #[serde(default)]
    pub file_name: Option<String>,
    /// 服务端给的字符串字节数（"518728"），诊断/校验用，本期不消费。
    #[serde(default)]
    pub len: Option<String>,
    /// MD5 hex，诊断用。
    #[serde(default)]
    pub md5: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendMessageReq {
    msg: SendMessagePayload,
    base_info: BaseInfo,
}

#[derive(Debug, Clone, Serialize)]
struct SendMessagePayload {
    from_user_id: String,
    to_user_id: String,
    client_id: String,
    /// `2` = BOT
    message_type: i64,
    /// `2` = FINISH
    message_state: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_list: Option<Vec<SendItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SendItem {
    /// `1` = TEXT
    r#type: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_item: Option<TextItemOut>,
}

#[derive(Debug, Clone, Serialize)]
struct TextItemOut {
    text: String,
}

/// 构造发送 text 消息的请求体。`context_token` 来自此前同 user 的入站消息缓存
/// （`context-token cache`）；缺失时不带，服务端通常仍能 fallback，但回信落
/// 在不同会话窗的概率会上升。
pub fn build_text_send_req(
    to_user_id: &str,
    text: &str,
    client_id: &str,
    context_token: Option<&str>,
    client_version: &str,
) -> SendMessageReq {
    let item_list = if text.is_empty() {
        None
    } else {
        Some(vec![SendItem {
            r#type: 1,
            text_item: Some(TextItemOut {
                text: text.to_string(),
            }),
        }])
    };
    SendMessageReq {
        msg: SendMessagePayload {
            from_user_id: String::new(),
            to_user_id: to_user_id.to_string(),
            client_id: client_id.to_string(),
            message_type: 2,  // BOT
            message_state: 2, // FINISH
            item_list,
            context_token: context_token.map(|s| s.to_string()),
        },
        base_info: build_base_info(client_version),
    }
}

// ---------------------------------------------------------------------------
// Wire calls
// ---------------------------------------------------------------------------

/// POST `ilink/bot/getupdates`，长轮询。client-side 超时视为正常（return 空），
/// 调用方原样把上一次的 `get_updates_buf` 再传一次即可继续 long-poll。
pub async fn get_updates(
    client: &reqwest::Client,
    base_url: &str,
    bot_token: &str,
    app_id: &str,
    client_version: &str,
    get_updates_buf: String,
    timeout_secs: Option<u64>,
) -> Result<GetUpdatesResp> {
    let url = format!("{}/{}", base_url.trim_end_matches('/'), GET_UPDATES);
    let body = GetUpdatesReq {
        get_updates_buf: get_updates_buf.clone(),
        base_info: build_base_info(client_version),
    };
    let headers = build_headers(HeaderInputs {
        app_id,
        client_version,
        bot_token: Some(bot_token),
        route_tag: None,
    });
    let timeout = timeout_secs.unwrap_or(DEFAULT_LONG_POLL_TIMEOUT_SECS);
    match client
        .post(&url)
        .headers(headers)
        .timeout(std::time::Duration::from_secs(timeout))
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let raw = resp.text().await.context("wechat getUpdates: read body")?;
            if !status.is_success() {
                return Err(anyhow!("wechat getUpdates HTTP {}: {}", status, raw));
            }
            let parsed: GetUpdatesResp = serde_json::from_str(&raw)
                .with_context(|| format!("wechat getUpdates parse; raw={}", raw))?;
            Ok(parsed)
        }
        Err(e) if e.is_timeout() => {
            // 客户端超时跟 openclaw 处理一致：返回空 resp，调用方重试时把
            // 原 get_updates_buf 传回即可。
            Ok(GetUpdatesResp {
                ret: 0,
                get_updates_buf: Some(get_updates_buf),
                ..Default::default()
            })
        }
        Err(e) => Err(anyhow!("wechat getUpdates network: {e}")),
    }
}

/// POST `ilink/bot/sendmessage`。返回 () —— 服务端 200 即视作下发成功。
pub async fn send_message(
    client: &reqwest::Client,
    base_url: &str,
    bot_token: &str,
    app_id: &str,
    client_version: &str,
    body: &SendMessageReq,
) -> Result<()> {
    let url = format!("{}/{}", base_url.trim_end_matches('/'), SEND_MESSAGE);
    let headers = build_headers(HeaderInputs {
        app_id,
        client_version,
        bot_token: Some(bot_token),
        route_tag: None,
    });
    let resp = client
        .post(&url)
        .headers(headers)
        .timeout(std::time::Duration::from_secs(
            DEFAULT_BUSINESS_TIMEOUT_SECS,
        ))
        .json(body)
        .send()
        .await
        .context("wechat sendMessage: network")?;
    let status = resp.status();
    let raw = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("wechat sendMessage HTTP {}: {}", status, raw));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers — flatten WeixinMessage.item_list into a single text blob.
// ---------------------------------------------------------------------------

/// 把 WeixinMessage.item_list 里 TEXT 项拼成一段。其它类型（image / voice / file /
/// video）走附件下载路径不消费这里；为兼容兜底，仍返回占位串 `[图片]/[文件]/...`，
/// 避免 attachment 下载全部失败时下游拿到空 content 触发 Anthropic 400。
/// 真正的附件路径由 [`extract_attachments_from_item_list`] 处理。
pub fn flatten_item_list_to_text(items: &[MessageItem]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for it in items {
        match it.r#type {
            1 => {
                if let Some(t) = &it.text_item {
                    if !t.text.is_empty() {
                        parts.push(t.text.clone());
                    }
                }
            }
            2 => parts.push("[图片]".to_string()),
            3 => parts.push("[语音]".to_string()),
            4 => parts.push("[文件]".to_string()),
            5 => parts.push("[视频]".to_string()),
            _ => {}
        }
    }
    parts.join("\n")
}

/// 从 item_list 抽取所有可下载附件，每个生成一个 `ChannelAttachmentSpec`。
/// 本期只处理 image (type=2) / file (type=4)；voice / video 跳过留后续 PR。
///
/// `download_code` 形式 `wechat://{aes_key_b64}@{full_url}`，跟 wecom 一致。
/// 图片同时下发裸 hex `image_item.aeskey` 和 base64(hex) `media.aes_key`，
/// 优先用 `aeskey`（更直接，少一次编码），不存在再 fallback 到 `media.aes_key`。
///
/// 服务端 `is_completed = Some(false)` 的 item 跳过（CDN 还在上传，拉不到）。
/// 缺 `full_url` 或 `aes_key` 的也跳过 + 打 warn log，让链路日志能追到丢消息。
pub fn extract_attachments_from_item_list(
    items: &[MessageItem],
    msg_id_for_log: &str,
) -> Vec<crate::connector::im::types::ChannelAttachmentSpec> {
    use crate::connector::im::types::{AttachmentKind, ChannelAttachmentSpec};
    let mut out = Vec::new();
    for it in items {
        if matches!(it.is_completed, Some(false)) {
            log::info!(
                "[wechat-api] skip incomplete item type={} msg_id={msg_id_for_log}",
                it.r#type
            );
            continue;
        }
        match it.r#type {
            2 => {
                let Some(img) = it.image_item.as_ref() else {
                    log::warn!(
                        "[wechat-api] image item without image_item body, msg_id={msg_id_for_log}"
                    );
                    continue;
                };
                let media = match img.media.as_ref() {
                    Some(m) if !m.full_url.is_empty() => m,
                    _ => {
                        log::warn!(
                            "[wechat-api] image item missing media.full_url, msg_id={msg_id_for_log}"
                        );
                        continue;
                    }
                };
                // 优先 image_item.aeskey（hex），不存在就用 media.aes_key（base64(hex)）
                let aes_key_b64 = match img.aeskey.as_deref().filter(|s| !s.is_empty()) {
                    Some(hex) => match super::media::hex_aeskey_to_base64(hex) {
                        Ok(b) => b,
                        Err(e) => {
                            log::warn!(
                                "[wechat-api] image aeskey hex→base64 failed msg_id={msg_id_for_log}: {e:#}"
                            );
                            continue;
                        }
                    },
                    None if !media.aes_key.is_empty() => media.aes_key.clone(),
                    None => {
                        log::warn!(
                            "[wechat-api] image item missing aes_key, msg_id={msg_id_for_log}"
                        );
                        continue;
                    }
                };
                out.push(ChannelAttachmentSpec {
                    kind: AttachmentKind::Picture,
                    download_code: format!("wechat://{aes_key_b64}@{}", media.full_url),
                    // 图片没有文件名，用 msg_id 占位；扩展名走 .jpg 默认（CDN 内容是
                    // 解密后的图片字节，wechat 个人微信侧 99% 是 JPEG）
                    file_name: format!("wechat-image-{msg_id_for_log}.jpg"),
                });
            }
            4 => {
                let Some(file) = it.file_item.as_ref() else {
                    log::warn!(
                        "[wechat-api] file item without file_item body, msg_id={msg_id_for_log}"
                    );
                    continue;
                };
                let media = match file.media.as_ref() {
                    Some(m) if !m.full_url.is_empty() && !m.aes_key.is_empty() => m,
                    _ => {
                        log::warn!(
                            "[wechat-api] file item missing media.full_url or aes_key, msg_id={msg_id_for_log}"
                        );
                        continue;
                    }
                };
                let file_name = file
                    .file_name
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("wechat-file-{msg_id_for_log}.bin"));
                out.push(ChannelAttachmentSpec {
                    kind: AttachmentKind::File,
                    download_code: format!("wechat://{}@{}", media.aes_key, media.full_url),
                    file_name,
                });
            }
            // type=1 (text) 在 flatten_item_list_to_text 里处理；
            // type=3 (voice) / type=5 (video) 本期 out-of-scope，跳过。
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_item(text: &str) -> MessageItem {
        MessageItem {
            r#type: 1,
            text_item: Some(TextItem { text: text.into() }),
            ..Default::default()
        }
    }

    #[test]
    fn flatten_text_items_concatenates_with_newline() {
        let items = vec![text_item("hello"), text_item("world")];
        assert_eq!(flatten_item_list_to_text(&items), "hello\nworld");
    }

    #[test]
    fn flatten_non_text_emits_placeholder_so_llm_content_never_empty() {
        let items = vec![MessageItem {
            r#type: 2,
            ..Default::default()
        }];
        assert_eq!(flatten_item_list_to_text(&items), "[图片]");
    }

    #[test]
    fn flatten_empty_text_skipped_no_blank_lines() {
        let items = vec![text_item(""), text_item("ok")];
        assert_eq!(flatten_item_list_to_text(&items), "ok");
    }

    #[test]
    fn build_text_send_req_skips_item_list_when_empty() {
        let req = build_text_send_req("uid", "", "cid-1", Some("ctx"), "0.5.30");
        let json = serde_json::to_value(&req).unwrap();
        assert!(json["msg"]["item_list"].is_null());
        assert_eq!(json["msg"]["context_token"], "ctx");
    }

    #[test]
    fn build_text_send_req_includes_context_token() {
        let req = build_text_send_req("uid", "hi", "cid-1", Some("ctx-abc"), "0.5.30");
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["msg"]["context_token"], "ctx-abc");
        assert_eq!(json["msg"]["item_list"][0]["text_item"]["text"], "hi");
        assert_eq!(json["msg"]["message_type"], 2);
        assert_eq!(json["msg"]["message_state"], 2);
    }

    #[test]
    fn parse_get_updates_response_with_message() {
        let raw = r#"{
            "ret":0,
            "get_updates_buf":"NEXT",
            "longpolling_timeout_ms":35000,
            "msgs":[{
                "message_id":42,
                "from_user_id":"wxid_alice",
                "message_type":1,
                "message_state":2,
                "context_token":"ctx-1",
                "create_time_ms":1779180000000,
                "item_list":[{"type":1,"text_item":{"text":"你好"}}]
            }]
        }"#;
        let r: GetUpdatesResp = serde_json::from_str(raw).unwrap();
        assert_eq!(r.ret, 0);
        assert_eq!(r.get_updates_buf.as_deref(), Some("NEXT"));
        assert_eq!(r.msgs.len(), 1);
        let m = &r.msgs[0];
        assert_eq!(m.from_user_id.as_deref(), Some("wxid_alice"));
        assert_eq!(m.context_token.as_deref(), Some("ctx-1"));
        assert_eq!(flatten_item_list_to_text(&m.item_list), "你好");
    }

    /// 端到端反序列化一段真实 iLink 图片消息（从 phase 5 raw body log 抓的）。
    /// 验证 ImageItem + CdnMedia + extract_attachments 全路径打通。
    #[test]
    fn parse_real_image_message_extracts_attachment() {
        let raw = r#"{
            "ret":0,
            "msgs":[{
                "message_id":7462500663980633224,
                "from_user_id":"o9cq80_fth4BcOU2z8JnWQt1OyoE@im.wechat",
                "to_user_id":"3b6c1f1b8901@im.bot",
                "create_time_ms":1779198804723,
                "message_type":1,
                "message_state":2,
                "item_list":[{
                    "type":2,
                    "is_completed":true,
                    "image_item":{
                        "aeskey":"7716ac836c2fc1faae223956cff3dbf7",
                        "media":{
                            "encrypt_query_param":"abc",
                            "aes_key":"NzcxNmFjODM2YzJmYzFmYWFlMjIzOTU2Y2ZmM2RiZjc=",
                            "full_url":"https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=abc"
                        },
                        "mid_size":35447
                    }
                }],
                "context_token":"ctx-img"
            }]
        }"#;
        let r: GetUpdatesResp = serde_json::from_str(raw).unwrap();
        assert_eq!(r.msgs.len(), 1);
        let m = &r.msgs[0];
        let attachments = extract_attachments_from_item_list(&m.item_list, "test");
        assert_eq!(attachments.len(), 1);
        let att = &attachments[0];
        assert!(matches!(
            att.kind,
            crate::connector::im::types::AttachmentKind::Picture
        ));
        // 优先使用 image_item.aeskey（hex），通过 hex_aeskey_to_base64 编码成 base64(hex)
        assert!(
            att.download_code
                .starts_with("wechat://NzcxNmFjODM2YzJmYzFmYWFlMjIzOTU2Y2ZmM2RiZjc=@"),
            "expected base64(hex) prefix, got: {}",
            att.download_code
        );
        assert!(att.download_code.contains("novac2c.cdn.weixin.qq.com"));
    }

    #[test]
    fn parse_real_file_message_extracts_attachment() {
        let raw = r#"{
            "ret":0,
            "msgs":[{
                "message_id":7462500638282099720,
                "from_user_id":"o9cq80_fth4BcOU2z8JnWQt1OyoE@im.wechat",
                "to_user_id":"3b6c1f1b8901@im.bot",
                "create_time_ms":1779198798594,
                "message_type":1,
                "message_state":2,
                "item_list":[{
                    "type":4,
                    "is_completed":true,
                    "file_item":{
                        "media":{
                            "encrypt_query_param":"xyz",
                            "aes_key":"Y2MwYjIzODNiYzgwNjVjZWQ0YWRjNWFmNDI0ZmEwMmE=",
                            "full_url":"https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=xyz"
                        },
                        "file_name":"王瀚坤简历.pdf",
                        "md5":"a6ce2ac63250777505eed4bf64a00770",
                        "len":"518728"
                    }
                }]
            }]
        }"#;
        let r: GetUpdatesResp = serde_json::from_str(raw).unwrap();
        let m = &r.msgs[0];
        let attachments = extract_attachments_from_item_list(&m.item_list, "test");
        assert_eq!(attachments.len(), 1);
        let att = &attachments[0];
        assert!(matches!(
            att.kind,
            crate::connector::im::types::AttachmentKind::File
        ));
        assert_eq!(att.file_name, "王瀚坤简历.pdf");
        assert!(att
            .download_code
            .starts_with("wechat://Y2MwYjIzODNiYzgwNjVjZWQ0YWRjNWFmNDI0ZmEwMmE=@"));
    }

    #[test]
    fn extract_attachments_skips_incomplete_items() {
        let item = MessageItem {
            r#type: 2,
            is_completed: Some(false),
            image_item: Some(ImageItem {
                aeskey: Some("00".repeat(16)),
                media: Some(CdnMedia {
                    full_url: "https://example.com".into(),
                    aes_key: "k".into(),
                    encrypt_query_param: None,
                }),
            }),
            ..Default::default()
        };
        let attachments = extract_attachments_from_item_list(&[item], "test");
        assert!(attachments.is_empty());
    }

    #[test]
    fn extract_attachments_falls_back_to_media_aes_key_when_image_aeskey_missing() {
        let item = MessageItem {
            r#type: 2,
            image_item: Some(ImageItem {
                aeskey: None,
                media: Some(CdnMedia {
                    full_url: "https://example.com/full".into(),
                    aes_key: "ALREADY_BASE64".into(),
                    encrypt_query_param: None,
                }),
            }),
            ..Default::default()
        };
        let attachments = extract_attachments_from_item_list(&[item], "msg-x");
        assert_eq!(attachments.len(), 1);
        // 用 media.aes_key 时不经过 hex_aeskey_to_base64 二次编码
        assert!(attachments[0]
            .download_code
            .starts_with("wechat://ALREADY_BASE64@"));
    }
}
