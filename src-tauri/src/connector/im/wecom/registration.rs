//! 企微智能机器人扫码注册流程。
//!
//! 协议来源：企微团队官方 CLI `@wecom/wecom-openclaw-cli`（npm 包 1.1.0
//! 解包后的 `dist/utils/qrcode.js`）。企微 developer center 暂未公开此协议
//! 的正式文档，所以这里保留对 source / endpoint / 字段名的硬编码注释，
//! 方便未来协议变更时反查。
//!
//! 流程：
//! 1. begin：GET `/ai/qc/generate?source=wecom-cli&plat={1|2|3}`
//!    返回 `{ data: { scode, auth_url } }`，scode 是后续 polling 的 session 标识，
//!    auth_url 是给企业微信 App 扫的二维码内容。
//! 2. poll：GET `/ai/qc/query_result?scode=<scode>` 每 3 秒一次，5 分钟超时。
//!    成功时返回 `{ data: { status: "success", bot_info: { botid, secret } } }`。
//!    其他 status 一律视为 Waiting（官方 CLI 也是这么写的——除 success 外不区分）。
//!
//! 风险点：source 参数当前 hardcode `wecom-cli`，企微后端按 source 字符串识别。
//! 改成自定义 source（如 `aijia`）可能被白名单拒绝；先沿用 wecom-cli 最稳妥。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const QR_GENERATE_URL: &str = "https://work.weixin.qq.com/ai/qc/generate";
const QR_QUERY_URL: &str = "https://work.weixin.qq.com/ai/qc/query_result";
/// 浏览器兜底页：用户在桌面 app 上看不到二维码时（webview 渲染失败 / 屏幕太小）
/// 把这个 URL 用系统浏览器打开就能看到。
const QR_FALLBACK_PAGE_PREFIX: &str = "https://work.weixin.qq.com/ai/qc/gen";

/// 沿用官方 CLI 的 source 标识。改成自定义值可能触发企微后端拒绝。
const QR_SOURCE: &str = "wecom-cli";
/// 用来填到 ChannelConfigView.source 的标识，与飞书 / 钉钉对齐。
pub const WECOM_QR_SOURCE: &str = "WECOM_QR_SCAN";

const DEFAULT_INTERVAL_SECONDS: u64 = 3;
const DEFAULT_EXPIRES_IN_SECONDS: u64 = 300; // 5 分钟，官方 CLI 同款

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WecomPollState {
    Waiting,
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WecomBeginResult {
    /// polling 用的 session 标识。
    pub scode: String,
    /// 给企业微信 App 扫的二维码内容（短链）。前端用 qrcode 库渲染。
    pub auth_url: String,
    /// 浏览器兜底页面：用户可以打开这个 URL 在网页里看到等价二维码。
    pub fallback_url: String,
    pub interval_seconds: u64,
    pub expires_in_seconds: u64,
    /// 给 ChannelConfigView.source 用，区分凭证来源。
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WecomPollResult {
    pub state: WecomPollState,
    pub bot_id: Option<String>,
    pub secret: Option<String>,
}

/// `plat` 值由企微 CLI 源码硬编码：darwin=1 / win32=2 / linux=3。其他平台传 0。
fn current_plat_code() -> u8 {
    if cfg!(target_os = "macos") {
        1
    } else if cfg!(target_os = "windows") {
        2
    } else if cfg!(target_os = "linux") {
        3
    } else {
        0
    }
}

pub async fn begin_registration() -> Result<WecomBeginResult> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}?source={}&plat={}",
        QR_GENERATE_URL,
        QR_SOURCE,
        current_plat_code()
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to GET wecom qr generate endpoint")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("wecom qr generate failed: {} {}", status, body);
    }

    #[derive(Deserialize)]
    struct Outer {
        data: Option<Inner>,
    }
    #[derive(Deserialize)]
    struct Inner {
        scode: String,
        auth_url: String,
    }
    let parsed: Outer = resp
        .json()
        .await
        .context("parse wecom qr generate response")?;
    let inner = parsed
        .data
        .ok_or_else(|| anyhow::anyhow!("wecom qr generate: missing data field"))?;

    let fallback_url = format!(
        "{}?source={}&scode={}",
        QR_FALLBACK_PAGE_PREFIX, QR_SOURCE, inner.scode
    );
    Ok(WecomBeginResult {
        scode: inner.scode,
        auth_url: inner.auth_url,
        fallback_url,
        interval_seconds: DEFAULT_INTERVAL_SECONDS,
        expires_in_seconds: DEFAULT_EXPIRES_IN_SECONDS,
        source: WECOM_QR_SOURCE.into(),
    })
}

pub async fn poll_registration(scode: &str) -> Result<WecomPollResult> {
    let client = reqwest::Client::new();
    let url = format!("{}?scode={}", QR_QUERY_URL, urlencoding::encode(scode));
    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to GET wecom qr query_result endpoint")?;
    if !resp.status().is_success() {
        // HTTP 层错误不直接 fail；保持 Waiting 让前端按 interval 继续轮询，
        // 避免临时网络抖动让用户重新扫码。如果是持续性错误（如 scode 过期被服务端拒），
        // 5 分钟到期后前端会自己 bail out。
        log::warn!(
            "[channel/wecom] poll_registration non-success http: {}",
            resp.status()
        );
        return Ok(WecomPollResult {
            state: WecomPollState::Waiting,
            bot_id: None,
            secret: None,
        });
    }

    #[derive(Deserialize)]
    struct Outer {
        data: Option<Inner>,
    }
    #[derive(Deserialize)]
    struct Inner {
        #[serde(default)]
        status: String,
        #[serde(default)]
        bot_info: Option<BotInfo>,
    }
    #[derive(Deserialize)]
    struct BotInfo {
        #[serde(default)]
        botid: String,
        #[serde(default)]
        secret: String,
    }

    let raw = resp
        .text()
        .await
        .context("read wecom qr poll response body")?;
    let parsed: Outer = serde_json::from_str(&raw)
        .with_context(|| format!("parse wecom qr poll response: {}", raw))?;
    let Some(inner) = parsed.data else {
        return Ok(WecomPollResult {
            state: WecomPollState::Waiting,
            bot_id: None,
            secret: None,
        });
    };

    if inner.status != "success" {
        // 官方 CLI 把所有非 success 都当 waiting；这里照办。
        return Ok(WecomPollResult {
            state: WecomPollState::Waiting,
            bot_id: None,
            secret: None,
        });
    }
    let bot_info = inner
        .bot_info
        .ok_or_else(|| anyhow::anyhow!("wecom qr poll: status=success but bot_info missing"))?;
    if bot_info.botid.is_empty() || bot_info.secret.is_empty() {
        anyhow::bail!("wecom qr poll: bot_info has empty botid or secret");
    }
    Ok(WecomPollResult {
        state: WecomPollState::Success,
        bot_id: Some(bot_info.botid),
        secret: Some(bot_info.secret),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plat_code_covers_three_desktop_platforms() {
        let code = current_plat_code();
        // 确保至少落在已知三个平台之一（CI 上目前是 mac/linux/windows）
        assert!(matches!(code, 1 | 2 | 3));
    }

    #[test]
    fn begin_result_serialization_is_camel_case() {
        let r = WecomBeginResult {
            scode: "s1".into(),
            auth_url: "https://x".into(),
            fallback_url: "https://y".into(),
            interval_seconds: 3,
            expires_in_seconds: 300,
            source: WECOM_QR_SOURCE.into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"authUrl\""), "expected camelCase: {s}");
        assert!(s.contains("\"intervalSeconds\""), "expected camelCase: {s}");
        assert!(s.contains("\"fallbackUrl\""), "expected camelCase: {s}");
    }

    #[test]
    fn poll_result_with_bot_info_serializes_camel_case() {
        let r = WecomPollResult {
            state: WecomPollState::Success,
            bot_id: Some("bot-1".into()),
            secret: Some("sec-1".into()),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"botId\""), "expected camelCase: {s}");
        // state should be lowercase camelCase variant
        assert!(s.contains("\"success\""), "expected camelCase variant: {s}");
    }
}
