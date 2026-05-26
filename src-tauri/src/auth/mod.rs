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
pub mod device_id;
pub mod state;

use anyhow::{anyhow, Result};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::storage::crypto::SecureStorage;
use crate::storage::{AiJiaHome, GlobalConfigStore};

use client::{is_auth_unauthorized, AuthClient};
use state::{CloudAuth, CloudAuthInfo, CloudModelInfo};

/// Storage key for persisted encrypted auth state.
const AUTH_STORAGE_KEY: &str = "cloud_auth";

pub struct AuthManager {
    client: AuthClient,
    state: RwLock<Option<CloudAuth>>,
    /// Single-flight serializer for refresh_token network calls.  Without
    /// this, `refresh_auth_info` (drops the read lock before the network
    /// call) and `get_session_key` (holds write lock through the network
    /// call) could fire two `/auth/refresh` requests with the same
    /// `refresh_token`. Server's single-use semantics revoke the token on
    /// the first hit; the second concurrent caller then 401s, and (worse)
    /// any state update from the second caller can overwrite the new
    /// tokens just persisted by the first. Holding this lock around the
    /// entire `read state → server call → persist → commit state` cycle
    /// guarantees only one in-flight refresh per AuthManager, and a
    /// follower re-reads state after acquiring the lock so a freshly
    /// rotated session_key is reused without a redundant server hit. See
    /// commit history for the SLS-confirmed user_id=87 incident
    /// (token c104df42... was rotated to ed8f044d... server-side but the
    /// client never persisted ed8f044d — re-using c104df42 24h later
    /// produced the recurring "API 密钥无效或已过期" symptom).
    refresh_lock: Mutex<()>,
    global_store: Arc<GlobalConfigStore>,
    secure_storage: Option<Arc<SecureStorage>>,
    /// Stable per-install identifier sent to the server when creating
    /// session keys; lets the server collapse repeat creations from the
    /// same desktop install into one slot.  Computed lazily on first
    /// construction via `device_id::load_or_create`.
    device_id: String,
    /// Best-effort throttle to prevent the 401 auto-retry path from
    /// rebuilding the session key faster than every 60s. Stores the last
    /// successful `create_session_key` timestamp.
    last_session_create_at: RwLock<Option<chrono::DateTime<Utc>>>,
}

impl AuthManager {
    /// Create a new AuthManager and restore persisted auth state (if any).
    pub fn new(
        global_store: Arc<GlobalConfigStore>,
        secure_storage: Option<Arc<SecureStorage>>,
        home: &AiJiaHome,
    ) -> Self {
        let device_id = device_id::load_or_create(home);
        log::info!("[AuthManager] device_id={}", device_id);
        Self {
            client: AuthClient::new(),
            state: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            global_store,
            secure_storage,
            device_id,
            last_session_create_at: RwLock::new(None),
        }
    }

    /// Restore persisted auth state from storage. Call during app init.
    pub async fn restore(&self) {
        match self.load_persisted_auth() {
            Ok(Some(auth)) => {
                // Keep the state even if refresh_token is expired — session_key
                // may still be valid, and even if not, `refresh_auth_info` will
                // make a best-effort network call before giving up. Clearing
                // persisted auth here based purely on refresh_token age loses
                // valid session credentials on system clock skew or a stale
                // cached refresh_expires_at.
                log::info!(
                    "Restored cloud auth for user '{}' (access_expires_at={}, refresh_expires_at={}, session_key_expires_at={})",
                    auth.user.username,
                    auth.access_expires_at,
                    auth.refresh_expires_at,
                    auth.session_key_expires_at
                );
                *self.state.write().await = Some(auth);
            }
            Ok(None) => {
                log::debug!("No persisted cloud auth found");
            }
            Err(e) => {
                // Unreadable blob → wipe it and force re-login. Network-layer
                // credentials carry no data worth preserving (session_key has
                // 24h TTL anyway, user/tenant info is re-fetched on login).
                // The previous "preserve and retry" behaviour created an
                // unrecoverable loop for users upgrading from 0.3.x — see
                // `storage::data_version` for the full incident write-up.
                log::warn!(
                    "Failed to restore cloud auth, clearing persisted file (will force re-login): {}",
                    e
                );
                self.clear_persisted_auth();
            }
        }
    }

    /// Login with username and password.
    /// Returns auth info for the frontend.
    pub async fn login(&self, username: &str, password: &str) -> Result<CloudAuthInfo> {
        let auth_resp = self.client.login(username, password).await?;

        let now = Utc::now();
        if auth_resp.access_expires_at <= now || auth_resp.refresh_expires_at <= now {
            return Err(anyhow!("服务器返回了无效的令牌有效期"));
        }

        // Create session key
        let sk_resp = self
            .client
            .create_session_key(&auth_resp.access_token, Some(&self.device_id))
            .await?;
        let sk_expires_at = sk_resp.effective_expires_at();
        if sk_expires_at <= now {
            return Err(anyhow!("服务器返回了无效的会话密钥有效期"));
        }
        *self.last_session_create_at.write().await = Some(Utc::now());

        // Fetch available models
        let models = self
            .client
            .list_models(&sk_resp.key)
            .await
            .unwrap_or_default();

        let user: state::UserInfo = auth_resp.user.into();
        let tenant: state::TenantInfo = auth_resp.tenant.into();

        let cloud_auth = CloudAuth {
            access_token: auth_resp.access_token,
            access_expires_at: auth_resp.access_expires_at,
            refresh_token: auth_resp.refresh_token,
            refresh_expires_at: auth_resp.refresh_expires_at,
            session_key: sk_resp.key,
            session_key_expires_at: sk_expires_at,
            user: user.clone(),
            tenant: tenant.clone(),
        };

        // Persist BEFORE updating in-memory state. If disk fails, return
        // the error and DON'T put the new tokens in memory — otherwise we'd
        // have a session that works until restart, then can't log back in
        // because disk holds nothing.
        self.persist_auth(&cloud_auth)?;
        *self.state.write().await = Some(cloud_auth);

        Ok(CloudAuthInfo {
            logged_in: true,
            user: Some(user),
            tenant: Some(tenant),
            models,
        })
    }

    /// Send an SMS verification code for registration. No auth required.
    pub async fn send_sms_code(&self, phone: &str) -> Result<()> {
        self.client.send_sms_code(phone).await
    }

    /// Send an email verification code for registration. No auth required.
    pub async fn send_email_code(&self, email: &str) -> Result<()> {
        self.client.send_email_code(email).await
    }

    /// Register a personal account. After success the caller is expected to
    /// call `login(...)` separately (we don't auto-login here because the
    /// post-register handshake currently requires the same username/password
    /// path as a regular login so session_key + scope activation happen via
    /// the existing `cloud_login` flow).
    pub async fn register(
        &self,
        method: &str,
        phone: &str,
        email: &str,
        code: &str,
        password: &str,
        name: &str,
    ) -> Result<()> {
        self.client
            .register(method, phone, email, code, password, name)
            .await
    }

    /// Logout — call server API then clear local state and persisted data.
    pub async fn logout(&self) {
        // Best-effort server-side logout
        if let Some(auth) = self.state.read().await.as_ref() {
            let _ = self.client.logout(&auth.access_token).await;
        }
        *self.state.write().await = None;
        self.clear_persisted_auth();
        log::info!("Cloud auth logged out");
    }

    /// Change password on the server.
    /// After success, clears local auth state (forces re-login).
    pub async fn change_password(&self, old_password: &str, new_password: &str) -> Result<()> {
        let access_token = {
            let state = self.state.read().await;
            let auth = state.as_ref().ok_or_else(|| anyhow!("未登录"))?;
            auth.access_token.clone()
        };
        self.client
            .change_password(&access_token, old_password, new_password)
            .await?;
        // Server revoked all refresh tokens; clear local state
        *self.state.write().await = None;
        self.clear_persisted_auth();
        Ok(())
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
                // We previously cleared state here when `refresh_expires_at <= now`
                // and called this an auto-logout. That's wrong: the refresh
                // token expiry is just one of three credentials we hold, and
                // session_key may still be valid for hours. Even when all
                // three timestamps are stale, an offline / network-limited
                // user opening the app should not be silently logged out —
                // we should still surface their identity from the persisted
                // record so the UI can show "you" and let an explicit network
                // call (chat send, etc.) trigger renewal or failure.
                //
                // Renewal logic for stale credentials lives in
                // `get_session_key` and `refresh_auth_info`, which are the
                // only paths that should ever clear state.
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

    /// Refresh auth info from server using refresh_token and persist the latest tenant/user profile.
    /// Best-effort: if refresh fails, falls back to current persisted auth info.
    ///
    /// Single-flight via `refresh_lock`: concurrent callers (typically
    /// `refresh_auth_info` from a startup task and `get_session_key` from
    /// the first chat send) serialize through one in-flight refresh. The
    /// follower re-reads state after acquiring the lock and skips the
    /// server hit if the leader already refreshed it.
    pub async fn refresh_auth_info(&self) -> CloudAuthInfo {
        // Single-flight: only one refresh op at a time.  Holding this for
        // the whole network round-trip is fine — refresh_auth_info is a
        // foreground op invoked at most once per user-visible event.
        let _refresh_guard = self.refresh_lock.lock().await;

        let (refresh_token, session_key) = {
            let state = self.state.read().await;
            match state.as_ref() {
                Some(auth) => (
                    if auth.refresh_expires_at > Utc::now() {
                        Some(auth.refresh_token.clone())
                    } else {
                        None
                    },
                    if auth.session_key_expires_at > Utc::now() {
                        Some(auth.session_key.clone())
                    } else {
                        None
                    },
                ),
                _ => return self.get_auth_info().await,
            }
        };

        // Strategy 1: use refresh_token to get new tokens + latest profile
        if let Some(rt) = refresh_token {
            match self.client.refresh_token(&rt).await {
                Ok(auth_resp) => {
                    let now = Utc::now();

                    let user: state::UserInfo = auth_resp.user.into();
                    let tenant: state::TenantInfo = auth_resp.tenant.into();

                    // Persist refreshed tokens IMMEDIATELY — the old refresh_token is
                    // already revoked (single-use), so we must not lose the new one even
                    // if create_session_key fails below.
                    let mut cloud_auth = CloudAuth {
                        access_token: auth_resp.access_token,
                        access_expires_at: auth_resp.access_expires_at,
                        refresh_token: auth_resp.refresh_token,
                        refresh_expires_at: auth_resp.refresh_expires_at,
                        // Keep existing session key for now — will update below if possible
                        session_key: String::new(),
                        session_key_expires_at: now,
                        user: user.clone(),
                        tenant: tenant.clone(),
                    };

                    // Copy existing session key from current state as fallback
                    {
                        let state = self.state.read().await;
                        if let Some(current) = state.as_ref() {
                            cloud_auth.session_key = current.session_key.clone();
                            cloud_auth.session_key_expires_at = current.session_key_expires_at;
                        }
                    }

                    // C-fix: only spin up a fresh session_key when the
                    // currently-cached one is missing or already expired.
                    // Repeatedly calling create_session_key on every app
                    // launch was consuming a 10-active-key slot per device
                    // per startup (see decision in spec).
                    let needs_new_session = cloud_auth.session_key.is_empty()
                        || cloud_auth.session_key_expires_at <= now;
                    if needs_new_session {
                        match self
                            .client
                            .create_session_key(&cloud_auth.access_token, Some(&self.device_id))
                            .await
                        {
                            Ok(sk) => {
                                let sk_expires = sk.effective_expires_at();
                                if sk_expires > now {
                                    cloud_auth.session_key = sk.key;
                                    cloud_auth.session_key_expires_at = sk_expires;
                                    *self.last_session_create_at.write().await = Some(Utc::now());
                                } else {
                                    log::warn!("refresh_auth_info: server returned expired session key, keeping existing");
                                }
                            }
                            Err(e) => {
                                log::warn!("refresh_auth_info: create_session_key failed: {}, keeping existing", e);
                            }
                        }
                    } else {
                        log::info!(
                            "[refresh_auth_info] keeping existing session_key (expires_at={}); skipping create",
                            cloud_auth.session_key_expires_at
                        );
                    }

                    // PERSIST FIRST — if disk write fails, do NOT update in-memory.
                    // That way next launch's `restore()` reads the old (still-valid)
                    // disk state and the user keeps working; the refreshed-but-lost
                    // tokens just stay lost server-side until the next refresh.
                    // Letting in-memory diverge from disk created the user_id=87
                    // class of incidents.
                    if let Err(e) = self.persist_auth(&cloud_auth) {
                        log::error!(
                            "refresh_auth_info: persist failed, NOT committing new tokens to memory: {}",
                            e
                        );
                        // Fall through to Strategy 2 / fallback path.
                    } else {
                        *self.state.write().await = Some(cloud_auth);
                        return CloudAuthInfo {
                            logged_in: true,
                            user: Some(user),
                            tenant: Some(tenant),
                            models: vec![],
                        };
                    }
                }
                Err(e) => {
                    // CLAUDE.md decision 11: only HTTP 401 means the refresh
                    // token is irrecoverable; everything else is transient
                    // (network blip, 5xx, etc) and we must NOT wipe state.
                    if is_auth_unauthorized(&e) {
                        log::warn!(
                            "refresh_auth_info: refresh_token revoked (HTTP 401): {}",
                            e
                        );
                    } else if session_key.is_some() {
                        log::debug!(
                            "refresh_auth_info: refresh_token failed (transient, will fall back to session_key): {}",
                            e
                        );
                    } else {
                        log::warn!("refresh_auth_info: refresh_token failed (transient): {}", e);
                    }
                }
            }
        }

        // Strategy 2: refresh_token unavailable/failed — use session_key to fetch profile only
        if let Some(sk) = session_key {
            match self.client.get_profile(&sk).await {
                Ok((user_info, tenant_info)) => {
                    let user: state::UserInfo = user_info.into();
                    let tenant: state::TenantInfo = tenant_info.into();
                    // Update only user/tenant in persisted state (keep existing tokens).
                    // Build a fresh CloudAuth snapshot, persist it first, then commit.
                    // If persist fails, leave in-memory state alone — we don't want
                    // memory and disk to diverge over a profile-update cosmetic change.
                    let snapshot = {
                        let state = self.state.read().await;
                        state.as_ref().map(|auth| CloudAuth {
                            user: user.clone(),
                            tenant: tenant.clone(),
                            ..auth.clone()
                        })
                    };
                    if let Some(updated) = snapshot {
                        match self.persist_auth(&updated) {
                            Ok(()) => {
                                *self.state.write().await = Some(updated);
                            }
                            Err(e) => {
                                log::warn!(
                                    "refresh_auth_info: profile-only persist failed (keeping in-memory): {}",
                                    e
                                );
                            }
                        }
                    }
                    return CloudAuthInfo {
                        logged_in: true,
                        user: Some(user),
                        tenant: Some(tenant),
                        models: vec![],
                    };
                }
                Err(e) => {
                    log::warn!("refresh_auth_info: get_profile failed: {}", e);
                }
            }
        }

        // Strategy 3: all failed — return persisted data as-is
        self.get_auth_info().await
    }

    /// Get a valid session key, auto-renewing if needed.
    ///
    /// Renewal chain:
    /// 1. session_key valid → return it
    /// 2. session_key expired, access_token valid → create new session_key
    /// 3. access_token expired, refresh_token valid → refresh → create new session_key
    /// 4. all expired or server says "revoked" (HTTP 401) → clear state, force re-login
    /// 5. transient failure (network / 5xx) → keep state, surface error so caller retries later
    ///
    /// Concurrent refresh-side calls (this one + `refresh_auth_info`) serialize
    /// through `refresh_lock`, so a concurrent pair never produces a 401-back-from-server
    /// from one call's single-use revocation hitting the other call's still-in-flight
    /// request with the same refresh_token.
    pub async fn get_session_key(&self) -> Result<String> {
        let now = Utc::now();
        // Add 60-second buffer to prevent edge-case expiry during request
        let buffer = chrono::Duration::seconds(60);

        // Fast path: session_key still valid (read lock only)
        {
            let state = self.state.read().await;
            if let Some(auth) = state.as_ref() {
                if auth.session_key_expires_at > now + buffer {
                    log::info!(
                        "[get_session_key] using cached session_key (len={}, expires_at={})",
                        auth.session_key.len(),
                        auth.session_key_expires_at
                    );
                    return Ok(auth.session_key.clone());
                }
            }
        }

        // Slow path: need to renew → acquire single-flight refresh lock.
        // A concurrent caller (refresh_auth_info / another get_session_key)
        // could already have refreshed by the time we get here; we re-check
        // state below before doing a server hit.
        let _refresh_guard = self.refresh_lock.lock().await;

        // Re-read state under read lock. Snapshot the fields we need so we
        // don't hold the read lock across the network call.
        let snapshot = {
            let state = self.state.read().await;
            let Some(auth) = state.as_ref() else {
                return Err(anyhow!("未登录"));
            };
            // Race-winner double-check: another concurrent caller already
            // refreshed for us. Return their session_key.
            if auth.session_key_expires_at > now + buffer {
                return Ok(auth.session_key.clone());
            }
            auth.clone()
        };

        log::info!("Session key expired, attempting renewal...");

        // B-fix: throttle session_key creation to avoid retry loops where a
        // 401 retry triggers a fresh key, which in turn revokes another
        // device's key, which 401-retries, etc. Only allow one create per
        // 60s; below that, surface the cached key (the caller's 401 path
        // is the next gate — it will give up if still 401).
        let throttle_window = chrono::Duration::seconds(60);
        if let Some(last) = *self.last_session_create_at.read().await {
            if Utc::now() - last < throttle_window {
                log::warn!(
                    "[get_session_key] suppressing create_session_key — last create was {}s ago (throttle={}s)",
                    (Utc::now() - last).num_seconds(),
                    throttle_window.num_seconds()
                );
                return Ok(snapshot.session_key);
            }
        }

        // Track whether server explicitly told us "this credential is dead".
        // Only such a signal warrants clearing state (decision §11 — judge
        // revocation by HTTP status, not by absence of success path).
        let mut auth_revoked_by_server = false;

        // Try to create new session key with current access_token.
        if snapshot.access_expires_at > now + buffer {
            match self
                .client
                .create_session_key(&snapshot.access_token, Some(&self.device_id))
                .await
            {
                Ok(sk_resp) => {
                    let sk_expires = sk_resp.effective_expires_at();
                    if sk_expires > now {
                        // Build new state from snapshot + new session_key.
                        let mut updated = snapshot.clone();
                        updated.session_key = sk_resp.key.clone();
                        updated.session_key_expires_at = sk_expires;
                        self.persist_auth(&updated)?;
                        *self.state.write().await = Some(updated);
                        *self.last_session_create_at.write().await = Some(Utc::now());
                        log::info!(
                            "[get_session_key] renewed via access_token (len={}, expires_at={})",
                            sk_resp.key.len(),
                            sk_expires
                        );
                        return Ok(sk_resp.key);
                    } else {
                        log::warn!("Session key response has invalid expires_at, skipping");
                    }
                }
                Err(e) => {
                    if is_auth_unauthorized(&e) {
                        log::warn!("create_session_key returned 401 (access_token rejected): {}", e);
                        auth_revoked_by_server = true;
                    } else {
                        log::warn!("Failed to create session key (transient): {}", e);
                    }
                }
            }
        }

        // Access token expired or rejected — try refresh.
        if snapshot.refresh_expires_at > now + buffer {
            log::info!("Access token expired, refreshing...");
            match self.client.refresh_token(&snapshot.refresh_token).await {
                Ok(auth_resp)
                    if auth_resp.access_expires_at > now && auth_resp.refresh_expires_at > now =>
                {
                    let new_access_token = auth_resp.access_token.clone();
                    let mut updated = CloudAuth {
                        access_token: auth_resp.access_token,
                        access_expires_at: auth_resp.access_expires_at,
                        refresh_token: auth_resp.refresh_token,
                        refresh_expires_at: auth_resp.refresh_expires_at,
                        // Keep prior session_key as fallback; replaced below
                        // on successful create_session_key.
                        session_key: snapshot.session_key.clone(),
                        session_key_expires_at: snapshot.session_key_expires_at,
                        user: auth_resp.user.into(),
                        tenant: auth_resp.tenant.into(),
                    };

                    // PERSIST the rotated tokens before doing anything else.
                    // The old refresh_token has just been revoked server-side
                    // (single-use); if we lose the new one in a crash here we
                    // have no way back. This was the root of the user_id=87
                    // SLS incident (server had ed8f044d, client kept c104df42).
                    self.persist_auth(&updated)?;
                    *self.state.write().await = Some(updated.clone());

                    // Create new session key
                    let sk_resp = self
                        .client
                        .create_session_key(&new_access_token, Some(&self.device_id))
                        .await?;
                    let sk_expires = sk_resp.effective_expires_at();
                    if sk_expires <= now {
                        return Err(anyhow!("服务器返回了无效的会话密钥有效期"));
                    }
                    *self.last_session_create_at.write().await = Some(Utc::now());
                    updated.session_key = sk_resp.key.clone();
                    updated.session_key_expires_at = sk_expires;
                    self.persist_auth(&updated)?;
                    *self.state.write().await = Some(updated);
                    log::info!("Token refreshed and session key renewed");
                    return Ok(sk_resp.key);
                }
                Ok(_) => {
                    log::warn!("Token refresh returned invalid TTL, treating as expired");
                    auth_revoked_by_server = true;
                }
                Err(e) => {
                    if is_auth_unauthorized(&e) {
                        log::warn!("Token refresh returned HTTP 401 (refresh_token revoked): {}", e);
                        auth_revoked_by_server = true;
                    } else {
                        log::warn!("Token refresh failed (transient, keeping state): {}", e);
                    }
                }
            }
        }

        // Decide: did the server actively reject us, or was this a transient
        // failure?  Only the former clears state.  Wiping on a transient
        // (network blip / 5xx) used to silently log out an otherwise-fine
        // user — they then either re-login (annoying) or, worse, hit the
        // disk-cleared state across a restart and lose all conversations
        // they hadn't pushed up.
        if auth_revoked_by_server {
            *self.state.write().await = None;
            self.clear_persisted_auth();
            Err(anyhow!("登录已过期，请重新登录"))
        } else {
            Err(anyhow!(
                "无法获取会话密钥，请稍后重试（网络或服务器暂时不可用）"
            ))
        }
    }

    /// Fetch available models from the server.
    pub async fn get_available_models(&self) -> Result<Vec<CloudModelInfo>> {
        let session_key = self.get_session_key().await?;
        self.client.list_models(&session_key).await
    }

    /// Fetch personal-tenant billing summary.
    pub async fn get_billing_summary(
        &self,
    ) -> Result<crate::transport::tauri_commands::billing::BillingSummary> {
        let session_key = self.get_session_key().await?;
        self.client.get_billing_summary(&session_key).await
    }

    /// Fetch a page of personal-tenant usage records.
    pub async fn get_billing_usage_records(
        &self,
        page: u32,
        size: u32,
    ) -> Result<crate::transport::tauri_commands::billing::UsageRecordsPage> {
        let session_key = self.get_session_key().await?;
        self.client
            .get_billing_usage_records(&session_key, page, size)
            .await
    }

    /// Force-invalidate the cached session key so the next `get_session_key`
    /// call goes through the renewal path. Called by gateway when a 401
    /// "Session key revoked" comes back — at that point the local
    /// `session_key_expires_at` cannot be trusted (clock skew / server-side
    /// revoke / tz bugs), so we drop it and let the renewal chain rebuild.
    /// Idempotent; no-op when no auth state.
    pub async fn invalidate_session_key(&self) {
        let mut state = self.state.write().await;
        let Some(auth) = state.as_mut() else { return };
        // Set expires_at to the past so fast-path miss triggers renewal.
        auth.session_key_expires_at = Utc::now() - chrono::Duration::seconds(1);
        log::info!(
            "[invalidate_session_key] session_key marked expired; next get_session_key will refresh"
        );
        // Best-effort persist of the invalidation marker. Failure here only
        // means the in-memory invalidation works for this run; next startup
        // would reload the cached (now-expired) value from disk and hit the
        // same fast-path miss. Either way `get_session_key` re-derives.
        if let Err(e) = self.persist_auth(auth) {
            log::warn!("[invalidate_session_key] persist failed (non-fatal): {}", e);
        }
    }

    // --- Persistence ---

    /// Serialize + encrypt + write auth state to disk. Returns `Err` on any
    /// failure so callers can refuse to commit the in-memory state (and thus
    /// not lose the new refresh_token in memory-only). Previously this swallowed
    /// errors with `log::error!`; that hid the case where a successful
    /// `/auth/refresh` returned new tokens but they never reached disk —
    /// next launch then loaded the OLD (already revoked) refresh_token and
    /// the user hit "API 密钥无效或已过期" on every retry.
    fn persist_auth(&self, auth: &CloudAuth) -> Result<()> {
        let json = serde_json::to_string(auth)
            .map_err(|e| anyhow!("Failed to serialize cloud auth: {}", e))?;

        let value = if let Some(ref ss) = self.secure_storage {
            ss.encrypt(&json)
                .map_err(|e| anyhow!("Failed to encrypt cloud auth: {}", e))?
        } else {
            json
        };

        self.global_store
            .set_setting(AUTH_STORAGE_KEY, &value)
            .map_err(|e| anyhow!("Failed to persist cloud auth: {}", e))
    }

    fn load_persisted_auth(&self) -> Result<Option<CloudAuth>> {
        let raw = match self.global_store.get_setting(AUTH_STORAGE_KEY)? {
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
        if let Err(e) = self.global_store.delete_setting(AUTH_STORAGE_KEY) {
            log::warn!("Failed to clear persisted cloud auth: {}", e);
        }
    }
}
