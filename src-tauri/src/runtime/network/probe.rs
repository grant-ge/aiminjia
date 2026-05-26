use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use log::{debug, info, warn};

use crate::runtime::network::state::{NetworkErrorKind, NetworkSnapshot, NetworkStatus};
use crate::transport::runtime_host::RuntimeHost;

// ── constants ──────────────────────────────────────────────────────────────

const PROBE_URL: &str = "https://ai-tenant.renlijia.com";
const ONLINE_INTERVAL_SECS: u64 = 30;
const OFFLINE_INTERVAL_SECS: u64 = 10;
const RECOVERY_SUCCESS_THRESHOLD: u32 = 3;
const HEAD_TIMEOUT_SECS: u64 = 5;
const FORCE_PROBE_THROTTLE_MS: i64 = 1000;

// ── pure classification functions ─────────────────────────────────────────

/// 把一次 HEAD 请求的结果（reqwest::Result<reqwest::Response>）映射为三态。
pub(crate) fn classify_response(
    result: &Result<reqwest::Response, reqwest::Error>,
) -> (NetworkStatus, Option<NetworkErrorKind>) {
    match result {
        Ok(resp) => {
            let status = resp.status();
            if status.is_server_error() {
                (NetworkStatus::ServerDegraded, None)
            } else {
                // 2xx / 3xx / 4xx including 401/403 — TCP+TLS+HTTP shook hands.
                (NetworkStatus::Online, None)
            }
        }
        Err(err) => {
            let kind = classify_error(err);
            (NetworkStatus::Offline, Some(kind))
        }
    }
}

pub(crate) fn classify_error(err: &reqwest::Error) -> NetworkErrorKind {
    if err.is_timeout() {
        return NetworkErrorKind::Timeout;
    }
    if err.is_connect() {
        let msg = err.to_string().to_lowercase();
        if msg.contains("dns") || msg.contains("name resolution") || msg.contains("lookup") {
            return NetworkErrorKind::Dns;
        }
        if msg.contains("refused") {
            return NetworkErrorKind::ConnectRefused;
        }
        if msg.contains("certificate") || msg.contains("tls") || msg.contains("ssl") {
            return NetworkErrorKind::Tls;
        }
        return NetworkErrorKind::Other;
    }
    NetworkErrorKind::Other
}

// ── NetworkProbe ──────────────────────────────────────────────────────────

pub struct NetworkProbe {
    client: reqwest::Client,
    host: Arc<dyn RuntimeHost>,
    snapshot: Arc<Mutex<Option<NetworkSnapshot>>>,
    force_tx: mpsc::Sender<()>,
    force_rx: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    last_force_at_ms: Arc<Mutex<i64>>,
    pub(crate) probe_url_override: Option<String>,
}

impl NetworkProbe {
    pub fn new(host: Arc<dyn RuntimeHost>) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(HEAD_TIMEOUT_SECS))
            .build()
            .expect("network probe reqwest client");
        let (force_tx, force_rx) = mpsc::channel(4);
        Arc::new(Self {
            client,
            host,
            snapshot: Arc::new(Mutex::new(None)),
            force_tx,
            force_rx: Arc::new(Mutex::new(Some(force_rx))),
            last_force_at_ms: Arc::new(Mutex::new(0)),
            probe_url_override: None,
        })
    }

    pub fn snapshot(&self) -> Option<NetworkSnapshot> {
        self.snapshot.lock().unwrap().clone()
    }

    /// Best-effort throttled force probe. Returns true if a probe was triggered,
    /// false if throttled.
    pub fn request_force_probe(&self) -> bool {
        let now_ms = Utc::now().timestamp_millis();
        let mut last = self.last_force_at_ms.lock().unwrap();
        if now_ms - *last < FORCE_PROBE_THROTTLE_MS {
            return false;
        }
        match self.force_tx.try_send(()) {
            Ok(_) => {
                *last = now_ms;
                true
            }
            Err(_) => false,
        }
    }

    /// Spawn the long-running probe task. Returns immediately.
    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            self.run_loop().await;
        });
    }

    async fn run_loop(self: Arc<Self>) {
        let mut force_rx = match self.force_rx.lock().unwrap().take() {
            Some(rx) => rx,
            None => {
                warn!("network probe: run_loop called twice, ignoring");
                return;
            }
        };

        let mut current_interval = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(ONLINE_INTERVAL_SECS),
            Duration::from_secs(ONLINE_INTERVAL_SECS),
        );
        current_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut current_period = ONLINE_INTERVAL_SECS;
        let mut consecutive_success = 0u32;

        // Initial probe immediately.
        self.probe_once_and_emit().await;

        loop {
            tokio::select! {
                _ = current_interval.tick() => {
                    self.probe_once_and_emit().await;
                }
                _ = force_rx.recv() => {
                    self.probe_once_and_emit().await;
                }
            }

            let snap = self.snapshot.lock().unwrap().clone();
            let desired_period = match snap.as_ref().map(|s| s.status) {
                Some(NetworkStatus::Offline) => {
                    consecutive_success = 0;
                    OFFLINE_INTERVAL_SECS
                }
                Some(NetworkStatus::Online) | Some(NetworkStatus::ServerDegraded) => {
                    consecutive_success = consecutive_success.saturating_add(1);
                    if current_period == OFFLINE_INTERVAL_SECS
                        && consecutive_success >= RECOVERY_SUCCESS_THRESHOLD
                    {
                        ONLINE_INTERVAL_SECS
                    } else {
                        current_period
                    }
                }
                None => current_period,
            };
            if desired_period != current_period {
                current_period = desired_period;
                current_interval = tokio::time::interval_at(
                    tokio::time::Instant::now() + Duration::from_secs(current_period),
                    Duration::from_secs(current_period),
                );
                current_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            }
        }
    }

    async fn probe_once_and_emit(&self) {
        let url = self.probe_url_override.as_deref().unwrap_or(PROBE_URL);
        let started = Instant::now();
        let result = self.client.head(url).send().await;
        let elapsed_ms = started.elapsed().as_millis() as u32;
        let (status, error_kind) = classify_response(&result);

        match (&result, status) {
            (Ok(_), NetworkStatus::Online) => {
                debug!(
                    "network probe ok: status=online latency_ms={}",
                    elapsed_ms
                );
            }
            (Ok(resp), NetworkStatus::ServerDegraded) => {
                warn!(
                    "network probe degraded: http_status={} elapsed_ms={}",
                    resp.status(),
                    elapsed_ms
                );
            }
            (Err(err), _) => {
                warn!(
                    "network probe failed: kind={:?} error=\"{}\" elapsed_ms={}",
                    error_kind, err, elapsed_ms
                );
            }
            _ => {}
        }

        let latency_ms = if matches!(status, NetworkStatus::Online | NetworkStatus::ServerDegraded)
        {
            Some(elapsed_ms)
        } else {
            None
        };
        let snapshot = NetworkSnapshot {
            status,
            last_check_at_ms: Utc::now().timestamp_millis(),
            latency_ms,
            error_kind,
        };

        // Update stored snapshot; determine if status changed.
        // The lock is dropped before emit to avoid holding it across the FFI call.
        let changed = {
            let mut guard = self.snapshot.lock().unwrap();
            let changed = match guard.as_ref() {
                Some(prev) => prev.status != snapshot.status,
                None => true,
            };
            *guard = Some(snapshot.clone());
            changed
        }; // lock released here

        if changed {
            let status_str = match snapshot.status {
                NetworkStatus::Online => "online",
                NetworkStatus::Offline => "offline",
                NetworkStatus::ServerDegraded => "server-degraded",
            };
            info!("network status changed -> {}", status_str);
            let payload = serde_json::to_value(&snapshot).unwrap_or(json!({}));
            if let Err(e) = self.host.emit_legacy_event("network:status", payload) {
                warn!("emit network:status failed: {}", e);
            }
        }
    }

    // ── test-only API ─────────────────────────────────────────────────────

    /// Construct a probe with a custom URL for integration testing.
    /// Gated by the `test-support` feature so it never appears in production
    /// builds. Integration tests must be run with `--features test-support`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_for_test(host: Arc<dyn RuntimeHost>, probe_url: String) -> Arc<Self> {
        // no_proxy: bypass system HTTP proxy (e.g. Clash/v2ray on macOS) for
        // loopback addresses so integration tests hit stub servers directly.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(HEAD_TIMEOUT_SECS))
            .no_proxy()
            .build()
            .expect("network probe reqwest client");
        let (force_tx, force_rx) = mpsc::channel(4);
        Arc::new(Self {
            client,
            host,
            snapshot: Arc::new(Mutex::new(None)),
            force_tx,
            force_rx: Arc::new(Mutex::new(Some(force_rx))),
            last_force_at_ms: Arc::new(Mutex::new(0)),
            probe_url_override: Some(probe_url),
        })
    }

    /// Execute a single probe cycle and emit — for integration tests only.
    /// Does not start the run_loop (avoids infinite loops in tests).
    #[cfg(any(test, feature = "test-support"))]
    pub async fn probe_once_for_test(&self) {
        self.probe_once_and_emit().await;
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn ok_response(status: StatusCode) -> Result<reqwest::Response, reqwest::Error> {
        Ok(reqwest::Response::from(
            http::Response::builder()
                .status(status)
                .body("")
                .unwrap(),
        ))
    }

    #[test]
    fn test_200_is_online() {
        let (status, kind) = classify_response(&ok_response(StatusCode::OK));
        assert_eq!(status, NetworkStatus::Online);
        assert_eq!(kind, None);
    }

    #[test]
    fn test_401_is_online() {
        let (status, kind) = classify_response(&ok_response(StatusCode::UNAUTHORIZED));
        assert_eq!(status, NetworkStatus::Online);
        assert_eq!(kind, None);
    }

    #[test]
    fn test_500_is_server_degraded() {
        let (status, _) = classify_response(&ok_response(StatusCode::INTERNAL_SERVER_ERROR));
        assert_eq!(status, NetworkStatus::ServerDegraded);
    }

    #[test]
    fn test_502_is_server_degraded() {
        let (status, _) = classify_response(&ok_response(StatusCode::BAD_GATEWAY));
        assert_eq!(status, NetworkStatus::ServerDegraded);
    }
}
