//! 飞书 OAuth 2.0 Device Authorization Grant 注册流（RFC 8628）。
//!
//! 单端点 begin + poll：
//!   POST https://accounts.feishu.cn/oauth/v1/app/registration
//!   Content-Type: application/x-www-form-urlencoded
//!
//! 错误模型走 RFC 8628 字符串（authorization_pending / slow_down / access_denied /
//! expired_token），HTTP 4xx body 里 `error` 字段返回；NOT 数字 errcode。
//!
//! 字段命名差异：device-flow 响应给的是 `client_id` / `client_secret`（RFC 8628 标准名），
//! tenant_access_token 端点要的是 `app_id` / `app_secret`——同一对值，不同字段名。
//! 持久化到磁盘的 schema 用 `app_id`（见 FeishuStoredCredentials），manager poll handler
//! 负责把 client_id / client_secret 映射到 app_id / app_secret。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const REGISTRATION_URL: &str = "https://accounts.feishu.cn/oauth/v1/app/registration";
pub const FEISHU_DEVICE_CODE_SOURCE: &str = "FEISHU_DEVICE_CODE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeishuPollState {
    Waiting,
    Success,
    Fail,
    Expired,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuBeginResult {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri_complete: String,
    pub verification_uri: String,
    pub interval_seconds: u64,
    pub expires_in_seconds: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuPollResult {
    pub state: FeishuPollState,
    /// RFC 8628 `client_id` — 等价于 tenant_access_token 端点的 `app_id`。
    pub client_id: Option<String>,
    /// RFC 8628 `client_secret` — 等价于 tenant_access_token 端点的 `app_secret`。
    pub client_secret: Option<String>,
    pub fail_reason: Option<String>,
}

/// RFC 8628 字符串错误 → FeishuPollState 映射。
pub fn map_feishu_error(err: &str) -> FeishuPollState {
    match err {
        "authorization_pending" => FeishuPollState::Waiting,
        // `slow_down` 严格语义是"增加 polling 间隔 5s"。connector 层简化处理为 Waiting；
        // 调用方（manager / 前端）按 begin response 的 interval_seconds 节奏 polling 即可。
        "slow_down" => FeishuPollState::Waiting,
        "access_denied" => FeishuPollState::Fail,
        "expired_token" => FeishuPollState::Expired,
        _ => FeishuPollState::Unknown,
    }
}

pub async fn begin_registration() -> Result<FeishuBeginResult> {
    let client = reqwest::Client::new();
    let resp = client
        .post(REGISTRATION_URL)
        .form(&[
            ("action", "begin"),
            ("archetype", "PersonalAgent"),
            ("auth_method", "client_secret"),
            ("request_user_info", "open_id"),
        ])
        .send()
        .await
        .context("Failed to POST feishu app registration (begin)")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("feishu registration begin failed: {} {}", status, body);
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    struct Resp {
        device_code: String,
        user_code: String,
        verification_uri: String,
        verification_uri_complete: Option<String>,
        interval: u64,
        expires_in: u64,
    }
    let r: Resp = resp
        .json()
        .await
        .context("parse feishu registration begin resp")?;
    Ok(FeishuBeginResult {
        device_code: r.device_code,
        user_code: r.user_code.clone(),
        verification_uri: r.verification_uri.clone(),
        verification_uri_complete: r
            .verification_uri_complete
            .unwrap_or_else(|| format!("{}?user_code={}", r.verification_uri, r.user_code)),
        interval_seconds: r.interval,
        expires_in_seconds: r.expires_in,
        source: FEISHU_DEVICE_CODE_SOURCE.into(),
    })
}

pub async fn poll_registration(device_code: &str) -> Result<FeishuPollResult> {
    let client = reqwest::Client::new();
    let resp = client
        .post(REGISTRATION_URL)
        .form(&[("action", "poll"), ("device_code", device_code)])
        .send()
        .await
        .context("Failed to POST feishu app registration (poll)")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // RFC 8628: 中间状态/错误都以 HTTP 400 + body { "error": "...", "error_description": "..." } 返回
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
                let s = map_feishu_error(err);
                let reason = v
                    .get("error_description")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| Some(err.to_string()));
                return Ok(FeishuPollResult {
                    state: s,
                    client_id: None,
                    client_secret: None,
                    fail_reason: reason,
                });
            }
        }
        return Err(anyhow!(
            "feishu registration poll failed: {} {}",
            status,
            body
        ));
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    struct Ok2 {
        client_id: Option<String>,
        client_secret: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let r: Ok2 = serde_json::from_str(&body).context("parse feishu registration poll resp")?;
    // 同时也防御 200 + body 里带 error 的写法（非 RFC 标准但有些实现会这样）
    let state = match r.error.as_deref() {
        Some(err) => map_feishu_error(err),
        None if r.client_id.is_some() && r.client_secret.is_some() => FeishuPollState::Success,
        None => FeishuPollState::Unknown,
    };
    Ok(FeishuPollResult {
        state,
        client_id: r.client_id,
        client_secret: r.client_secret,
        fail_reason: r.error_description.or(r.error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_maps_known_rfc8628_codes() {
        assert_eq!(
            map_feishu_error("authorization_pending"),
            FeishuPollState::Waiting
        );
        assert_eq!(map_feishu_error("slow_down"), FeishuPollState::Waiting);
        assert_eq!(map_feishu_error("access_denied"), FeishuPollState::Fail);
        assert_eq!(map_feishu_error("expired_token"), FeishuPollState::Expired);
        assert_eq!(map_feishu_error("anything_else"), FeishuPollState::Unknown);
    }
}
