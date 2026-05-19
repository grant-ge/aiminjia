//! HTTP client for the Lotus API Gateway.
//!
//! Base URL: `https://ai-tenant.renlijia.com`
//!
//! Endpoints:
//! - POST /auth/login           — username/password → JWT tokens
//! - POST /auth/refresh         — refresh_token → new JWT tokens
//! - PUT  /auth/password        — change password (JWT required)
//! - POST /auth/logout          — logout (JWT required)
//! - POST /auth/session-keys    — access_token → session key (sk-sess***)
//! - GET  /v1/models            — list available models

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use super::state::{CloudModelInfo, TenantInfo, UserInfo};

pub const BASE_URL: &str = "https://ai-tenant.renlijia.com";

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
    #[serde(default)]
    pub username: Option<String>,
}

/// Tenant info as returned by the login/refresh API (snake_case, superset of fields).
#[derive(Debug, Deserialize)]
pub struct AuthTenantInfo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub balance: String,
    #[serde(default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub logo_url: Option<String>,
    #[serde(default)]
    pub accent_color: Option<String>,
    #[serde(default)]
    pub primary_color: Option<String>,
    #[serde(default)]
    pub bg_color: Option<String>,
    #[serde(default)]
    pub sidebar_bg_color: Option<String>,
    #[serde(default)]
    pub font_family: Option<String>,
}

impl From<AuthUserInfo> for UserInfo {
    fn from(u: AuthUserInfo) -> Self {
        Self {
            id: u.id,
            name: u.name,
            username: u.username.unwrap_or_default(),
        }
    }
}

impl From<AuthTenantInfo> for TenantInfo {
    fn from(t: AuthTenantInfo) -> Self {
        Self {
            id: t.id,
            name: t.name,
            balance: t.balance,
            product_name: t.product_name,
            logo_url: t.logo_url,
            accent_color: t.accent_color,
            primary_color: t.primary_color,
            bg_color: t.bg_color,
            sidebar_bg_color: t.sidebar_bg_color,
            font_family: t.font_family,
        }
    }
}

/// Raw session key response from the API.
/// Server returns: { "key": "sk-sess...", "expires_at": "2026-03-05T...", "ttl_seconds": 86400 }
#[derive(Debug, Deserialize)]
pub struct SessionKeyResponse {
    pub key: String,
    pub expires_at: DateTime<Utc>,
    /// Server-computed seconds-until-expiry. Preferred over `expires_at`
    /// because it avoids any clock-skew / timezone interpretation issues
    /// between server and client. `None` for legacy servers that don't
    /// emit this field yet — caller falls back to `expires_at`.
    #[serde(default)]
    pub ttl_seconds: Option<i64>,
}

impl SessionKeyResponse {
    /// Compute the effective expiry instant the client should cache.
    /// Prefers `ttl_seconds + now` (zero ambiguity) over the wallclock
    /// `expires_at` (subject to server/client time skew).
    pub fn effective_expires_at(&self) -> DateTime<Utc> {
        if let Some(ttl) = self.ttl_seconds.filter(|t| *t > 0) {
            chrono::Utc::now() + chrono::Duration::seconds(ttl)
        } else {
            self.expires_at
        }
    }
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
        let url = format!("{}/auth/login", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .json(&json!({ "username": username, "password": password }))
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
        let url = format!("{}/auth/refresh", BASE_URL);
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

    /// Create a session key for API access. `device_id` is a stable
    /// per-installation identifier; server-side it dedupes active session
    /// keys by `(user_id, device_id)` so the same desktop install opening
    /// the app N times only contributes one slot to the 10-active-key cap.
    pub async fn create_session_key(
        &self,
        access_token: &str,
        device_id: Option<&str>,
    ) -> Result<SessionKeyResponse> {
        let url = format!("{}/auth/session-keys", BASE_URL);
        let mut req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token));
        if let Some(did) = device_id.filter(|s| !s.is_empty()) {
            req = req.json(&serde_json::json!({ "device_id": did }));
        }
        let resp = req.send().await?;

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
    ///
    /// Phase C: hits `/anthropic/v1/models` (NOT the OpenAI-shape
    /// `/v1/models`) so the returned set is exactly the models reachable
    /// over the anthropic ingress that `LotusProvider` now uses. Returning
    /// the OpenAI list would let the desktop UI pick a model that has no
    /// anthropic-protocol route, then fail with `5001 no_route` on first
    /// real request.
    ///
    /// The anthropic response shape is
    ///   `{ data: [{ type: "model", id, display_name, created_at }], has_more, ... }`
    /// — different from OpenAI's `{ data: [{ id, type: "chat" | "reasoner", ... }] }`.
    /// We map it back into our own `CloudModelInfo`. The `model_type`
    /// distinction (chat / reasoner) used to pick a gateway endpoint
    /// in the OpenAI-ingress era; under anthropic ingress everything
    /// goes to a single endpoint, so all entries get `model_type =
    /// "chat"` as a future-proof default.
    pub async fn list_models(&self, session_key: &str) -> Result<Vec<CloudModelInfo>> {
        let url = format!("{}/anthropic/v1/models", BASE_URL);
        log::info!(
            "[list_models] GET {} session_key_len={}",
            url,
            session_key.len()
        );
        let resp = self
            .client
            .get(&url)
            .header("x-api-key", session_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await?;

        let status = resp.status();
        let request_id = resp
            .headers()
            .get("x-request-id")
            .or_else(|| resp.headers().get("lotus-request-id"))
            .or_else(|| resp.headers().get("x-lotus-request-id"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();

        log::info!("[list_models] status={} request_id={}", status, request_id);

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            log::error!(
                "[list_models] failed: status={} request_id={} body={}",
                status,
                request_id,
                body
            );
            return Err(parse_api_error(status.as_u16(), &body));
        }

        // Anthropic `/v1/models` shape:
        //   { "data": [{"type":"model","id":"claude-...","display_name":"...","created_at":"..."}], "has_more": false, ... }
        let body: serde_json::Value = resp.json().await?;
        let models = body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|m| {
                        let id = m["id"].as_str().unwrap_or("").to_string();
                        let display = m["display_name"].as_str().unwrap_or("").to_string();
                        let name = if display.is_empty() { id.clone() } else { display };
                        CloudModelInfo {
                            id,
                            name,
                            model_type: "chat".to_string(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    /// Change password on the server.
    pub async fn change_password(
        &self,
        access_token: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<()> {
        let url = format!("{}/auth/password", BASE_URL);
        let resp = self
            .client
            .put(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .json(&json!({ "old_password": old_password, "new_password": new_password }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }

        Ok(())
    }

    /// Logout from the server (revoke all refresh tokens).
    pub async fn logout(&self, access_token: &str) -> Result<()> {
        let url = format!("{}/auth/logout", BASE_URL);
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

        Ok(())
    }

    /// Request a verification code via SMS for registration.
    /// Hits tenant-portal `/api/auth/send-code`.
    pub async fn send_sms_code(&self, phone: &str) -> Result<()> {
        let url = format!("{}/api/auth/send-code", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .json(&json!({ "phone": phone }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }
        Ok(())
    }

    /// Request a verification code via email for registration.
    /// Hits tenant-portal `/api/auth/send-email-code`.
    pub async fn send_email_code(&self, email: &str) -> Result<()> {
        let url = format!("{}/api/auth/send-email-code", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .json(&json!({ "email": email }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }
        Ok(())
    }

    /// Register a new personal account via phone or email.
    /// Hits tenant-portal `/api/auth/register`.
    ///
    /// `method` must be `"phone"` or `"email"`. The matching identifier
    /// (phone or email) must be supplied; the other can be empty.
    /// `name` is the optional display name shown to the user post-login.
    pub async fn register(
        &self,
        method: &str,
        phone: &str,
        email: &str,
        code: &str,
        password: &str,
        name: &str,
    ) -> Result<()> {
        let url = format!("{}/api/auth/register", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .json(&json!({
                "method": method,
                "phone": phone,
                "email": email,
                "code": code,
                "password": password,
                "name": name,
            }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }
        Ok(())
    }

    /// Get current user + tenant profile (including latest branding).
    /// Uses session_key auth (Bearer), no token rotation.
    pub async fn get_profile(&self, session_key: &str) -> Result<(AuthUserInfo, AuthTenantInfo)> {
        let url = format!("{}/v1/profile", BASE_URL);
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

        let body: serde_json::Value = resp.json().await?;
        let user: AuthUserInfo = serde_json::from_value(body["user"].clone())
            .map_err(|e| anyhow!("Failed to parse user profile: {}", e))?;
        let tenant: AuthTenantInfo = serde_json::from_value(body["tenant"].clone())
            .map_err(|e| anyhow!("Failed to parse tenant profile: {}", e))?;
        Ok((user, tenant))
    }
}

/// Parse API error body into a user-friendly Chinese error message.
fn parse_api_error(status: u16, body: &str) -> anyhow::Error {
    // Try to parse as JSON { "code": int, "message": "..." }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = json["error"]["message"]
            .as_str()
            .or(json["message"].as_str())
        {
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
        "Password must be at least 8 characters" => "密码长度至少 8 个字符",
        _ => msg,
    }
}
