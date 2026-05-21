//! Scan-to-login state machine — mirrors openclaw-weixin-main/src/auth/login-qr.ts.
//!
//! Flow (spec §1):
//!   begin → fetch_qrcode → returns `qrcode` (poll handle) + `qrcode_img_content`
//!     (a URL string the frontend renders into a QR image client-side).
//!   poll  → get_qrcode_status long-poll → 5 wire states (wait, scaned,
//!     scaned_but_redirect, confirmed, expired). LoginSession::tick converts
//!     these into next-action signals for the caller.
//!
//! `scaned_but_redirect` switches the polling base_url to `redirect_host`.
//! `expired` auto-refreshes the QR up to MAX_QR_REFRESH_COUNT times.

use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

use super::endpoints::{DEFAULT_BASE_URL, DEFAULT_BOT_TYPE, GET_BOT_QRCODE, GET_QRCODE_STATUS};
use super::headers::{build_headers, HeaderInputs};

#[derive(Debug, Error)]
pub enum LoginError {
    #[error("network error: {0}")]
    Network(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrCodeResponse {
    pub qrcode: String,
    pub qrcode_img_content: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QrStatusResponse {
    /// "wait" / "scaned" / "scaned_but_redirect" / "confirmed" / "expired"
    pub status: String,
    #[serde(default)]
    pub bot_token: Option<String>,
    #[serde(default)]
    pub ilink_bot_id: Option<String>,
    #[serde(default)]
    pub baseurl: Option<String>,
    #[serde(default)]
    pub ilink_user_id: Option<String>,
    #[serde(default)]
    pub redirect_host: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfirmedLogin {
    pub bot_token: String,
    pub ilink_bot_id: String,
    pub ilink_user_id: String,
    /// Effective base URL after IDC redirect; subsequent business endpoints
    /// MUST hit this URL (not DEFAULT_BASE_URL).
    pub effective_base_url: String,
}

/// Server-side long-poll on `get_qrcode_status` typically returns within 35s
/// when status changes. We back off slightly for client-side timeout.
pub const QR_LONG_POLL_TIMEOUT_SECS: u64 = 35;
pub const MAX_QR_REFRESH_COUNT: u32 = 3;

/// `fetch_qrcode` — GET `ilink/bot/get_bot_qrcode?bot_type=3`.
pub async fn fetch_qrcode(
    client: &reqwest::Client,
    app_id: &str,
    client_version: &str,
    base_url: &str,
) -> Result<QrCodeResponse, LoginError> {
    let url = format!(
        "{}/{}?bot_type={}",
        base_url.trim_end_matches('/'),
        GET_BOT_QRCODE,
        DEFAULT_BOT_TYPE
    );
    let headers = build_headers(HeaderInputs {
        app_id,
        client_version,
        bot_token: None,
        route_tag: None,
    });
    log::info!("[wechat] fetch_qrcode GET {url}");
    let started = std::time::Instant::now();
    let resp = client
        .get(&url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| {
            log::warn!(
                "[wechat] fetch_qrcode network error after {:?}: {e}",
                started.elapsed()
            );
            LoginError::Network(e.to_string())
        })?;
    let status = resp.status();
    if !status.is_success() {
        log::warn!(
            "[wechat] fetch_qrcode HTTP {status} after {:?}",
            started.elapsed()
        );
        return Err(LoginError::InvalidResponse(format!(
            "get_bot_qrcode HTTP {status}"
        )));
    }
    let raw = resp
        .text()
        .await
        .map_err(|e| LoginError::Network(e.to_string()))?;
    log::info!(
        "[wechat] fetch_qrcode OK http={status} body_len={} elapsed={:?}",
        raw.len(),
        started.elapsed()
    );
    serde_json::from_str(&raw).map_err(|e| LoginError::InvalidResponse(format!("{e}; raw={raw}")))
}

/// `poll_qr_status` — GET `ilink/bot/get_qrcode_status?qrcode=<x>`.
/// Treats client-side timeout as `wait` so the caller can simply retry.
pub async fn poll_qr_status(
    client: &reqwest::Client,
    app_id: &str,
    client_version: &str,
    base_url: &str,
    qrcode: &str,
) -> Result<QrStatusResponse, LoginError> {
    let url = format!(
        "{}/{}?qrcode={}",
        base_url.trim_end_matches('/'),
        GET_QRCODE_STATUS,
        urlencoding::encode(qrcode)
    );
    let headers = build_headers(HeaderInputs {
        app_id,
        client_version,
        bot_token: None,
        route_tag: None,
    });
    let req = client
        .get(&url)
        .headers(headers)
        .timeout(Duration::from_secs(QR_LONG_POLL_TIMEOUT_SECS));
    match req.send().await {
        Ok(r) if r.status().is_success() => {
            let raw = r
                .text()
                .await
                .map_err(|e| LoginError::Network(e.to_string()))?;
            serde_json::from_str(&raw)
                .map_err(|e| LoginError::InvalidResponse(format!("{e}; raw={raw}")))
        }
        Ok(r) => Err(LoginError::InvalidResponse(format!(
            "get_qrcode_status HTTP {}",
            r.status()
        ))),
        Err(e) if e.is_timeout() => {
            // long-poll client-side timeout is normal — return wait so caller
            // can simply re-poll.
            Ok(QrStatusResponse {
                status: "wait".to_string(),
                ..QrStatusResponse::default()
            })
        }
        Err(e) => {
            // Treat network errors (incl. gateway 524) as transient wait —
            // openclaw plugin does the same so users don't see flaky errors.
            log::warn!("[wechat] poll_qr_status network error, retrying: {e}");
            Ok(QrStatusResponse {
                status: "wait".to_string(),
                ..QrStatusResponse::default()
            })
        }
    }
}

/// State machine output. Caller drives next action.
#[derive(Debug)]
pub enum LoginStep {
    KeepWaiting,
    Scanned,
    /// QR expired; caller should fetch a new one and pass it back via
    /// `LoginSession::apply_new_qr`. If `refresh_count` has hit
    /// `MAX_QR_REFRESH_COUNT`, returns `Failed` instead.
    NeedsQrRefresh,
    Confirmed(ConfirmedLogin),
    Failed(String),
}

pub struct LoginSession {
    qrcode: String,
    base_url: String,
    refresh_count: u32,
}

impl LoginSession {
    pub fn new(qrcode: String) -> Self {
        Self {
            qrcode,
            base_url: DEFAULT_BASE_URL.to_string(),
            refresh_count: 0,
        }
    }

    pub fn current_base_url(&self) -> &str {
        &self.base_url
    }

    pub fn current_qrcode(&self) -> &str {
        &self.qrcode
    }

    pub fn apply_new_qr(&mut self, new_qrcode: String) {
        self.qrcode = new_qrcode;
        self.refresh_count += 1;
    }

    /// Convert a poll response into the next state-machine step. Mutates
    /// `base_url` on `scaned_but_redirect`.
    pub fn tick(&mut self, resp: QrStatusResponse) -> LoginStep {
        match resp.status.as_str() {
            "wait" => LoginStep::KeepWaiting,
            "scaned" => LoginStep::Scanned,
            "scaned_but_redirect" => {
                if let Some(host) = resp.redirect_host.filter(|s| !s.is_empty()) {
                    self.base_url = format!("https://{host}");
                    log::info!("[wechat] IDC redirect → {}", self.base_url);
                }
                LoginStep::KeepWaiting
            }
            "expired" => {
                if self.refresh_count >= MAX_QR_REFRESH_COUNT {
                    LoginStep::Failed("登录超时：二维码已过期 3 次，请重新发起登录".into())
                } else {
                    LoginStep::NeedsQrRefresh
                }
            }
            "confirmed" => match (resp.bot_token, resp.ilink_bot_id, resp.ilink_user_id) {
                (Some(tk), Some(bot), Some(uid)) => LoginStep::Confirmed(ConfirmedLogin {
                    bot_token: tk,
                    ilink_bot_id: bot,
                    ilink_user_id: uid,
                    effective_base_url: resp
                        .baseurl
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| self.base_url.clone()),
                }),
                _ => LoginStep::Failed("登录失败：服务器返回 confirmed 但缺字段".into()),
            },
            other => LoginStep::Failed(format!("未知 QR 状态：{other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_qr_code_response() {
        let raw = r#"{"qrcode":"abc","qrcode_img_content":"https://ilink.weixin.qq.com/qr/xyz"}"#;
        let r: QrCodeResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(r.qrcode, "abc");
        assert_eq!(r.qrcode_img_content, "https://ilink.weixin.qq.com/qr/xyz");
    }

    #[test]
    fn deserialize_qr_status_wait() {
        let raw = r#"{"status":"wait"}"#;
        let s: QrStatusResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(s.status, "wait");
        assert!(s.bot_token.is_none());
    }

    #[test]
    fn deserialize_qr_status_scaned_but_redirect() {
        let raw = r#"{"status":"scaned_but_redirect","redirect_host":"sg.ilink.weixin.qq.com"}"#;
        let s: QrStatusResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(s.status, "scaned_but_redirect");
        assert_eq!(s.redirect_host.as_deref(), Some("sg.ilink.weixin.qq.com"));
    }

    #[test]
    fn deserialize_qr_status_confirmed_with_all_fields() {
        let raw = r#"{
            "status":"confirmed",
            "bot_token":"tk-abc",
            "ilink_bot_id":"bot-123",
            "baseurl":"https://sg.ilink.weixin.qq.com",
            "ilink_user_id":"wxid_alice@im.wechat"
        }"#;
        let s: QrStatusResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(s.status, "confirmed");
        assert_eq!(s.bot_token.as_deref(), Some("tk-abc"));
        assert_eq!(s.ilink_bot_id.as_deref(), Some("bot-123"));
        assert_eq!(s.baseurl.as_deref(), Some("https://sg.ilink.weixin.qq.com"));
        assert_eq!(s.ilink_user_id.as_deref(), Some("wxid_alice@im.wechat"));
    }

    #[test]
    fn session_transitions_wait_then_confirmed() {
        let mut session = LoginSession::new("qr-1".into());
        assert!(matches!(
            session.tick(QrStatusResponse {
                status: "wait".into(),
                ..Default::default()
            }),
            LoginStep::KeepWaiting
        ));
        assert!(matches!(
            session.tick(QrStatusResponse {
                status: "scaned".into(),
                ..Default::default()
            }),
            LoginStep::Scanned
        ));
        match session.tick(QrStatusResponse {
            status: "confirmed".into(),
            bot_token: Some("tk".into()),
            ilink_bot_id: Some("bot".into()),
            ilink_user_id: Some("wxid_alice@im.wechat".into()),
            baseurl: Some("https://sg.ilink.weixin.qq.com".into()),
            ..Default::default()
        }) {
            LoginStep::Confirmed(c) => {
                assert_eq!(c.bot_token, "tk");
                assert_eq!(c.ilink_bot_id, "bot");
                assert_eq!(c.ilink_user_id, "wxid_alice@im.wechat");
                assert_eq!(c.effective_base_url, "https://sg.ilink.weixin.qq.com");
            }
            other => panic!("expected Confirmed, got {other:?}"),
        }
    }

    #[test]
    fn session_switches_base_url_on_scaned_but_redirect() {
        let mut session = LoginSession::new("qr-1".into());
        let step = session.tick(QrStatusResponse {
            status: "scaned_but_redirect".into(),
            redirect_host: Some("sg.ilink.weixin.qq.com".into()),
            ..Default::default()
        });
        assert!(matches!(step, LoginStep::KeepWaiting));
        assert_eq!(session.current_base_url(), "https://sg.ilink.weixin.qq.com");
    }

    #[test]
    fn session_refresh_qr_up_to_3_times_then_fails() {
        let mut session = LoginSession::new("qr-1".into());
        for _ in 0..MAX_QR_REFRESH_COUNT {
            assert!(matches!(
                session.tick(QrStatusResponse {
                    status: "expired".into(),
                    ..Default::default()
                }),
                LoginStep::NeedsQrRefresh
            ));
            session.apply_new_qr("qr-next".into());
        }
        match session.tick(QrStatusResponse {
            status: "expired".into(),
            ..Default::default()
        }) {
            LoginStep::Failed(msg) => assert!(msg.contains("3 次")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
