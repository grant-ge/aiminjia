//! `manager::begin_wechat_registration` / `manager::poll_wechat_registration`
//! call into this module. Holds the per-`device_code` `LoginSession` map so
//! `poll_registration` can pick up where `begin_registration` left off.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

use super::endpoints::DEFAULT_BASE_URL;
use super::login::{fetch_qrcode, poll_qr_status, ConfirmedLogin, LoginSession, LoginStep};

/// Active scan sessions keyed by the `qrcode` string (which doubles as the
/// `device_code` we hand the frontend so its `RegistrationModal` can poll us
/// back). One global map is fine: MVP allows one in-flight wechat scan at a
/// time, just like dingtalk's flow.
static ACTIVE_SESSIONS: Lazy<Mutex<HashMap<String, LoginSession>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// HTTP client kept around so we don't pay TCP / TLS setup per request.
static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .user_agent("aijia-wechat-ilink/0.1 (https://github.com/grant-ge/aiminjia)")
        .build()
        .expect("reqwest client should build")
});

/// Connection-pool-tuned client used only for `begin_registration`.
///
/// History:
///   - Originally we used the shared module-level `HTTP_CLIENT` (40s timeout,
///     default pool). After a user logged in once, the iLink server side
///     associated keep-alive sockets in that pool with the logged-in bot
///     identity. Reopening the QR modal triggered a new anonymous
///     `fetch_qrcode`, but reqwest happily reused those "poisoned" sockets and
///     iLink hung the request for ~60s before idle-timing-out the connection.
///     Symptom: "正在准备扫码…" stuck on the modal for ~60s before unsticking.
///   - We first tried `pool_max_idle_per_host(0)` (no pool at all). That fixed
///     the deadlock but introduced a *new* problem: cold-start TLS handshakes
///     on macOS occasionally fail outright with "error sending request"
///     ~5s after the GET (TLS / DNS race). The end-user experience was just
///     as broken — they'd see an error toast on first open.
///   - Final design: keep the pool, but cap idle-per-host to 1 socket AND set
///     a very short `pool_idle_timeout` so any "poisoned" socket from the
///     previous flow gets reaped before the next begin reuses it. 4 seconds
///     is well under iLink's ~60s server-side keep-alive but long enough that
///     a quick reopen still finds the socket warm.
fn build_fresh_begin_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(Duration::from_secs(4))
        .user_agent("aijia-wechat-ilink/0.1 (https://github.com/grant-ge/aiminjia)")
        .build()
        .map_err(|e| anyhow!("wechat begin client build: {e}"))
}

/// Mirrors `ChannelRegistrationBeginResult` fields we care about. Manager
/// converts this into the public type before returning to the Tauri command.
pub struct WechatBegin {
    pub device_code: String,
    pub qr_url: String,
    pub interval_seconds: u64,
    pub expires_in_seconds: u64,
}

#[derive(Debug)]
pub enum WechatPollState {
    Waiting,
    /// Phone has scanned the QR; user is now at the "在手机上确认" step on
    /// WeChat. We surface this as a distinct UI state so the modal can swap
    /// the QR for a "✓ 已扫码，请在手机上确认" banner instead of leaving the
    /// user staring at a stale QR.
    Scanned,
    /// Caller should reissue `begin_registration` to the frontend with this new
    /// device_code + QR URL. The existing modal can swap its `qrUrl` prop and
    /// keep polling.
    Refreshed {
        new_device_code: String,
        new_qr_url: String,
    },
    Success(ConfirmedLogin),
    Fail(String),
    Expired,
}

/// `begin_registration` — fetch a QR + store session keyed by qrcode.
pub async fn begin_registration(app_id: &str, client_version: &str) -> Result<WechatBegin> {
    // Use a one-shot client to dodge the post-login keep-alive deadlock described
    // on `build_fresh_begin_client`. Cost: ~1 extra TCP+TLS handshake per modal
    // open. Benefit: deterministic ≤20s response instead of "hangs for ~60s".
    let client = build_fresh_begin_client()?;
    let resp = fetch_qrcode(&client, app_id, client_version, DEFAULT_BASE_URL)
        .await
        .map_err(|e| anyhow!("wechat fetch_qrcode: {e}"))?;
    let session = LoginSession::new(resp.qrcode.clone());
    ACTIVE_SESSIONS
        .lock()
        .await
        .insert(resp.qrcode.clone(), session);
    Ok(WechatBegin {
        device_code: resp.qrcode,
        qr_url: resp.qrcode_img_content,
        // iLink doesn't return interval / expires_in; openclaw polls
        // server-suggested timeout (long-poll). Frontend uses these to drive
        // the countdown and poll cadence — pick sensible defaults.
        interval_seconds: 2,
        // The QR itself is fairly short-lived (~2-3 min per refresh) but
        // LoginSession refreshes automatically up to MAX_QR_REFRESH_COUNT, so
        // expose a longer overall ceiling.
        expires_in_seconds: 600,
    })
}

/// `poll_registration` — drives the LoginSession state machine one step.
/// On `NeedsQrRefresh` it auto-refetches the QR (within the session refresh
/// budget) and signals the frontend via `Refreshed` so it can swap the QR.
pub async fn poll_registration(
    app_id: &str,
    client_version: &str,
    device_code: &str,
) -> Result<WechatPollState> {
    let mut sessions = ACTIVE_SESSIONS.lock().await;
    let session = sessions
        .get_mut(device_code)
        .ok_or_else(|| anyhow!("no active wechat scan session for device_code={device_code}"))?;

    let resp = poll_qr_status(
        &HTTP_CLIENT,
        app_id,
        client_version,
        session.current_base_url(),
        device_code,
    )
    .await
    .map_err(|e| anyhow!("wechat poll_qr_status: {e}"))?;

    match session.tick(resp) {
        LoginStep::KeepWaiting => Ok(WechatPollState::Waiting),
        LoginStep::Scanned => Ok(WechatPollState::Scanned),
        LoginStep::NeedsQrRefresh => {
            let new_qr = fetch_qrcode(
                &HTTP_CLIENT,
                app_id,
                client_version,
                session.current_base_url(),
            )
            .await
            .map_err(|e| anyhow!("wechat refetch_qrcode: {e}"))?;
            session.apply_new_qr(new_qr.qrcode.clone());
            // Re-key the session map: drop old qrcode, insert under new one.
            let owned = sessions.remove(device_code).unwrap();
            sessions.insert(new_qr.qrcode.clone(), owned);
            Ok(WechatPollState::Refreshed {
                new_device_code: new_qr.qrcode,
                new_qr_url: new_qr.qrcode_img_content,
            })
        }
        LoginStep::Confirmed(c) => {
            sessions.remove(device_code);
            Ok(WechatPollState::Success(c))
        }
        LoginStep::Failed(msg) => {
            sessions.remove(device_code);
            // Treat 3-refreshes-exhausted as Expired so the frontend shows
            // "expired" instead of a generic fail banner.
            if msg.contains("过期") {
                Ok(WechatPollState::Expired)
            } else {
                Ok(WechatPollState::Fail(msg))
            }
        }
    }
}

/// Drop any in-flight session for this `device_code`. Manager calls this on
/// `remove_wechat` so a stale modal poll on app re-open doesn't keep firing.
#[allow(dead_code)]
pub async fn forget_session(device_code: &str) {
    ACTIVE_SESSIONS.lock().await.remove(device_code);
}

/// For diagnostics.
#[allow(dead_code)]
pub async fn active_session_count() -> usize {
    ACTIVE_SESSIONS.lock().await.len()
}

#[allow(dead_code)]
fn _suppress_arc_unused() {
    let _: Arc<()> = Arc::new(());
}
