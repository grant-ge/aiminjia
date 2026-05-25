//! Feishu tenant_access_token source. Wrapped by SharedTokenCache<FeishuTokenSource>.
//! Endpoint: POST /open-apis/auth/v3/tenant_access_token/internal returns expire=7200.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;

use crate::connector::im::shared::token::PlatformTokenSource;

const FEISHU_API: &str = "https://open.feishu.cn";

pub struct FeishuTokenSource {
    app_id: String,
    app_secret: String,
    api_base: String,
    http: reqwest::Client,
}

impl FeishuTokenSource {
    pub fn new(app_id: String, app_secret: String) -> Self {
        Self {
            app_id,
            app_secret,
            api_base: FEISHU_API.to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Test-only constructor that points the token endpoint at a mock server.
    /// Hidden from production callers; used by `feishu::download` tests so
    /// they can hermetically satisfy BOTH the token POST and the resource GET
    /// against the same wiremock server. Constant FEISHU_API is otherwise
    /// hard-wired in `new`.
    #[doc(hidden)]
    pub fn new_with_api_base_for_tests(
        app_id: String,
        app_secret: String,
        api_base: String,
    ) -> Self {
        Self {
            app_id,
            app_secret,
            api_base,
            http: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl PlatformTokenSource for FeishuTokenSource {
    async fn fetch(&self) -> Result<(String, u64)> {
        #[derive(Deserialize)]
        struct Resp {
            tenant_access_token: String,
            expire: u64,
            code: i64,
            msg: Option<String>,
        }
        let resp = self
            .http
            .post(format!(
                "{}/open-apis/auth/v3/tenant_access_token/internal",
                self.api_base
            ))
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .context("Failed to request feishu tenant_access_token")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("feishu tenant_access_token http: {} {}", status, body);
        }
        let r: Resp = resp.json().await.context("parse feishu token resp")?;
        if r.code != 0 {
            anyhow::bail!(
                "feishu tenant_access_token errcode={} msg={}",
                r.code,
                r.msg.unwrap_or_default()
            );
        }
        Ok((r.tenant_access_token, r.expire))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_stores_credentials() {
        let s = FeishuTokenSource::new("ak".into(), "as".into());
        assert_eq!(s.app_id, "ak");
        assert_eq!(s.app_secret, "as");
    }
}
