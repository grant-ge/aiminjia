use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_REGISTRATION_BASE_URL: &str = "https://oapi.dingtalk.com";
pub const OPEN_CLAW_SOURCE: &str = "OPEN_CLAW";
const OPEN_CLAW_VERIFY_URL: &str = "https://open-dev.dingtalk.com/openapp/registration/openClaw";
const REGISTRATION_HTTP_TIMEOUT_SECS: u64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistrationPollState {
    Waiting,
    Success,
    Fail,
    Expired,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationBeginResult {
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
pub struct RegistrationPollResult {
    pub state: RegistrationPollState,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub robot_code: Option<String>,
    pub fail_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    errcode: i64,
    errmsg: Option<String>,
    #[serde(flatten)]
    data: T,
}

#[derive(Debug, Deserialize)]
struct InitData {
    nonce: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BeginData {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    interval: Option<u64>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PollData {
    status: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    robot_code: Option<String>,
    fail_reason: Option<String>,
}

pub fn normalize_poll_status(status: &str) -> RegistrationPollState {
    match status.trim().to_ascii_uppercase().as_str() {
        "WAITING" => RegistrationPollState::Waiting,
        "SUCCESS" => RegistrationPollState::Success,
        "FAIL" => RegistrationPollState::Fail,
        "EXPIRED" => RegistrationPollState::Expired,
        _ => RegistrationPollState::Unknown,
    }
}

pub fn build_open_claw_verification_url(user_code: &str, source: &str) -> String {
    let mut url = format!(
        "{OPEN_CLAW_VERIFY_URL}?user_code={}",
        urlencoding::encode(user_code)
    );
    if !source.trim().is_empty() {
        url.push_str("&source=");
        url.push_str(&urlencoding::encode(source.trim()));
    }
    url
}

fn registration_base_url() -> String {
    std::env::var("DINGTALK_REGISTRATION_BASE_URL")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRATION_BASE_URL.to_string())
}

fn registration_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(REGISTRATION_HTTP_TIMEOUT_SECS))
        .build()
        .context("Failed to build DingTalk registration HTTP client")
}

fn init_payload(source: &str) -> serde_json::Value {
    serde_json::json!({ "source": source })
}

fn begin_payload(nonce: &str, source: &str) -> serde_json::Value {
    serde_json::json!({ "nonce": nonce, "source": source })
}

fn poll_payload(device_code: &str, source: &str) -> serde_json::Value {
    serde_json::json!({ "device_code": device_code, "source": source })
}

fn api_error<T>(action: &str, envelope: ApiEnvelope<T>) -> Result<ApiEnvelope<T>> {
    if envelope.errcode == 0 {
        Ok(envelope)
    } else {
        Err(anyhow!(
            "DingTalk registration {action} failed: {} (errcode={})",
            envelope
                .errmsg
                .unwrap_or_else(|| "unknown error".to_string()),
            envelope.errcode
        ))
    }
}

pub async fn begin_registration() -> Result<RegistrationBeginResult> {
    let source = OPEN_CLAW_SOURCE.to_string();
    let base_url = registration_base_url();
    let client = registration_http_client()?;

    let init_resp = client
        .post(format!("{base_url}/app/registration/init"))
        .json(&init_payload(&source))
        .send()
        .await
        .context("Failed to initialize DingTalk registration")?;
    let init_status = init_resp.status();
    if !init_status.is_success() {
        let body = init_resp.text().await.unwrap_or_default();
        anyhow::bail!("DingTalk registration init HTTP {init_status}: {body}");
    }
    let init: ApiEnvelope<InitData> = init_resp
        .json()
        .await
        .context("Failed to parse DingTalk registration init response")?;
    let init = api_error("init", init)?;
    let nonce = init
        .data
        .nonce
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("DingTalk registration init response missing nonce"))?;

    let begin_resp = client
        .post(format!("{base_url}/app/registration/begin"))
        .json(&begin_payload(&nonce, &source))
        .send()
        .await
        .context("Failed to begin DingTalk registration")?;
    let begin_status = begin_resp.status();
    if !begin_status.is_success() {
        let body = begin_resp.text().await.unwrap_or_default();
        anyhow::bail!("DingTalk registration begin HTTP {begin_status}: {body}");
    }
    let begin: ApiEnvelope<BeginData> = begin_resp
        .json()
        .await
        .context("Failed to parse DingTalk registration begin response")?;
    let begin = api_error("begin", begin)?;
    let device_code = begin
        .data
        .device_code
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("DingTalk registration begin response missing device_code"))?;
    let user_code = begin
        .data
        .user_code
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("DingTalk registration begin response missing user_code"))?;
    let verification_uri = begin
        .data
        .verification_uri
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| OPEN_CLAW_VERIFY_URL.to_string());
    let verification_uri_complete = begin
        .data
        .verification_uri_complete
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| build_open_claw_verification_url(&user_code, &source));

    Ok(RegistrationBeginResult {
        device_code,
        user_code,
        verification_uri_complete,
        verification_uri,
        interval_seconds: begin.data.interval.unwrap_or(2).max(1),
        expires_in_seconds: begin.data.expires_in.unwrap_or(7200),
        source,
    })
}

pub async fn poll_registration(device_code: &str) -> Result<RegistrationPollResult> {
    let trimmed = device_code.trim();
    if trimmed.is_empty() {
        anyhow::bail!("device_code is required");
    }

    let base_url = registration_base_url();
    let client = registration_http_client()?;
    let resp = client
        .post(format!("{base_url}/app/registration/poll"))
        .json(&poll_payload(trimmed, OPEN_CLAW_SOURCE))
        .send()
        .await
        .context("Failed to poll DingTalk registration")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("DingTalk registration poll HTTP {status}: {body}");
    }
    let poll: ApiEnvelope<PollData> = resp
        .json()
        .await
        .context("Failed to parse DingTalk registration poll response")?;
    let poll = api_error("poll", poll)?;
    let state = normalize_poll_status(poll.data.status.as_deref().unwrap_or_default());

    Ok(RegistrationPollResult {
        state,
        client_id: poll.data.client_id.filter(|v| !v.trim().is_empty()),
        client_secret: poll.data.client_secret.filter(|v| !v.trim().is_empty()),
        robot_code: poll.data.robot_code.filter(|v| !v.trim().is_empty()),
        fail_reason: poll.data.fail_reason.filter(|v| !v.trim().is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_open_claw_verification_url_with_source() {
        let url = build_open_claw_verification_url("ABCD-EFGH-IJKL", "OPEN_CLAW");
        assert_eq!(
            url,
            "https://open-dev.dingtalk.com/openapp/registration/openClaw?user_code=ABCD-EFGH-IJKL&source=OPEN_CLAW"
        );
    }

    #[test]
    fn registration_payloads_keep_open_claw_source() {
        assert_eq!(init_payload(OPEN_CLAW_SOURCE)["source"], "OPEN_CLAW");
        assert_eq!(
            begin_payload("nonce-1", OPEN_CLAW_SOURCE)["nonce"],
            "nonce-1"
        );
        assert_eq!(
            begin_payload("nonce-1", OPEN_CLAW_SOURCE)["source"],
            "OPEN_CLAW"
        );
        assert_eq!(
            poll_payload("device-1", OPEN_CLAW_SOURCE)["device_code"],
            "device-1"
        );
        assert_eq!(
            poll_payload("device-1", OPEN_CLAW_SOURCE)["source"],
            "OPEN_CLAW"
        );
    }

    #[test]
    fn normalizes_poll_status_values() {
        assert_eq!(
            normalize_poll_status("SUCCESS"),
            RegistrationPollState::Success
        );
        assert_eq!(
            normalize_poll_status("waiting"),
            RegistrationPollState::Waiting
        );
        assert_eq!(
            normalize_poll_status("EXPIRED"),
            RegistrationPollState::Expired
        );
        assert_eq!(
            normalize_poll_status("whatever"),
            RegistrationPollState::Unknown
        );
    }
}
