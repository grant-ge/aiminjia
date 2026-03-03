//! Cloud authentication manager.
//!
//! Manages the login lifecycle:
//! 1. Login with username/password → get JWT tokens
//! 2. Create session key (sk-sess***) for API access
//! 3. Auto-renew expired tokens/keys
//! 4. Persist state encrypted at rest
//!
//! Thread-safe via `RwLock<Option<CloudAuth>>`.

pub mod client;
pub mod state;

use std::sync::Arc;
use anyhow::{anyhow, Result};
use chrono::Utc;
use tokio::sync::RwLock;

use crate::storage::crypto::SecureStorage;
use crate::storage::file_store::AppStorage;

use client::AuthClient;
use state::{CloudAuth, CloudAuthInfo, CloudModelInfo};

/// Storage key for persisted encrypted auth state.
const AUTH_STORAGE_KEY: &str = "cloud_auth";

pub struct AuthManager {
    client: AuthClient,
    state: RwLock<Option<CloudAuth>>,
    storage: Arc<AppStorage>,
    secure_storage: Option<Arc<SecureStorage>>,
}

impl AuthManager {
    /// Create a new AuthManager and restore persisted auth state (if any).
    pub fn new(
        storage: Arc<AppStorage>,
        secure_storage: Option<Arc<SecureStorage>>,
    ) -> Self {
        let mgr = Self {
            client: AuthClient::new(),
            state: RwLock::new(None),
            storage,
            secure_storage,
        };
        mgr
    }

    /// Restore persisted auth state from storage. Call during app init.
    pub async fn restore(&self) {
        match self.load_persisted_auth() {
            Ok(Some(auth)) => {
                // Check if refresh_token is still valid
                if auth.refresh_expires_at > Utc::now() {
                    log::info!(
                        "Restored cloud auth for user '{}' (session_key expires: {})",
                        auth.user.username,
                        auth.session_key_expires_at
                    );
                    *self.state.write().await = Some(auth);
                } else {
                    log::info!("Persisted cloud auth expired (refresh_token), clearing");
                    self.clear_persisted_auth();
                }
            }
            Ok(None) => {
                log::debug!("No persisted cloud auth found");
            }
            Err(e) => {
                log::warn!("Failed to restore cloud auth: {}", e);
                self.clear_persisted_auth();
            }
        }
    }

    /// Login with username and password.
    /// Returns auth info for the frontend.
    pub async fn login(&self, username: &str, password: &str) -> Result<CloudAuthInfo> {
        let auth_resp = self.client.login(username, password).await?;

        let now = Utc::now();
        if auth_resp.expires_in <= 0 || auth_resp.refresh_expires_in <= 0 {
            return Err(anyhow!("服务器返回了无效的令牌有效期"));
        }
        let access_expires = now + chrono::Duration::seconds(auth_resp.expires_in);
        let refresh_expires = now + chrono::Duration::seconds(auth_resp.refresh_expires_in);

        // Create session key
        let sk_resp = self.client.create_session_key(&auth_resp.access_token).await?;
        if sk_resp.expires_in <= 0 {
            return Err(anyhow!("服务器返回了无效的会话密钥有效期"));
        }
        let sk_expires = now + chrono::Duration::seconds(sk_resp.expires_in);

        // Fetch available models
        let models = self.client.list_models(&sk_resp.session_key).await
            .unwrap_or_default();

        let cloud_auth = CloudAuth {
            access_token: auth_resp.access_token,
            access_expires_at: access_expires,
            refresh_token: auth_resp.refresh_token,
            refresh_expires_at: refresh_expires,
            session_key: sk_resp.session_key,
            session_key_expires_at: sk_expires,
            user: auth_resp.user.clone(),
            tenant: auth_resp.tenant.clone(),
        };

        // Persist and store
        self.persist_auth(&cloud_auth);
        *self.state.write().await = Some(cloud_auth);

        Ok(CloudAuthInfo {
            logged_in: true,
            user: Some(auth_resp.user),
            tenant: Some(auth_resp.tenant),
            models,
        })
    }

    /// Logout — clear state and persisted data.
    pub async fn logout(&self) {
        *self.state.write().await = None;
        self.clear_persisted_auth();
        log::info!("Cloud auth logged out");
    }

    /// Check if user is logged in.
    pub async fn is_logged_in(&self) -> bool {
        self.state.read().await.is_some()
    }

    /// Get current auth info for frontend display.
    pub async fn get_auth_info(&self) -> CloudAuthInfo {
        let state = self.state.read().await;
        match state.as_ref() {
            Some(auth) => {
                // Check if refresh token has expired
                if auth.refresh_expires_at <= Utc::now() {
                    drop(state);
                    // Re-check under write lock to avoid clobbering a concurrent fresh login
                    let mut wstate = self.state.write().await;
                    if let Some(current) = wstate.as_ref() {
                        if current.refresh_expires_at <= Utc::now() {
                            *wstate = None;
                            drop(wstate);
                            self.clear_persisted_auth();
                            log::info!("Cloud auth refresh token expired, auto-logged out");
                        }
                    }
                    return CloudAuthInfo {
                        logged_in: false,
                        user: None,
                        tenant: None,
                        models: vec![],
                    };
                }
                CloudAuthInfo {
                    logged_in: true,
                    user: Some(auth.user.clone()),
                    tenant: Some(auth.tenant.clone()),
                    models: vec![], // caller should use get_available_models() separately
                }
            }
            None => CloudAuthInfo {
                logged_in: false,
                user: None,
                tenant: None,
                models: vec![],
            },
        }
    }

    /// Get a valid session key, auto-renewing if needed.
    ///
    /// Renewal chain:
    /// 1. session_key valid → return it
    /// 2. session_key expired, access_token valid → create new session_key
    /// 3. access_token expired, refresh_token valid → refresh → create new session_key
    /// 4. all expired → error (triggers re-login)
    pub async fn get_session_key(&self) -> Result<String> {
        let now = Utc::now();
        // Add 60-second buffer to prevent edge-case expiry during request
        let buffer = chrono::Duration::seconds(60);

        // Fast path: session_key still valid
        {
            let state = self.state.read().await;
            if let Some(auth) = state.as_ref() {
                if auth.session_key_expires_at > now + buffer {
                    return Ok(auth.session_key.clone());
                }
            }
        }

        // Need renewal — acquire write lock
        let mut state = self.state.write().await;
        let auth = state.as_mut().ok_or_else(|| anyhow!("未登录"))?;

        // Double-check after acquiring write lock (another task may have renewed)
        if auth.session_key_expires_at > now + buffer {
            return Ok(auth.session_key.clone());
        }

        log::info!("Session key expired, attempting renewal...");

        // Try to create new session key with current access_token
        if auth.access_expires_at > now + buffer {
            match self.client.create_session_key(&auth.access_token).await {
                Ok(sk_resp) if sk_resp.expires_in > 0 => {
                    auth.session_key = sk_resp.session_key.clone();
                    auth.session_key_expires_at = now + chrono::Duration::seconds(sk_resp.expires_in);
                    self.persist_auth(auth);
                    log::info!("Session key renewed successfully");
                    return Ok(sk_resp.session_key);
                }
                Err(e) => {
                    log::warn!("Failed to create session key: {}", e);
                }
                Ok(_) => {
                    log::warn!("Session key response has invalid expires_in, skipping");
                }
            }
        }

        // Access token expired — try refresh
        if auth.refresh_expires_at > now + buffer {
            log::info!("Access token expired, refreshing...");
            match self.client.refresh_token(&auth.refresh_token).await {
                Ok(auth_resp) if auth_resp.expires_in > 0 && auth_resp.refresh_expires_in > 0 => {
                    auth.access_token = auth_resp.access_token.clone();
                    auth.access_expires_at = now + chrono::Duration::seconds(auth_resp.expires_in);
                    auth.refresh_token = auth_resp.refresh_token;
                    auth.refresh_expires_at = now + chrono::Duration::seconds(auth_resp.refresh_expires_in);
                    auth.user = auth_resp.user;
                    auth.tenant = auth_resp.tenant;

                    // Persist refreshed tokens immediately (even before session_key creation)
                    // so they aren't lost if create_session_key fails
                    self.persist_auth(auth);

                    // Create new session key
                    let sk_resp = self.client.create_session_key(&auth_resp.access_token).await?;
                    if sk_resp.expires_in <= 0 {
                        return Err(anyhow!("服务器返回了无效的会话密钥有效期"));
                    }
                    auth.session_key = sk_resp.session_key.clone();
                    auth.session_key_expires_at = now + chrono::Duration::seconds(sk_resp.expires_in);
                    self.persist_auth(auth);
                    log::info!("Token refreshed and session key renewed");
                    return Ok(sk_resp.session_key);
                }
                Ok(_) => {
                    log::warn!("Token refresh returned invalid TTL, treating as expired");
                }
                Err(e) => {
                    log::warn!("Token refresh failed: {}", e);
                }
            }
        }

        // All tokens expired — clear state, force re-login
        *state = None;
        self.clear_persisted_auth();
        Err(anyhow!("登录已过期，请重新登录"))
    }

    /// Fetch available models from the server.
    pub async fn get_available_models(&self) -> Result<Vec<CloudModelInfo>> {
        let session_key = self.get_session_key().await?;
        self.client.list_models(&session_key).await
    }

    // --- Persistence ---

    fn persist_auth(&self, auth: &CloudAuth) {
        let json = match serde_json::to_string(auth) {
            Ok(j) => j,
            Err(e) => {
                log::error!("Failed to serialize cloud auth: {}", e);
                return;
            }
        };

        let value = if let Some(ref ss) = self.secure_storage {
            match ss.encrypt(&json) {
                Ok(encrypted) => encrypted,
                Err(e) => {
                    log::error!("Failed to encrypt cloud auth: {}", e);
                    return;
                }
            }
        } else {
            json
        };

        if let Err(e) = self.storage.set_setting(AUTH_STORAGE_KEY, &value) {
            log::error!("Failed to persist cloud auth: {}", e);
        }
    }

    fn load_persisted_auth(&self) -> Result<Option<CloudAuth>> {
        let raw = match self.storage.get_setting(AUTH_STORAGE_KEY)? {
            Some(v) if !v.is_empty() => v,
            _ => return Ok(None),
        };

        let json = if let Some(ref ss) = self.secure_storage {
            // Try decryption — if it fails, the data may be plaintext (migration)
            match ss.decrypt(&raw) {
                Ok(decrypted) => decrypted,
                Err(e) => {
                    log::warn!("Decryption failed (trying plaintext fallback): {}", e);
                    raw
                }
            }
        } else {
            raw
        };

        let auth: CloudAuth = serde_json::from_str(&json)
            .map_err(|e| anyhow!("Failed to parse persisted cloud auth: {}", e))?;
        Ok(Some(auth))
    }

    fn clear_persisted_auth(&self) {
        if let Err(e) = self.storage.delete_setting(AUTH_STORAGE_KEY) {
            log::warn!("Failed to clear persisted cloud auth: {}", e);
        }
    }
}
