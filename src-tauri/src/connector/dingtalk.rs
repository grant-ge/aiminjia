//! DingtalkBridge — dws CLI sidecar for DingTalk workspace operations.
//!
//! Manages `dws` command execution, JSON output parsing, and authentication
//! status tracking.
//! Covers 5 product domains: AI Table, Contacts, Chat, Calendar, Todo.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

use crate::storage::process_ext::NoWindowExt;

/// Authentication status for DingTalk connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DingtalkAuthStatus {
    Connected {
        user_name: String,
        corp_name: String,
    },
    Disconnected,
    Expired,
}

/// Info returned to frontend about DingTalk connection state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStatusInfo {
    pub connected: bool,
    pub user_name: Option<String>,
    pub corp_name: Option<String>,
}

impl From<&DingtalkAuthStatus> for DingtalkStatusInfo {
    fn from(status: &DingtalkAuthStatus) -> Self {
        match status {
            DingtalkAuthStatus::Connected {
                user_name,
                corp_name,
            } => DingtalkStatusInfo {
                connected: true,
                user_name: Some(user_name.clone()),
                corp_name: Some(corp_name.clone()),
            },
            _ => DingtalkStatusInfo {
                connected: false,
                user_name: None,
                corp_name: None,
            },
        }
    }
}

pub struct DingtalkBridge {
    app_handle: AppHandle,
    auth_status: RwLock<DingtalkAuthStatus>,
}

impl DingtalkBridge {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            auth_status: RwLock::new(DingtalkAuthStatus::Disconnected),
        }
    }

    // ── Binary Resolution ───────────────────────────────────────

    /// Find the dws binary from the system PATH.
    fn find_dws(&self) -> Result<PathBuf> {
        #[cfg(not(target_os = "windows"))]
        if let Ok(output) = std::process::Command::new("which")
            .arg("dws")
            .no_window()
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(PathBuf::from(path));
                }
            }
        }

        #[cfg(target_os = "windows")]
        if let Ok(output) = std::process::Command::new("where.exe")
            .arg("dws")
            .no_window()
            .output()
        {
            if output.status.success() {
                let path = crate::storage::console_decode::decode_console_bytes(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !path.is_empty() {
                    return Ok(PathBuf::from(path));
                }
            }
        }

        Err(anyhow!(
            "system dws CLI not found. Install dingtalk-workspace-cli or add dws to PATH."
        ))
    }

    // ── Command Execution ───────────────────────────────────────

    /// Execute a dws command and return parsed JSON output.
    ///
    /// All commands are run with `--format json --yes` to ensure machine-readable
    /// output and skip interactive confirmations (confirmation is handled by AIjia UI).
    pub async fn exec(&self, args: &[&str], timeout_secs: u64) -> Result<Value> {
        let dws_path = self.find_dws()?;
        let timeout = Duration::from_secs(timeout_secs);

        // Build command with --format json --yes appended
        let mut cmd = tokio::process::Command::new(&dws_path);
        cmd.args(args)
            .args(["--format", "json", "--yes"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Windows: suppress CMD window
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let child = cmd.spawn().context("Failed to spawn dws process")?;

        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| anyhow!("dws command timed out after {}s", timeout_secs))?
            .context("dws process failed")?;

        let stdout = crate::storage::console_decode::decode_console_bytes(&output.stdout);
        let stderr = crate::storage::console_decode::decode_console_bytes(&output.stderr);

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            // Check for auth expiry
            if stderr.contains("token")
                && (stderr.contains("expired") || stderr.contains("invalid"))
            {
                *self.auth_status.write().await = DingtalkAuthStatus::Expired;
                return Err(anyhow!(
                    "DingTalk authorization expired. Please re-authorize in Settings → DingTalk Account."
                ));
            }
            return Err(anyhow!(
                "dws command failed (exit {}): {}",
                code,
                stderr.trim()
            ));
        }

        // Parse JSON from stdout
        let json: Value = serde_json::from_str(stdout.trim()).context(format!(
            "Failed to parse dws JSON output: {}",
            stdout.chars().take(200).collect::<String>()
        ))?;

        Ok(json)
    }

    /// Execute a dws command for read operations (30s timeout).
    pub async fn query(&self, args: &[&str]) -> Result<Value> {
        self.exec(args, 30).await
    }

    /// Execute a dws command for write operations (60s timeout).
    pub async fn mutate(&self, args: &[&str]) -> Result<Value> {
        self.exec(args, 60).await
    }

    // ── Authentication ──────────────────────────────────────────

    /// Start OAuth login using dws default mode (local callback server).
    ///
    /// Flow:
    /// 1. Spawns `dws auth login` — starts a local HTTP callback server
    /// 2. Reads stderr to find the OAuth URL (dws outputs it there when piped)
    /// 3. Opens the URL in system browser via `open` / `cmd /C start` / `xdg-open`
    /// 4. User scans QR code in browser → DingTalk redirects to local callback
    /// 5. dws process exits on success → returns connection status
    pub async fn login(&self) -> Result<DingtalkStatusInfo> {
        let dws_path = self.find_dws()?;

        let mut cmd = tokio::process::Command::new(&dws_path);
        cmd.args(["auth", "login"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000);
        }

        let mut child = cmd.spawn().context("Failed to start dws auth login")?;

        // dws outputs interactive info (OAuth URL) to stderr when stdout is piped.
        let stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("Failed to capture dws stderr"))?;

        let mut reader = tokio::io::BufReader::new(stderr_pipe);
        let mut url_opened = false;

        use tokio::io::AsyncBufReadExt;
        let mut line = String::new();
        let read_deadline = tokio::time::Instant::now() + Duration::from_secs(30);

        // Phase 1: scan stderr for the OAuth URL (max 30s)
        loop {
            line.clear();
            let read_result =
                tokio::time::timeout_at(read_deadline, reader.read_line(&mut line)).await;

            match read_result {
                Ok(Ok(0)) => break, // EOF — dws exited (maybe cached token was valid)
                Ok(Ok(_)) => {
                    let trimmed = line.trim();
                    log::info!("[dws-login] {}", trimmed);

                    if !url_opened {
                        if let Some(url) = extract_auth_url(trimmed) {
                            log::info!("[dws-login] Auth URL found: {}", url);
                            // Emit event to frontend so it can open the URL via Tauri shell plugin
                            let _ = self.app_handle.emit("dingtalk:auth-url", &url);
                            url_opened = true;
                        }
                    }

                    // Once we see "Waiting for authorization", stop reading and let dws run
                    if trimmed.contains("Waiting for authorization") {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    warn!("[dws-login] read error: {}", e);
                    break;
                }
                Err(_) => {
                    if !url_opened {
                        let _ = child.kill().await;
                        return Err(anyhow!("Timed out waiting for DingTalk authorization URL"));
                    }
                    break;
                }
            }
        }

        // Phase 2: wait for dws to finish (local callback receives OAuth redirect)
        // Default mode uses local HTTP server, so completion is near-instant after scan.
        // Timeout 5 min as safety net.
        let output = tokio::time::timeout(Duration::from_secs(300), child.wait_with_output())
            .await
            .map_err(|_| anyhow!("Login timed out. Please try again."))?
            .context("dws auth login failed")?;

        if !output.status.success() {
            let all_stderr = crate::storage::console_decode::decode_console_bytes(&output.stderr);
            return Err(anyhow!("Login failed: {}", all_stderr.trim()));
        }

        log::info!("[dws-login] Login completed successfully");
        self.refresh_status().await
    }

    /// Logout from DingTalk.
    pub async fn logout(&self) -> Result<()> {
        let dws_path = self.find_dws()?;

        let mut cmd = tokio::process::Command::new(&dws_path);
        cmd.args(["auth", "logout"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000);
        }

        let output = cmd
            .output()
            .await
            .context("Failed to run dws auth logout")?;

        if !output.status.success() {
            warn!("dws auth logout returned non-zero, ignoring");
        }

        *self.auth_status.write().await = DingtalkAuthStatus::Disconnected;
        Ok(())
    }

    /// Check current auth status.
    ///
    /// 1. `dws auth status` → check if authenticated
    /// 2. If authenticated, `dws contact user get-self` → get user name and org name
    pub async fn refresh_status(&self) -> Result<DingtalkStatusInfo> {
        // Step 1: check auth
        let auth_result = self.exec(&["auth", "status"], 10).await;

        match auth_result {
            Ok(json) => {
                let authenticated = json
                    .get("authenticated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if !authenticated {
                    *self.auth_status.write().await = DingtalkAuthStatus::Disconnected;
                    return Ok(DingtalkStatusInfo::from(&*self.auth_status.read().await));
                }

                // Step 2: get user info via contact get-self
                let (user_name, corp_name) =
                    match self.exec(&["contact", "user", "get-self"], 10).await {
                        Ok(user_json) => {
                            // Path: result[0].orgEmployeeModel.orgUserName / orgName
                            let employee = user_json
                                .get("result")
                                .and_then(|r| r.as_array())
                                .and_then(|arr| arr.first())
                                .and_then(|item| item.get("orgEmployeeModel"));

                            let name = employee
                                .and_then(|e| e.get("orgUserName"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown")
                                .to_string();
                            let org = employee
                                .and_then(|e| e.get("orgName"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            (name, org)
                        }
                        Err(e) => {
                            warn!("[dws] Failed to get user info: {}", e);
                            ("Unknown".to_string(), String::new())
                        }
                    };

                let status = DingtalkAuthStatus::Connected {
                    user_name,
                    corp_name,
                };
                let info = DingtalkStatusInfo::from(&status);
                *self.auth_status.write().await = status;
                Ok(info)
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("expired") {
                    *self.auth_status.write().await = DingtalkAuthStatus::Expired;
                } else {
                    *self.auth_status.write().await = DingtalkAuthStatus::Disconnected;
                }
                Ok(DingtalkStatusInfo::from(&*self.auth_status.read().await))
            }
        }
    }

    /// Check if currently authenticated.
    pub async fn is_connected(&self) -> bool {
        matches!(
            &*self.auth_status.read().await,
            DingtalkAuthStatus::Connected { .. }
        )
    }

    /// Get current status info (no network call).
    pub async fn status_info(&self) -> DingtalkStatusInfo {
        DingtalkStatusInfo::from(&*self.auth_status.read().await)
    }
}

// ── Helpers ─────────────────────────────────────────────────────

/// Extract the DingTalk authorization URL from a dws output line.
///
/// Matches two formats:
/// - Default mode: "https://login.dingtalk.com/oauth2/auth?client_id=...&redirect_uri=..."
/// - Device mode:  "https://login.dingtalk.com/oauth2/device/verify.htm?user_code=XXXX-XXXX"
fn extract_auth_url(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(start) = trimmed.find("https://login.dingtalk.com/") {
        let url_part = &trimmed[start..];
        let url = url_part.split_whitespace().next().unwrap_or(url_part);
        // Default mode: has client_id param (OAuth redirect flow)
        // Device mode: has user_code param
        if url.contains("client_id=") || url.contains("user_code=") {
            return Some(url.to_string());
        }
    }
    None
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_info_from_connected() {
        let status = DingtalkAuthStatus::Connected {
            user_name: "Zhang San".into(),
            corp_name: "Acme Corp".into(),
        };
        let info = DingtalkStatusInfo::from(&status);
        assert!(info.connected);
        assert_eq!(info.user_name.as_deref(), Some("Zhang San"));
        assert_eq!(info.corp_name.as_deref(), Some("Acme Corp"));
    }

    #[test]
    fn test_status_info_from_disconnected() {
        let status = DingtalkAuthStatus::Disconnected;
        let info = DingtalkStatusInfo::from(&status);
        assert!(!info.connected);
        assert!(info.user_name.is_none());
    }

    #[test]
    fn test_status_info_from_expired() {
        let status = DingtalkAuthStatus::Expired;
        let info = DingtalkStatusInfo::from(&status);
        assert!(!info.connected);
    }

    #[test]
    fn test_extract_auth_url_device_mode() {
        let line = "    https://login.dingtalk.com/oauth2/device/verify.htm?user_code=HWKM-BVJB";
        assert_eq!(
            extract_auth_url(line),
            Some("https://login.dingtalk.com/oauth2/device/verify.htm?user_code=HWKM-BVJB".into())
        );
    }

    #[test]
    fn test_extract_auth_url_default_mode() {
        let line = "  https://login.dingtalk.com/oauth2/auth?client_id=dingmbw5n9kt&redirect_uri=http%3A%2F%2F127.0.0.1%3A65042%2Fcallback&response_type=code&scope=openid+corpid";
        assert!(extract_auth_url(line).is_some());
        assert!(extract_auth_url(line).unwrap().contains("client_id="));
    }

    #[test]
    fn test_extract_auth_url_plain_link_no_params() {
        let line = "    link: https://login.dingtalk.com/oauth2/device/verify.htm";
        assert_eq!(extract_auth_url(line), None);
    }

    #[test]
    fn test_extract_auth_url_no_match() {
        let line = "⏳ Waiting for authorization...";
        assert_eq!(extract_auth_url(line), None);
    }
}
