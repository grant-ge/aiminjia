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
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use super::state::{CloudModelInfo, TenantInfo, UserInfo};

const BASE_URL: &str = "https://ai-tenant.renlijia.com";

/// Raw login/refresh response from the API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// Token TTL in seconds (e.g. 3600 for 1 hour).
    pub expires_in: i64,
    /// Refresh token TTL in seconds.
    pub refresh_expires_in: i64,
    pub user: UserInfo,
    pub tenant: TenantInfo,
}

/// Raw session key response from the API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionKeyResponse {
    pub session_key: String,
    /// TTL in seconds (e.g. 86400 for 24 hours).
    pub expires_in: i64,
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
            .map_err(|e| anyhow!("Failed to parse login response: {}", e))
    }

    /// Refresh access token using refresh token.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<AuthResponse> {
        let url = format!("{}/api/auth/refresh", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .json(&json!({ "refreshToken": refresh_token }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_api_error(status.as_u16(), &body));
        }

        resp.json::<AuthResponse>()
            .await
            .map_err(|e| anyhow!("Failed to parse refresh response: {}", e))
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
            .map_err(|e| anyhow!("Failed to parse session key response: {}", e))
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

        // OpenAI-compatible /v1/models response: { data: [{ id, ... }] }
        let body: serde_json::Value = resp.json().await?;
        let models = body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|m| CloudModelInfo {
                        id: m["id"].as_str().unwrap_or("").to_string(),
                        name: m["id"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }
}

/// Parse API error body into a user-friendly error message.
fn parse_api_error(status: u16, body: &str) -> anyhow::Error {
    // Try to parse as JSON { "error": { "message": "..." } } or { "message": "..." }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = json["error"]["message"].as_str() {
            return anyhow!("{}", msg);
        }
        if let Some(msg) = json["message"].as_str() {
            return anyhow!("{}", msg);
        }
    }

    match status {
        401 => anyhow!("用户名或密码错误"),
        403 => anyhow!("账户已被禁用"),
        429 => anyhow!("请求过于频繁，请稍后再试"),
        _ => anyhow!("服务器错误 ({}): {}", status, body),
    }
}
