//! HTTP client for the Lotus tenant portal API.
//!
//! Base URL: `https://ai-tenant.renlijia.com`
//!
//! Endpoints:
//! - POST /api/auth/login       — username/password → JWT tokens
//! - POST /api/auth/refresh     — refresh_token → new JWT tokens
//! - POST /api/session-keys     — access_token → session key (sk-sess***)
//! - GET  /v1/models            — list available models

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use super::state::{CloudModelInfo, TenantInfo, UserInfo};

const BASE_URL: &str = "https://ai-tenant.renlijia.com";

/// Raw login/refresh response from the API (snake_case fields).
#[derive(Debug, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// Absolute expiration timestamp for access token.
    pub access_expires_at: DateTime<Utc>,
    /// Absolute expiration timestamp for refresh token.
    pub refresh_expires_at: DateTime<Utc>,
    pub user: AuthUserInfo,
    pub tenant: AuthTenantInfo,
}

/// User info as returned by the login/refresh API (snake_case, superset of fields).
#[derive(Debug, Deserialize)]
pub struct AuthUserInfo {
    pub id: i64,
    pub name: String,
    pub username: String,
}

/// Tenant info as returned by the login/refresh API (snake_case, superset of fields).
#[derive(Debug, Deserialize)]
pub struct AuthTenantInfo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub balance: String,
}

impl From<AuthUserInfo> for UserInfo {
    fn from(u: AuthUserInfo) -> Self {
        Self { id: u.id, name: u.name, username: u.username }
    }
}

impl From<AuthTenantInfo> for TenantInfo {
    fn from(t: AuthTenantInfo) -> Self {
        Self { id: t.id, name: t.name, balance: t.balance }
    }
}

/// Raw session key response from the API.
/// Server returns: { "key": "sk-sess...", "expires_at": "2026-03-05T..." }
#[derive(Debug, Deserialize)]
pub struct SessionKeyResponse {
    pub key: String,
    pub expires_at: DateTime<Utc>,
}

/// HTTP client for Lotus tenant portal.
pub struct AuthClient {
    client: Client,
}

impl AuthClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Login with username and password.
    pub async fn login(&self, username: &str, password: &str) -> Result<AuthResponse> {
        let url = format!("{}/api/auth/login", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .json(&json!({ "method": "username", "username": username, "password": password }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }

        resp.json::<AuthResponse>()
            .await
            .map_err(|e| anyhow!("服务器响应格式异常: {}", e))
    }

    /// Refresh access token using refresh token.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<AuthResponse> {
        let url = format!("{}/api/auth/refresh", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .json(&json!({ "refresh_token": refresh_token }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }

        resp.json::<AuthResponse>()
            .await
            .map_err(|e| anyhow!("服务器响应格式异常: {}", e))
    }

    /// Create a session key for API access.
    pub async fn create_session_key(&self, access_token: &str) -> Result<SessionKeyResponse> {
        let url = format!("{}/api/session-keys", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }

        resp.json::<SessionKeyResponse>()
            .await
            .map_err(|e| anyhow!("服务器响应格式异常: {}", e))
    }

    /// List available models from the server.
    pub async fn list_models(&self, session_key: &str) -> Result<Vec<CloudModelInfo>> {
        let url = format!("{}/v1/models", BASE_URL);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", session_key))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }

        // OpenAI-compatible /v1/models response: { data: [{ id, type, ... }] }
        let body: serde_json::Value = resp.json().await?;
        let models = body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|m| CloudModelInfo {
                        id: m["id"].as_str().unwrap_or("").to_string(),
                        name: m["id"].as_str().unwrap_or("").to_string(),
                        model_type: m["type"].as_str().unwrap_or("chat").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }
}

/// Parse API error body into a user-friendly Chinese error message.
fn parse_api_error(status: u16, body: &str) -> anyhow::Error {
    // Try to parse as JSON { "code": int, "message": "..." }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = json["error"]["message"].as_str().or(json["message"].as_str()) {
            return anyhow!("{}", localize_error(msg));
        }
    }

    match status {
        401 => anyhow!("用户名或密码错误"),
        403 => anyhow!("账户已被禁用"),
        429 => anyhow!("请求过于频繁，请稍后再试"),
        502 | 503 | 504 => anyhow!("服务器暂时不可用，请稍后重试"),
        500..=599 => anyhow!("服务器内部错误 ({})", status),
        _ => anyhow!("请求失败 ({})", status),
    }
}

/// Translate known English server messages to Chinese.
fn localize_error(msg: &str) -> &str {
    match msg {
        "Invalid credentials" | "user not found" => "用户名或密码错误",
        "Account is frozen" | "Account is disabled" => "账户已被冻结，请联系管理员",
        "Tenant is suspended" => "企业账户已被停用，请联系管理员",
        "Tenant not found" => "企业不存在，请检查用户名中的企业编码",
        "Too many failed attempts, please try again later" => "登录尝试过多，请稍后再试",
        "Token expired" | "Invalid token" => "登录已过期，请重新登录",
        "Insufficient balance" => "账户余额不足，请联系管理员充值",
        "Rate limit exceeded" => "请求过于频繁，请稍后再试",
        _ => msg,
    }
}
