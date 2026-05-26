//! NetworkProbe integration tests — Spec §9
//!
//! Covers: 200 → online, 5xx → server-degraded, connect-refused → offline,
//! and state dedup (unchanged status must not re-emit).
//!
//! Implementation note: wiremock 0.6 and mockito 1.x both return 502 for HEAD
//! requests because they use hyper under the hood, which does not handle HEAD
//! body-less responses correctly in server mode.
//!
//! Instead we use a hand-rolled minimal TCP server (tokio::net::TcpListener)
//! that speaks just enough HTTP/1.1 to answer HEAD with a configurable status.
//! The test reqwest client is built with `.no_proxy()` to bypass any system
//! HTTP proxy (e.g. Clash/v2ray on macOS) that would route localhost traffic
//! away from the stub servers.
//!
//! Run with:
//!   cargo test --test network_probe_integration_test --features test-support -- --nocapture

use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use app_lib::runtime::agent::{
    AgentNameRegistry, CancellationRegistry, InboxRegistry, LeadIdleSupervisor, TeamRegistry,
};
use app_lib::runtime::network::probe::NetworkProbe;
use app_lib::transport::runtime_host::RuntimeHost;

// ── CapturingHost ──────────────────────────────────────────────────────────

/// Minimal RuntimeHost that records every emit_legacy_event call.
struct CapturingHost {
    events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl CapturingHost {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
        })
    }
}

impl RuntimeHost for CapturingHost {
    fn emit_legacy_event(&self, name: &str, payload: serde_json::Value) -> Result<()> {
        self.events.lock().unwrap().push((name.to_string(), payload));
        Ok(())
    }

    fn team_registry(&self) -> Arc<TeamRegistry> {
        TeamRegistry::new()
    }

    fn agent_names(&self) -> Arc<AgentNameRegistry> {
        AgentNameRegistry::new()
    }

    fn inbox_registry(&self) -> Arc<InboxRegistry> {
        InboxRegistry::new()
    }

    fn lead_idle_supervisor(&self) -> Arc<LeadIdleSupervisor> {
        LeadIdleSupervisor::new()
    }

    fn cancellation_registry(&self) -> Arc<CancellationRegistry> {
        CancellationRegistry::new()
    }
}

// ── minimal HTTP stub server ───────────────────────────────────────────────

/// Spawn a minimal TCP server that accepts connections and responds to any
/// HTTP request with `status_code`. Returns the base URL `http://127.0.0.1:PORT`.
///
/// Uses raw TCP + manual HTTP/1.1 framing to avoid the HEAD-502 bug in
/// wiremock 0.6 / mockito 1.x (both built on hyper's server side).
async fn start_stub_server(status_code: u16) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        // Handle up to 10 connections — covers the dedup test's 3 probes.
        for _ in 0..10 {
            let Ok((mut stream, _)) = listener.accept().await else { break };
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let reason = match status_code {
                200 => "OK",
                503 => "Service Unavailable",
                _ => "Unknown",
            };
            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                status_code, reason
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    format!("http://127.0.0.1:{}", port)
}

// ── helpers ────────────────────────────────────────────────────────────────

fn status_of(event: &(String, Value)) -> &str {
    event.1.get("status").and_then(Value::as_str).unwrap_or("<missing>")
}

// ── tests ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn probe_200_emits_online_status_changed() {
    let url = start_stub_server(200).await;

    let host = CapturingHost::new();
    let probe = NetworkProbe::new_for_test(host.clone(), url);
    probe.probe_once_for_test().await;

    let events = host.events.lock().unwrap();
    assert_eq!(events.len(), 1, "first probe must emit exactly one event");
    assert_eq!(events[0].0, "network:status");
    assert_eq!(status_of(&events[0]), "online");
}

#[tokio::test]
async fn probe_500_emits_server_degraded() {
    let url = start_stub_server(503).await;

    let host = CapturingHost::new();
    let probe = NetworkProbe::new_for_test(host.clone(), url);
    probe.probe_once_for_test().await;

    let events = host.events.lock().unwrap();
    assert_eq!(events.len(), 1, "5xx probe must emit exactly one event");
    assert_eq!(events[0].0, "network:status");
    // NetworkStatus::ServerDegraded serializes as "server-degraded" (kebab-case).
    assert_eq!(status_of(&events[0]), "server-degraded");
}

#[tokio::test]
async fn probe_connect_refused_emits_offline() {
    // Port 1 is reserved and virtually never has a listener — connection
    // should be refused immediately (no server needed).
    let host = CapturingHost::new();
    let probe = NetworkProbe::new_for_test(host.clone(), "http://127.0.0.1:1".to_string());
    probe.probe_once_for_test().await;

    let events = host.events.lock().unwrap();
    assert_eq!(events.len(), 1, "connect-refused probe must emit exactly one event");
    assert_eq!(events[0].0, "network:status");
    assert_eq!(status_of(&events[0]), "offline");
    // errorKind must be present (camelCase per NetworkSnapshot serialization).
    assert!(
        events[0].1.get("errorKind").is_some(),
        "errorKind must be present for offline status"
    );
}

#[tokio::test]
async fn probe_dedups_unchanged_status() {
    let url = start_stub_server(200).await;

    let host = CapturingHost::new();
    let probe = NetworkProbe::new_for_test(host.clone(), url);
    // Three consecutive probes with the same 200 result.
    probe.probe_once_for_test().await;
    probe.probe_once_for_test().await;
    probe.probe_once_for_test().await;

    let events = host.events.lock().unwrap();
    assert_eq!(
        events.len(),
        1,
        "unchanged status must not re-emit (dedup): expected 1 event, got {}",
        events.len()
    );
}
