# IM Channel Domain Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the current DingTalk-only IM channel implementation into a future-proof Channel Domain with separated configuration, enabled intent, and runtime connection state.

**Architecture:** The backend becomes the source of truth via `ChannelPlatformState` and a user-scoped `ChannelConfigStore`. DingTalk remains the only available runtime, while Feishu/WeChat/WeCom are represented as coming-soon platform states. The frontend consumes platform states through `channelStore`, renders cards from the domain model, and uses explicit commands for registration, enable/disable, remove, and AppSecret reveal.

**Tech Stack:** Rust/Tauri v2, serde JSON persistence, existing `SecureStorage`, Tokio cancellation, React 19, Zustand, Vitest, Testing Library, existing Radix dialog/confirm primitives.

---

## File Structure

### Backend

- Modify: `src-tauri/src/storage/user_scoped_paths.rs`
  - Add platform subdirectory helpers: `channel_platform_dir`, `channel_platform_config_path`, `channel_platform_sessions_path`.
  - Keep existing flat helpers only if needed by other code, but new channel code must not use them.

- Modify: `src-tauri/src/connector/channel/types.rs`
  - Replace the current DingTalk-only config/status surface with domain types.
  - Add `Platform` variants for `Dingtalk`, `Feishu`, `Wechat`, `Wecom`.
  - Add `ChannelCapability`, `ChannelConnectionState`, `ChannelConfigView`, `RobotCodeSource`, `DingtalkStoredConfig`, `ChannelPlatformState`, and `ChannelPlatformStatePayload`.
  - Keep `ChannelConversation`, `ConversationType`, `ChannelMessage`, `ChannelRegistrationBeginResult`, and registration poll types, but extend poll success result with a config view/platform state.

- Create: `src-tauri/src/connector/channel/config_store.rs`
  - Own user-scoped channel config persistence.
  - Own masking and reveal of AppSecret.
  - Own `enabled` updates, removal, and platform state snapshots.
  - Return coming-soon platform states for unsupported platforms.

- Modify: `src-tauri/src/connector/channel/router.rs`
  - Update tests to use `channels/dingtalk/sessions.json`.
  - No behavior change needed beyond path expectations.

- Modify: `src-tauri/src/connector/channel/manager.rs`
  - Use `ChannelConfigStore` and platform states.
  - Replace flat `dingtalk_config.json` and `dingtalk_sessions.json` paths with new platform subdirectory paths.
  - Add methods: `get_platforms`, `get_platform`, `set_enabled`, `remove_platform`, `reveal_secret`.
  - Convert stream status callbacks into `ChannelPlatformState` events.
  - Stop stream and emit disconnected state when disabled or removed.

- Modify: `src-tauri/src/commands/channel.rs`
  - Replace old status/save commands with domain commands.
  - Keep old command wrappers only if tests still need them; otherwise remove frontend usage.

- Modify: `src-tauri/src/connector/channel/mod.rs`
  - Export new store and domain types.

- Modify: `src-tauri/src/lib.rs`
  - Register new channel commands.
  - Continue initializing `ChannelManager` only when current user paths are available.

### Frontend

- Modify: `src/lib/tauri.ts`
  - Replace channel status types with platform domain types.
  - Add `CHANNEL_PLATFORM_STATE` event constant.
  - Add wrappers for new IPC commands.
  - Keep `channel:message` wrapper.

- Modify: `src/stores/channelStore.ts`
  - Store platform map instead of `dingtalkStatus`.
  - Add domain actions: `loadPlatforms`, `loadPlatform`, `beginRegistration`, `pollRegistration`, `setEnabled`, `removePlatform`, `revealSecret`, `loadConversations`.
  - Subscribe to `channel:platform-state`.
  - Never store revealed AppSecret.

- Modify: `src/features/channel/ChannelConfig.tsx`
  - Convert to registration-mode component.
  - Use store actions and new registration command names.
  - Show success summary with AppKey, masked AppSecret, and RobotCode.

- Create: `src/features/channel/ChannelConfigDetails.tsx`
  - Read-only config details dialog content.
  - Does not start registration.
  - Reveals AppSecret only after `requestConfirm` returns true.
  - Clears revealed secret on unmount/close.

- Modify: `src/features/channel/ChannelPage.tsx`
  - Render cards from `ChannelPlatformState`.
  - Hide switch/more menu when `configured=false`.
  - Connected/configured `配置` opens `ChannelConfigDetails`.
  - `移除` uses `requestConfirm` and `removePlatform`.
  - Switch uses `setEnabled` only.

- Modify tests:
  - `src/features/channel/ChannelConfig.test.tsx`
  - `src/features/channel/ChannelPage.test.tsx`
  - Create `src/features/channel/ChannelConfigDetails.test.tsx`
  - Create or modify `src/stores/channelStore.test.ts` if store tests are added.

---

## Task 1: Backend Domain Types and User-scoped Paths

**Files:**
- Modify: `src-tauri/src/storage/user_scoped_paths.rs`
- Modify: `src-tauri/src/connector/channel/types.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`

- [ ] **Step 1: Add failing user-scoped path assertions**

Edit `src-tauri/src/storage/user_scoped_paths.rs` in `#[cfg(test)] mod tests` and add this test:

```rust
    #[test]
    fn channel_platform_paths_are_nested_under_user_scope() {
        let root = PathBuf::from("/tmp/test-renlijia");
        let paths = UserScopedPaths::new(&root, "t_1__u_2");
        let base = root.join("users/t_1__u_2/channels/dingtalk");

        assert_eq!(paths.channel_platform_dir("dingtalk"), base);
        assert_eq!(
            paths.channel_platform_config_path("dingtalk"),
            base.join("config.json")
        );
        assert_eq!(
            paths.channel_platform_sessions_path("dingtalk"),
            base.join("sessions.json")
        );
    }
```

- [ ] **Step 2: Run the path test and verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml storage::user_scoped_paths::tests::channel_platform_paths_are_nested_under_user_scope --lib
```

Expected: FAIL with methods like `channel_platform_dir` not found.

- [ ] **Step 3: Implement platform path helpers**

In `impl UserScopedPaths` in `src-tauri/src/storage/user_scoped_paths.rs`, add:

```rust
    pub fn channel_platform_dir(&self, platform: &str) -> PathBuf {
        self.channels_dir().join(platform)
    }

    pub fn channel_platform_config_path(&self, platform: &str) -> PathBuf {
        self.channel_platform_dir(platform).join("config.json")
    }

    pub fn channel_platform_sessions_path(&self, platform: &str) -> PathBuf {
        self.channel_platform_dir(platform).join("sessions.json")
    }
```

Do not remove existing `channel_config_path` or `channel_sessions_path` in this step.

- [ ] **Step 4: Run the path test and verify it passes**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml storage::user_scoped_paths::tests::channel_platform_paths_are_nested_under_user_scope --lib
```

Expected: PASS.

- [ ] **Step 5: Extend `types.rs` with domain types while preserving compile compatibility**

Replace `src-tauri/src/connector/channel/types.rs` with this content. It intentionally keeps the legacy `ChannelStatus`, `ChannelStatusPayload`, `DingtalkChannelConfig`, and old registration poll result shape until Task 3 updates the manager and command layer.

```rust
use serde::{Deserialize, Serialize};

/// IM platform identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Dingtalk,
    Feishu,
    Wechat,
    Wecom,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Dingtalk => "dingtalk",
            Platform::Feishu => "feishu",
            Platform::Wechat => "wechat",
            Platform::Wecom => "wecom",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dingtalk" => Some(Platform::Dingtalk),
            "feishu" => Some(Platform::Feishu),
            "wechat" => Some(Platform::Wechat),
            "wecom" => Some(Platform::Wecom),
            _ => None,
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Dingtalk, Self::Feishu, Self::Wechat, Self::Wecom]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelCapability {
    Available,
    ComingSoon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelConnectionState {
    Unconfigured,
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    ConfigError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RobotCodeSource {
    Registration,
    AppKeyFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfigView {
    pub platform: Platform,
    pub app_key: String,
    pub app_secret_masked: String,
    pub robot_code: String,
    pub robot_code_source: RobotCodeSource,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPlatformState {
    pub platform: Platform,
    pub capability: ChannelCapability,
    pub configured: bool,
    pub enabled: bool,
    pub connection: ChannelConnectionState,
    pub config: Option<ChannelConfigView>,
    pub last_connected_at: Option<String>,
    pub last_error: Option<String>,
}

impl ChannelPlatformState {
    pub fn coming_soon(platform: Platform) -> Self {
        Self {
            platform,
            capability: ChannelCapability::ComingSoon,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPlatformStatePayload {
    pub state: ChannelPlatformState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredCredentials {
    pub app_key: String,
    /// Encrypted AppSecret. Falls back to plaintext only when SecureStorage is unavailable.
    pub app_secret_encrypted: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredBot {
    pub robot_code: String,
    pub robot_code_source: RobotCodeSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredRegistration {
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredMetadata {
    pub created_at: String,
    pub updated_at: String,
}

/// DingTalk channel config stored at users/<scope>/channels/dingtalk/config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredConfig {
    pub schema_version: u32,
    pub platform: Platform,
    pub configured: bool,
    pub enabled: bool,
    pub credentials: DingtalkStoredCredentials,
    pub bot: DingtalkStoredBot,
    pub registration: DingtalkStoredRegistration,
    pub metadata: DingtalkStoredMetadata,
}

// Legacy DingTalk config/status types kept until Task 3 rewires manager/commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkChannelConfig {
    pub app_key: String,
    pub app_secret_encrypted: String,
    pub robot_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ChannelStatus {
    Unconfigured,
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { delay_secs: u64 },
    ConfigError { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatusPayload {
    pub platform: String,
    pub status: ChannelStatus,
}

/// DingTalk OPEN_CLAW registration session begin result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRegistrationBeginResult {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri_complete: String,
    pub verification_uri: String,
    pub interval_seconds: u64,
    pub expires_in_seconds: u64,
    pub source: String,
}

/// Legacy poll result shape. Task 3 extends this after manager/commands are rewired.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRegistrationPollResult {
    pub state: ChannelRegistrationPollState,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub robot_code: Option<String>,
    pub fail_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChannelRegistrationPollState {
    Waiting,
    Success,
    Fail,
    Expired,
    Unknown,
}

/// Channel conversations are internal Lotus sessions backed by an external IM conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConversation {
    pub session_id: String,
    pub platform: Platform,
    pub conversation_type: ConversationType,
    pub external_id: String,
    pub display_name: String,
    pub unread_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConversationType {
    Group,
    Private,
}

/// One parsed message from DingTalk Stream.
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub msg_id: String,
    pub conversation_type: ConversationType,
    pub conversation_key: String,
    pub sender_id: String,
    pub sender_nick: String,
    pub text: String,
    pub robot_code: String,
    pub reply_group_id: String,
    pub app_key: String,
    pub app_secret: String,
}

/// channel:message event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMessagePayload {
    pub platform: String,
    pub session_id: String,
    pub sender_nick: String,
    pub text_preview: String,
}
```

- [ ] **Step 6: Update module exports**

In `src-tauri/src/connector/channel/mod.rs`, replace the `pub use types::{...}` block with:

```rust
pub use types::{
    ChannelCapability, ChannelConfigView, ChannelConnectionState, ChannelConversation,
    ChannelMessagePayload, ChannelPlatformState, ChannelPlatformStatePayload,
    ChannelRegistrationBeginResult, ChannelRegistrationPollResult, ChannelRegistrationPollState,
    DingtalkStoredConfig, Platform, RobotCodeSource,
};
```

- [ ] **Step 7: Run check and verify compatibility**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS. Task 1 preserves legacy channel status/config types so the current manager keeps compiling while new domain types are introduced.

- [ ] **Step 8: Commit Task 1**

```bash
git add src-tauri/src/storage/user_scoped_paths.rs src-tauri/src/connector/channel/types.rs src-tauri/src/connector/channel/mod.rs
git commit -m "refactor(channel): define platform domain types"
```

---

## Task 2: Backend ChannelConfigStore

**Files:**
- Create: `src-tauri/src/connector/channel/config_store.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`
- Test: `src-tauri/src/connector/channel/config_store.rs`

- [ ] **Step 1: Create failing config store tests**

Create `src-tauri/src/connector/channel/config_store.rs` with this test-first scaffold:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::storage::crypto::SecureStorage;

use super::dingtalk_registration::OPEN_CLAW_SOURCE;
use super::types::{
    ChannelCapability, ChannelConfigView, ChannelConnectionState, ChannelPlatformState,
    DingtalkStoredBot, DingtalkStoredConfig, DingtalkStoredCredentials,
    DingtalkStoredMetadata, DingtalkStoredRegistration, Platform, RobotCodeSource,
};

#[derive(Clone)]
pub struct ChannelConfigStore {
    channels_dir: PathBuf,
    secure_storage: Option<Arc<SecureStorage>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store_in(dir: &TempDir) -> ChannelConfigStore {
        ChannelConfigStore::new(dir.path().join("channels"), None)
    }

    #[test]
    fn dingtalk_config_path_uses_platform_subdirectory() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        assert_eq!(
            store.dingtalk_config_path(),
            dir.path().join("channels/dingtalk/config.json")
        );
    }

    #[test]
    fn save_registration_writes_enabled_config_and_masks_secret() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);

        let state = store
            .save_dingtalk_registration("ding-app-key".into(), "super-secret-value".into(), None)
            .unwrap();

        assert!(state.configured);
        assert!(state.enabled);
        assert_eq!(state.connection, ChannelConnectionState::Disconnected);
        let view = state.config.unwrap();
        assert_eq!(view.app_key, "ding-app-key");
        assert_eq!(view.robot_code, "ding-app-key");
        assert_eq!(view.robot_code_source, RobotCodeSource::AppKeyFallback);
        assert_eq!(view.app_secret_masked, "••••••••••••alue");

        let persisted = std::fs::read_to_string(store.dingtalk_config_path()).unwrap();
        assert!(persisted.contains("ding-app-key"));
        assert!(persisted.contains("super-secret-value"), "plaintext fallback is expected when SecureStorage is unavailable");
    }

    #[test]
    fn save_registration_preserves_returned_robot_code() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);

        let state = store
            .save_dingtalk_registration(
                "ding-app-key".into(),
                "super-secret-value".into(),
                Some("robot-001".into()),
            )
            .unwrap();

        let view = state.config.unwrap();
        assert_eq!(view.robot_code, "robot-001");
        assert_eq!(view.robot_code_source, RobotCodeSource::Registration);
    }

    #[test]
    fn reveal_secret_returns_plain_secret_and_store_never_puts_it_in_state() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store
            .save_dingtalk_registration("ding-app-key".into(), "super-secret-value".into(), None)
            .unwrap();

        let state = store.dingtalk_state(ChannelConnectionState::Disconnected, None).unwrap();
        let view = state.config.unwrap();
        assert_eq!(view.app_secret_masked, "••••••••••••alue");
        assert!(!serde_json::to_string(&view).unwrap().contains("super-secret-value"));
        assert_eq!(store.reveal_dingtalk_secret().unwrap(), "super-secret-value");
    }

    #[test]
    fn set_enabled_false_keeps_config() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store
            .save_dingtalk_registration("ding-app-key".into(), "super-secret-value".into(), None)
            .unwrap();

        let state = store.set_dingtalk_enabled(false).unwrap();
        assert!(state.configured);
        assert!(!state.enabled);
        assert_eq!(state.connection, ChannelConnectionState::Disconnected);
        assert!(store.dingtalk_config_path().exists());
    }

    #[test]
    fn remove_dingtalk_deletes_config_and_returns_unconfigured_state() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store
            .save_dingtalk_registration("ding-app-key".into(), "super-secret-value".into(), None)
            .unwrap();

        let state = store.remove_dingtalk().unwrap();
        assert!(!state.configured);
        assert!(!state.enabled);
        assert_eq!(state.connection, ChannelConnectionState::Unconfigured);
        assert!(!store.dingtalk_config_path().exists());
    }

    #[test]
    fn coming_soon_platform_state_is_not_configured() {
        let state = ChannelConfigStore::coming_soon_state(Platform::Feishu);
        assert_eq!(state.capability, ChannelCapability::ComingSoon);
        assert!(!state.configured);
        assert!(!state.enabled);
        assert_eq!(state.connection, ChannelConnectionState::Unconfigured);
    }
}
```

- [ ] **Step 2: Register module so tests compile toward missing methods**

Add this line to `src-tauri/src/connector/channel/mod.rs`:

```rust
pub mod config_store;
```

Also add this export:

```rust
pub use config_store::ChannelConfigStore;
```

- [ ] **Step 3: Run config store tests and verify failure**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml connector::channel::config_store::tests --lib
```

Expected: FAIL with missing `ChannelConfigStore::new`, `dingtalk_config_path`, and other methods.

- [ ] **Step 4: Implement ChannelConfigStore**

In `src-tauri/src/connector/channel/config_store.rs`, add this implementation above the test module:

```rust
impl ChannelConfigStore {
    pub fn new(channels_dir: PathBuf, secure_storage: Option<Arc<SecureStorage>>) -> Self {
        Self { channels_dir, secure_storage }
    }

    pub fn dingtalk_dir(&self) -> PathBuf {
        self.channels_dir.join("dingtalk")
    }

    pub fn dingtalk_config_path(&self) -> PathBuf {
        self.dingtalk_dir().join("config.json")
    }

    pub fn dingtalk_sessions_path(&self) -> PathBuf {
        self.dingtalk_dir().join("sessions.json")
    }

    pub fn all_platform_states(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<Vec<ChannelPlatformState>> {
        let mut states = Vec::new();
        states.push(self.dingtalk_state(connection, last_error)?);
        states.push(Self::coming_soon_state(Platform::Feishu));
        states.push(Self::coming_soon_state(Platform::Wechat));
        states.push(Self::coming_soon_state(Platform::Wecom));
        Ok(states)
    }

    pub fn coming_soon_state(platform: Platform) -> ChannelPlatformState {
        ChannelPlatformState::coming_soon(platform)
    }

    pub fn dingtalk_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let Some(config) = self.read_dingtalk_config()? else {
            return Ok(ChannelPlatformState {
                platform: Platform::Dingtalk,
                capability: ChannelCapability::Available,
                configured: false,
                enabled: false,
                connection: ChannelConnectionState::Unconfigured,
                config: None,
                last_connected_at: None,
                last_error: None,
            });
        };

        let connection = if !config.enabled {
            ChannelConnectionState::Disconnected
        } else {
            connection
        };

        Ok(ChannelPlatformState {
            platform: Platform::Dingtalk,
            capability: ChannelCapability::Available,
            configured: config.configured,
            enabled: config.enabled,
            connection,
            config: Some(self.config_view(&config)?),
            last_connected_at: None,
            last_error,
        })
    }

    pub fn save_dingtalk_registration(
        &self,
        app_key: String,
        app_secret_plain: String,
        robot_code: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let trimmed_app_key = non_empty(app_key, "app_key")?;
        let trimmed_secret = non_empty(app_secret_plain, "app_secret")?;
        let now = now_rfc3339();
        let robot_code = robot_code.and_then(|v| normalize_optional(v));
        let (robot_code, robot_code_source) = match robot_code {
            Some(value) => (value, RobotCodeSource::Registration),
            None => (trimmed_app_key.clone(), RobotCodeSource::AppKeyFallback),
        };
        let app_secret_encrypted = self.encrypt_secret(&trimmed_secret)?;

        let existing_created_at = self
            .read_dingtalk_config()?
            .map(|cfg| cfg.metadata.created_at)
            .unwrap_or_else(|| now.clone());

        let config = DingtalkStoredConfig {
            schema_version: 1,
            platform: Platform::Dingtalk,
            configured: true,
            enabled: true,
            credentials: DingtalkStoredCredentials {
                app_key: trimmed_app_key,
                app_secret_encrypted,
            },
            bot: DingtalkStoredBot { robot_code, robot_code_source },
            registration: DingtalkStoredRegistration { source: OPEN_CLAW_SOURCE.to_string() },
            metadata: DingtalkStoredMetadata { created_at: existing_created_at, updated_at: now },
        };

        self.write_dingtalk_config(&config)?;
        self.dingtalk_state(ChannelConnectionState::Disconnected, None)
    }

    pub fn set_dingtalk_enabled(&self, enabled: bool) -> Result<ChannelPlatformState> {
        let mut config = self
            .read_dingtalk_config()?
            .ok_or_else(|| anyhow!("DingTalk channel is not configured"))?;
        config.enabled = enabled;
        config.metadata.updated_at = now_rfc3339();
        self.write_dingtalk_config(&config)?;
        let connection = if enabled {
            ChannelConnectionState::Connecting
        } else {
            ChannelConnectionState::Disconnected
        };
        self.dingtalk_state(connection, None)
    }

    pub fn remove_dingtalk(&self) -> Result<ChannelPlatformState> {
        let path = self.dingtalk_config_path();
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(ChannelPlatformState {
            platform: Platform::Dingtalk,
            capability: ChannelCapability::Available,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        })
    }

    pub fn reveal_dingtalk_secret(&self) -> Result<String> {
        let config = self
            .read_dingtalk_config()?
            .ok_or_else(|| anyhow!("DingTalk channel is not configured"))?;
        self.decrypt_secret(&config.credentials.app_secret_encrypted)
    }

    pub fn read_dingtalk_config(&self) -> Result<Option<DingtalkStoredConfig>> {
        let path = self.dingtalk_config_path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(path)?;
        let config = serde_json::from_str::<DingtalkStoredConfig>(&raw)?;
        Ok(Some(config))
    }

    pub fn decrypt_dingtalk_config(&self) -> Result<(DingtalkStoredConfig, String)> {
        let config = self
            .read_dingtalk_config()?
            .ok_or_else(|| anyhow!("DingTalk channel is not configured"))?;
        let secret = self.decrypt_secret(&config.credentials.app_secret_encrypted)?;
        Ok((config, secret))
    }

    fn write_dingtalk_config(&self, config: &DingtalkStoredConfig) -> Result<()> {
        std::fs::create_dir_all(self.dingtalk_dir())?;
        let content = serde_json::to_string_pretty(config)?;
        std::fs::write(self.dingtalk_config_path(), content)?;
        Ok(())
    }

    fn config_view(&self, config: &DingtalkStoredConfig) -> Result<ChannelConfigView> {
        let secret = self.decrypt_secret(&config.credentials.app_secret_encrypted)?;
        Ok(ChannelConfigView {
            platform: Platform::Dingtalk,
            app_key: config.credentials.app_key.clone(),
            app_secret_masked: mask_secret(&secret),
            robot_code: config.bot.robot_code.clone(),
            robot_code_source: config.bot.robot_code_source.clone(),
            source: config.registration.source.clone(),
            created_at: config.metadata.created_at.clone(),
            updated_at: config.metadata.updated_at.clone(),
        })
    }

    fn encrypt_secret(&self, secret: &str) -> Result<String> {
        match &self.secure_storage {
            Some(storage) => storage.encrypt(secret),
            None => Ok(secret.to_string()),
        }
    }

    fn decrypt_secret(&self, encrypted: &str) -> Result<String> {
        match &self.secure_storage {
            Some(storage) => storage.decrypt(encrypted),
            None => Ok(encrypted.to_string()),
        }
    }
}

fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

fn non_empty(value: String, field: &str) -> Result<String> {
    normalize_optional(value).ok_or_else(|| anyhow!("{field} is required"))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    let suffix: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if suffix.is_empty() {
        "••••••••••••".to_string()
    } else {
        format!("••••••••••••{suffix}")
    }
}
```

- [ ] **Step 5: Add chrono dependency if missing**

Check `src-tauri/Cargo.toml`. If `chrono` is not present, add under `[dependencies]`:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 6: Run config store tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml connector::channel::config_store::tests --lib
```

Expected: PASS.

- [ ] **Step 7: Commit Task 2**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/connector/channel/config_store.rs src-tauri/src/connector/channel/mod.rs
git commit -m "feat(channel): add user-scoped config store"
```

---

## Task 3: Backend Manager Commands and Platform State Events

**Files:**
- Modify: `src-tauri/src/connector/channel/types.rs`
- Modify: `src-tauri/src/connector/channel/manager.rs`
- Modify: `src-tauri/src/connector/channel/dingtalk_stream.rs`
- Modify: `src-tauri/src/commands/channel.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/connector/channel/manager.rs` existing tests or compile checks

- [ ] **Step 1: Update stream client to emit connection states**

In `src-tauri/src/connector/channel/dingtalk_stream.rs`, replace import:

```rust
use super::types::{ChannelMessage, ChannelStatus, ConversationType};
```

with:

```rust
use super::types::{ChannelConnectionState, ChannelMessage, ConversationType};
```

Replace the `start` signature:

```rust
pub fn start(
    &self,
    on_status: impl Fn(ChannelStatus) + Send + Sync + 'static,
) -> CancellationToken {
```

with:

```rust
pub fn start(
    &self,
    on_status: impl Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static,
) -> CancellationToken {
```

Replace `run_with_retry` signature:

```rust
async fn run_with_retry(
    &self,
    on_status: Arc<impl Fn(ChannelStatus) + Send + Sync>,
    cancel: CancellationToken,
) {
```

with:

```rust
async fn run_with_retry(
    &self,
    on_status: Arc<impl Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    cancel: CancellationToken,
) {
```

Replace status calls:

```rust
on_status(ChannelStatus::Connecting);
on_status(ChannelStatus::Connected);
on_status(ChannelStatus::ConfigError { message: "AppKey 或 AppSecret 有误，请检查配置".into() });
on_status(ChannelStatus::Reconnecting { delay_secs });
```

with:

```rust
on_status(ChannelConnectionState::Connecting, None);
on_status(ChannelConnectionState::Connected, None);
on_status(
    ChannelConnectionState::ConfigError,
    Some("AppKey 或 AppSecret 有误，请检查配置".into()),
);
on_status(ChannelConnectionState::Reconnecting, None);
```

Do not model retry delay in the new frontend state in this iteration.

- [ ] **Step 2: Replace manager implementation with domain facade**

Edit `src-tauri/src/connector/channel/manager.rs` carefully. Keep the message loop logic, but update fields and config access.

At the top, replace the channel imports with:

```rust
use super::config_store::ChannelConfigStore;
use super::dingtalk_card::CardTarget;
use super::dingtalk_registration::{begin_registration, poll_registration, RegistrationPollState};
use super::dingtalk_stream::DingtalkStreamClient;
use super::reply_manager::DingtalkReplyManager;
use super::router::ChannelSessionRouter;
use super::types::{
    ChannelConnectionState, ChannelConversation, ChannelMessage, ChannelMessagePayload,
    ChannelPlatformState, ChannelPlatformStatePayload, ChannelRegistrationBeginResult,
    ChannelRegistrationPollResult, ChannelRegistrationPollState, ConversationType, Platform,
};
```

Replace these fields:

```rust
secure_storage: Option<Arc<SecureStorage>>,
channels_dir: PathBuf,
sessions_path: PathBuf,
status: Arc<RwLock<ChannelStatus>>,
```

with:

```rust
config_store: Arc<ChannelConfigStore>,
sessions_path: PathBuf,
connection: Arc<RwLock<ChannelConnectionState>>,
last_error: Arc<RwLock<Option<String>>>,
```

In `new`, create the store and new sessions path:

```rust
let config_store = Arc::new(ChannelConfigStore::new(channels_dir.clone(), secure_storage));
let sessions_path = config_store.dingtalk_sessions_path();
```

Initialize fields:

```rust
config_store,
sessions_path,
connection: Arc::new(RwLock::new(ChannelConnectionState::Unconfigured)),
last_error: Arc::new(RwLock::new(None)),
```

- [ ] **Step 3: Add platform state helpers in manager**

In `impl ChannelManager`, add these helper methods:

```rust
async fn current_dingtalk_state(&self) -> Result<ChannelPlatformState> {
    let connection = self.connection.read().await.clone();
    let last_error = self.last_error.read().await.clone();
    self.config_store.dingtalk_state(connection, last_error)
}

async fn emit_dingtalk_state(&self) {
    match self.current_dingtalk_state().await {
        Ok(state) => {
            let _ = self.app_handle.emit(
                "channel:platform-state",
                &ChannelPlatformStatePayload { state },
            );
        }
        Err(error) => log::warn!("[channel] failed to emit platform state: {:#}", error),
    }
}

async fn set_connection_state(&self, connection: ChannelConnectionState, last_error: Option<String>) {
    *self.connection.write().await = connection;
    *self.last_error.write().await = last_error;
    self.emit_dingtalk_state().await;
}

async fn stop_stream(&self) {
    let mut cancel_guard = self.stream_cancel.write().await;
    if let Some(token) = cancel_guard.take() {
        token.cancel();
    }
}
```

- [ ] **Step 4: Replace auto-connect logic**

Replace `auto_connect_if_configured` body with:

```rust
pub async fn auto_connect_if_configured(&self) {
    match self.config_store.read_dingtalk_config() {
        Ok(Some(config)) if config.enabled => {
            if let Err(error) = self.connect_dingtalk_from_store().await {
                log::warn!("[channel] auto_connect failed: {:#}", error);
                self.set_connection_state(
                    ChannelConnectionState::ConfigError,
                    Some(error.to_string()),
                )
                .await;
            }
        }
        Ok(Some(_)) => {
            self.set_connection_state(ChannelConnectionState::Disconnected, None).await;
        }
        Ok(None) => {
            self.set_connection_state(ChannelConnectionState::Unconfigured, None).await;
        }
        Err(error) => {
            log::warn!("[channel] failed to read config: {:#}", error);
            self.set_connection_state(ChannelConnectionState::ConfigError, Some(error.to_string()))
                .await;
        }
    }
}
```

- [ ] **Step 5: Add public manager domain methods**

Add these methods to `impl ChannelManager`:

```rust
pub async fn get_platforms(&self) -> Result<Vec<ChannelPlatformState>> {
    let connection = self.connection.read().await.clone();
    let last_error = self.last_error.read().await.clone();
    self.config_store.all_platform_states(connection, last_error)
}

pub async fn get_platform(&self, platform: Platform) -> Result<ChannelPlatformState> {
    match platform {
        Platform::Dingtalk => self.current_dingtalk_state().await,
        other => Ok(ChannelConfigStore::coming_soon_state(other)),
    }
}

pub async fn set_enabled(&self, platform: Platform, enabled: bool) -> Result<ChannelPlatformState> {
    match platform {
        Platform::Dingtalk => {
            if enabled {
                self.config_store.set_dingtalk_enabled(true)?;
                self.connect_dingtalk_from_store().await?;
            } else {
                self.stop_stream().await;
                self.config_store.set_dingtalk_enabled(false)?;
                self.set_connection_state(ChannelConnectionState::Disconnected, None).await;
            }
            self.current_dingtalk_state().await
        }
        other => anyhow::bail!("{} channel is not available yet", other.as_str()),
    }
}

pub async fn remove_platform(&self, platform: Platform) -> Result<ChannelPlatformState> {
    match platform {
        Platform::Dingtalk => {
            self.stop_stream().await;
            let state = self.config_store.remove_dingtalk()?;
            self.set_connection_state(ChannelConnectionState::Unconfigured, None).await;
            Ok(state)
        }
        other => anyhow::bail!("{} channel is not available yet", other.as_str()),
    }
}

pub async fn reveal_secret(&self, platform: Platform) -> Result<String> {
    match platform {
        Platform::Dingtalk => self.config_store.reveal_dingtalk_secret(),
        other => anyhow::bail!("{} channel is not available yet", other.as_str()),
    }
}
```

- [ ] **Step 6: Extend registration poll result type for platform state**

In `src-tauri/src/connector/channel/types.rs`, replace the legacy `ChannelRegistrationPollResult` definition with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRegistrationPollResult {
    pub state: ChannelRegistrationPollState,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub robot_code: Option<String>,
    pub config: Option<ChannelConfigView>,
    pub platform_state: Option<ChannelPlatformState>,
    pub fail_reason: Option<String>,
}
```

After this change, all constructors in `ChannelManager::poll_dingtalk_registration` must include `config` and `platform_state`. The frontend will stop relying on `clientSecret` in Task 5.

- [ ] **Step 7: Update registration poll success flow**

In `poll_dingtalk_registration`, replace the existing `save_config_and_connect(app_key, app_secret, robot_code)` call with:

```rust
let state = self
    .save_config_and_connect(app_key, app_secret, poll.robot_code.clone())
    .await?;
return Ok(ChannelRegistrationPollResult {
    state: ChannelRegistrationPollState::Success,
    client_id: poll.client_id,
    client_secret: None,
    robot_code: state.config.as_ref().map(|config| config.robot_code.clone()),
    config: state.config.clone(),
    platform_state: Some(state),
    fail_reason: poll.fail_reason,
});
```

For the non-success return at the end, include new fields:

```rust
Ok(ChannelRegistrationPollResult {
    state,
    client_id: poll.client_id,
    client_secret: None,
    robot_code: poll.robot_code,
    config: None,
    platform_state: None,
    fail_reason: poll.fail_reason,
})
```

This intentionally stops returning `client_secret` to the frontend poll result. AppSecret display uses masked config view and reveal command.

- [ ] **Step 8: Replace save/connect helpers**

Replace `save_config_and_connect` with:

```rust
pub async fn save_config_and_connect(
    &self,
    app_key: String,
    app_secret_plain: String,
    robot_code: Option<String>,
) -> Result<ChannelPlatformState> {
    self.config_store
        .save_dingtalk_registration(app_key, app_secret_plain, robot_code)?;
    self.connect_dingtalk_from_store().await?;
    self.current_dingtalk_state().await
}
```

Add:

```rust
async fn connect_dingtalk_from_store(&self) -> Result<()> {
    let (config, app_secret_plain) = self.config_store.decrypt_dingtalk_config()?;
    self.connect_dingtalk(config, app_secret_plain).await
}
```

Replace `connect_dingtalk(&self, config: DingtalkChannelConfig)` signature with:

```rust
async fn connect_dingtalk(&self, config: super::types::DingtalkStoredConfig, app_secret_plain: String) -> Result<()> {
```

Inside it, create the stream client with:

```rust
let stream_client = DingtalkStreamClient::new(
    config.credentials.app_key.clone(),
    app_secret_plain.clone(),
    config.bot.robot_code.clone(),
    msg_tx,
);
```

In the message loop, where reply manager registers credentials, replace `msg.app_key` and `msg.app_secret` usage remains unchanged because `DingtalkStreamClient` still injects them into `ChannelMessage`.

- [ ] **Step 9: Update stream status callback in manager**

Replace the existing `on_status` closure with:

```rust
let connection_arc = Arc::clone(&self.connection);
let last_error_arc = Arc::clone(&self.last_error);
let config_store = Arc::clone(&self.config_store);
let app_for_status = self.app_handle.clone();
let on_status = move |new_connection: ChannelConnectionState, error: Option<String>| {
    let connection_arc = connection_arc.clone();
    let last_error_arc = last_error_arc.clone();
    let config_store = config_store.clone();
    let app_for_status = app_for_status.clone();
    tokio::spawn(async move {
        *connection_arc.write().await = new_connection.clone();
        *last_error_arc.write().await = error.clone();
        match config_store.dingtalk_state(new_connection, error) {
            Ok(state) => {
                let _ = app_for_status.emit(
                    "channel:platform-state",
                    &ChannelPlatformStatePayload { state },
                );
            }
            Err(error) => log::warn!("[channel] failed to build platform state: {:#}", error),
        }
    });
};
```

- [ ] **Step 10: Update commands**

Replace `src-tauri/src/commands/channel.rs` with:

```rust
//! Tauri IPC commands for IM channel management.

use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::connector::channel::{
    ChannelConversation, ChannelManager, ChannelPlatformState, ChannelRegistrationBeginResult,
    ChannelRegistrationPollResult, Platform,
};

fn parse_platform(platform: String) -> Result<Platform, String> {
    Platform::from_str(&platform).ok_or_else(|| format!("Unsupported channel platform: {platform}"))
}

fn manager(app: &AppHandle) -> Result<Arc<ChannelManager>, String> {
    app.try_state::<Arc<ChannelManager>>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "频道功能未初始化，请先登录".to_string())
}

#[tauri::command]
pub async fn channel_get_platforms(app: AppHandle) -> Result<Vec<ChannelPlatformState>, String> {
    manager(&app)?.get_platforms().await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_get_platform(
    app: AppHandle,
    platform: String,
) -> Result<ChannelPlatformState, String> {
    manager(&app)?
        .get_platform(parse_platform(platform)?)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_begin_registration(
    app: AppHandle,
    platform: String,
) -> Result<ChannelRegistrationBeginResult, String> {
    match parse_platform(platform)? {
        Platform::Dingtalk => manager(&app)?
            .begin_dingtalk_registration()
            .await
            .map_err(|e| format!("{:#}", e)),
        other => Err(format!("{} channel registration is not available yet", other.as_str())),
    }
}

#[tauri::command]
pub async fn channel_poll_registration(
    app: AppHandle,
    platform: String,
    device_code: String,
) -> Result<ChannelRegistrationPollResult, String> {
    match parse_platform(platform)? {
        Platform::Dingtalk => manager(&app)?
            .poll_dingtalk_registration(device_code)
            .await
            .map_err(|e| format!("{:#}", e)),
        other => Err(format!("{} channel registration is not available yet", other.as_str())),
    }
}

#[tauri::command]
pub async fn channel_set_enabled(
    app: AppHandle,
    platform: String,
    enabled: bool,
) -> Result<ChannelPlatformState, String> {
    manager(&app)?
        .set_enabled(parse_platform(platform)?, enabled)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_remove_platform(
    app: AppHandle,
    platform: String,
) -> Result<ChannelPlatformState, String> {
    manager(&app)?
        .remove_platform(parse_platform(platform)?)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_reveal_secret(app: AppHandle, platform: String) -> Result<String, String> {
    manager(&app)?
        .reveal_secret(parse_platform(platform)?)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_get_conversations(
    app: AppHandle,
    platform: Option<String>,
) -> Result<Vec<ChannelConversation>, String> {
    if let Some(platform) = platform {
        let parsed = parse_platform(platform)?;
        if parsed != Platform::Dingtalk {
            return Ok(vec![]);
        }
    }
    match app.try_state::<Arc<ChannelManager>>() {
        Some(m) => Ok(m.get_conversations().await),
        None => Ok(vec![]),
    }
}
```

- [ ] **Step 11: Update command registration in lib.rs**

In `src-tauri/src/lib.rs`, replace old channel command registrations:

```rust
commands::channel::channel_save_config,
commands::channel::channel_get_status,
commands::channel::channel_get_conversations,
commands::channel::channel_begin_dingtalk_registration,
commands::channel::channel_poll_dingtalk_registration,
```

with:

```rust
commands::channel::channel_get_platforms,
commands::channel::channel_get_platform,
commands::channel::channel_get_conversations,
commands::channel::channel_begin_registration,
commands::channel::channel_poll_registration,
commands::channel::channel_set_enabled,
commands::channel::channel_remove_platform,
commands::channel::channel_reveal_secret,
```

- [ ] **Step 12: Run cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS. If it fails, fix only compile errors related to renamed types/commands introduced in Tasks 1-3.

- [ ] **Step 13: Commit Task 3**

```bash
git add src-tauri/src/connector/channel src-tauri/src/commands/channel.rs src-tauri/src/lib.rs
git commit -m "feat(channel): expose platform state commands"
```

---

## Task 4: Frontend IPC Types and Channel Store

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/stores/channelStore.ts`
- Create: `src/stores/channelStore.test.ts`

- [ ] **Step 1: Add failing store tests**

Create `src/stores/channelStore.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useChannelStore, initChannelListeners } from './channelStore'
import {
  channelGetPlatforms,
  channelGetConversations,
  channelSetEnabled,
  channelRemovePlatform,
  channelRevealSecret,
  onChannelMessage,
  onChannelPlatformState,
} from '@/lib/tauri'

vi.mock('@/lib/tauri', () => ({
  channelGetPlatforms: vi.fn(),
  channelGetPlatform: vi.fn(),
  channelGetConversations: vi.fn(),
  channelBeginRegistration: vi.fn(),
  channelPollRegistration: vi.fn(),
  channelSetEnabled: vi.fn(),
  channelRemovePlatform: vi.fn(),
  channelRevealSecret: vi.fn(),
  onChannelMessage: vi.fn(),
  onChannelPlatformState: vi.fn(),
}))

const dingtalkConnected = {
  platform: 'dingtalk' as const,
  capability: 'available' as const,
  configured: true,
  enabled: true,
  connection: 'connected' as const,
  config: {
    platform: 'dingtalk' as const,
    appKey: 'ding-key',
    appSecretMasked: '••••••••••••cret',
    robotCode: 'robot-001',
    robotCodeSource: 'registration' as const,
    source: 'OPEN_CLAW',
    createdAt: '2026-05-07T00:00:00Z',
    updatedAt: '2026-05-07T00:00:00Z',
  },
  lastConnectedAt: null,
  lastError: null,
}

const feishuComingSoon = {
  platform: 'feishu' as const,
  capability: 'comingSoon' as const,
  configured: false,
  enabled: false,
  connection: 'unconfigured' as const,
  config: null,
  lastConnectedAt: null,
  lastError: null,
}

describe('channelStore platform domain', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useChannelStore.setState({
      platforms: {},
      conversations: [],
      activeSessionId: null,
    })
    vi.mocked(channelGetPlatforms).mockResolvedValue([dingtalkConnected, feishuComingSoon])
    vi.mocked(channelGetConversations).mockResolvedValue([])
    vi.mocked(onChannelPlatformState).mockResolvedValue(() => {})
    vi.mocked(onChannelMessage).mockResolvedValue(() => {})
  })

  it('loads platform states into a platform map', async () => {
    await useChannelStore.getState().loadPlatforms()

    expect(useChannelStore.getState().platforms.dingtalk).toEqual(dingtalkConnected)
    expect(useChannelStore.getState().platforms.feishu).toEqual(feishuComingSoon)
  })

  it('setEnabled updates only returned platform state', async () => {
    vi.mocked(channelSetEnabled).mockResolvedValue({ ...dingtalkConnected, enabled: false, connection: 'disconnected' })

    await useChannelStore.getState().setEnabled('dingtalk', false)

    expect(channelSetEnabled).toHaveBeenCalledWith('dingtalk', false)
    expect(useChannelStore.getState().platforms.dingtalk.enabled).toBe(false)
    expect(useChannelStore.getState().platforms.dingtalk.config?.appSecretMasked).toBe('••••••••••••cret')
  })

  it('removePlatform stores the unconfigured state returned by backend', async () => {
    const unconfigured = {
      ...dingtalkConnected,
      configured: false,
      enabled: false,
      connection: 'unconfigured' as const,
      config: null,
    }
    vi.mocked(channelRemovePlatform).mockResolvedValue(unconfigured)

    await useChannelStore.getState().removePlatform('dingtalk')

    expect(channelRemovePlatform).toHaveBeenCalledWith('dingtalk')
    expect(useChannelStore.getState().platforms.dingtalk.config).toBeNull()
  })

  it('revealSecret returns secret without writing it to store', async () => {
    await useChannelStore.getState().loadPlatforms()
    vi.mocked(channelRevealSecret).mockResolvedValue('plain-secret')

    await expect(useChannelStore.getState().revealSecret('dingtalk')).resolves.toBe('plain-secret')
    expect(JSON.stringify(useChannelStore.getState())).not.toContain('plain-secret')
  })
})
```

- [ ] **Step 2: Run store test and verify it fails**

Run:

```bash
pnpm vitest run src/stores/channelStore.test.ts
```

Expected: FAIL because new IPC wrappers and store shape do not exist.

- [ ] **Step 3: Update channel types and IPC wrappers**

In `src/lib/tauri.ts`, replace the current Channel types/IPC block with:

```ts
export type ChannelPlatform = 'dingtalk' | 'feishu' | 'wechat' | 'wecom'
export type ChannelCapability = 'available' | 'comingSoon'
export type ChannelConnectionState =
  | 'unconfigured'
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'configError'

export interface ChannelConfigView {
  platform: ChannelPlatform
  appKey: string
  appSecretMasked: string
  robotCode: string
  robotCodeSource: 'registration' | 'appKeyFallback'
  source: 'OPEN_CLAW'
  createdAt: string
  updatedAt: string
}

export interface ChannelPlatformState {
  platform: ChannelPlatform
  capability: ChannelCapability
  configured: boolean
  enabled: boolean
  connection: ChannelConnectionState
  config: ChannelConfigView | null
  lastConnectedAt: string | null
  lastError: string | null
}

export interface ChannelPlatformStatePayload {
  state: ChannelPlatformState
}

export interface ChannelMessagePayload {
  platform: string
  sessionId: string
  senderNick: string
  textPreview: string
}

export interface ChannelConversation {
  sessionId: string
  platform: ChannelPlatform
  conversationType: 'group' | 'private'
  externalId: string
  displayName: string
  unreadCount: number
}

export interface ChannelRegistrationBeginResult {
  deviceCode: string
  userCode: string
  verificationUriComplete: string
  verificationUri: string
  intervalSeconds: number
  expiresInSeconds: number
  source: string
}

export interface ChannelRegistrationPollResult {
  state: 'waiting' | 'success' | 'fail' | 'expired' | 'unknown'
  clientId?: string | null
  clientSecret?: string | null
  robotCode?: string | null
  config?: ChannelConfigView | null
  platformState?: ChannelPlatformState | null
  failReason?: string | null
}

export function channelGetPlatforms(): Promise<ChannelPlatformState[]> {
  return invoke<ChannelPlatformState[]>('channel_get_platforms')
}

export function channelGetPlatform(platform: ChannelPlatform): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_get_platform', { platform })
}

export function channelGetConversations(platform?: ChannelPlatform): Promise<ChannelConversation[]> {
  return invoke<ChannelConversation[]>('channel_get_conversations', { platform })
}

export function channelBeginRegistration(platform: ChannelPlatform): Promise<ChannelRegistrationBeginResult> {
  return invoke<ChannelRegistrationBeginResult>('channel_begin_registration', { platform })
}

export function channelPollRegistration(
  platform: ChannelPlatform,
  deviceCode: string,
): Promise<ChannelRegistrationPollResult> {
  return invoke<ChannelRegistrationPollResult>('channel_poll_registration', { platform, deviceCode })
}

export function channelSetEnabled(platform: ChannelPlatform, enabled: boolean): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_set_enabled', { platform, enabled })
}

export function channelRemovePlatform(platform: ChannelPlatform): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_remove_platform', { platform })
}

export function channelRevealSecret(platform: ChannelPlatform): Promise<string> {
  return invoke<string>('channel_reveal_secret', { platform })
}

export function onChannelPlatformState(
  handler: (payload: ChannelPlatformStatePayload) => void,
): Promise<() => void> {
  return listen<ChannelPlatformStatePayload>(TAURI_EVENTS.CHANNEL_PLATFORM_STATE, (e) => handler(e.payload))
}

export function onChannelMessage(
  handler: (payload: ChannelMessagePayload) => void,
): Promise<() => void> {
  return listen<ChannelMessagePayload>(TAURI_EVENTS.CHANNEL_MESSAGE, (e) => handler(e.payload))
}
```

Also update `TAURI_EVENTS` near the top:

```ts
CHANNEL_PLATFORM_STATE: 'channel:platform-state',
CHANNEL_MESSAGE: 'channel:message',
```

Remove `CHANNEL_STATUS` if it is no longer referenced after this task.

- [ ] **Step 4: Replace channelStore implementation**

Replace `src/stores/channelStore.ts` with:

```ts
import { create } from 'zustand'
import {
  type ChannelConversation,
  type ChannelPlatform,
  type ChannelPlatformState,
  channelBeginRegistration,
  channelGetConversations,
  channelGetPlatform,
  channelGetPlatforms,
  channelPollRegistration,
  channelRemovePlatform,
  channelRevealSecret,
  channelSetEnabled,
  onChannelMessage,
  onChannelPlatformState,
} from '@/lib/tauri'

type PlatformMap = Partial<Record<ChannelPlatform, ChannelPlatformState>>

interface ChannelState {
  platforms: PlatformMap
  conversations: ChannelConversation[]
  activeSessionId: string | null

  setPlatformState: (state: ChannelPlatformState) => void
  setConversations: (convs: ChannelConversation[]) => void
  setActiveSession: (sessionId: string | null) => void
  incrementUnread: (sessionId: string) => void
  clearUnread: (sessionId: string) => void
  loadPlatforms: () => Promise<void>
  loadPlatform: (platform: ChannelPlatform) => Promise<void>
  beginRegistration: typeof channelBeginRegistration
  pollRegistration: typeof channelPollRegistration
  setEnabled: (platform: ChannelPlatform, enabled: boolean) => Promise<void>
  removePlatform: (platform: ChannelPlatform) => Promise<void>
  revealSecret: (platform: ChannelPlatform) => Promise<string>
  loadConversations: (platform?: ChannelPlatform) => Promise<void>
}

export const useChannelStore = create<ChannelState>((set, get) => ({
  platforms: {},
  conversations: [],
  activeSessionId: null,

  setPlatformState: (state) => {
    set((current) => ({ platforms: { ...current.platforms, [state.platform]: state } }))
  },

  setConversations: (convs) => set({ conversations: convs }),

  setActiveSession: (sessionId) => {
    set({ activeSessionId: sessionId })
    if (sessionId) get().clearUnread(sessionId)
  },

  incrementUnread: (sessionId) =>
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.sessionId === sessionId ? { ...c, unreadCount: c.unreadCount + 1 } : c,
      ),
    })),

  clearUnread: (sessionId) =>
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.sessionId === sessionId ? { ...c, unreadCount: 0 } : c,
      ),
    })),

  loadPlatforms: async () => {
    const states = await channelGetPlatforms()
    set({ platforms: Object.fromEntries(states.map((state) => [state.platform, state])) as PlatformMap })
  },

  loadPlatform: async (platform) => {
    const state = await channelGetPlatform(platform)
    get().setPlatformState(state)
  },

  beginRegistration: channelBeginRegistration,
  pollRegistration: channelPollRegistration,

  setEnabled: async (platform, enabled) => {
    const state = await channelSetEnabled(platform, enabled)
    get().setPlatformState(state)
  },

  removePlatform: async (platform) => {
    const state = await channelRemovePlatform(platform)
    get().setPlatformState(state)
  },

  revealSecret: async (platform) => channelRevealSecret(platform),

  loadConversations: async (platform) => {
    try {
      const convs = await channelGetConversations(platform)
      set({ conversations: convs })
    } catch (e) {
      console.error('[channelStore] loadConversations failed', e)
    }
  },
}))

let listenersInitialized = false

/** App 启动时调用一次，订阅后端事件并拉取初始状态。 */
export async function initChannelListeners() {
  if (listenersInitialized) return
  listenersInitialized = true

  try {
    await useChannelStore.getState().loadPlatforms()
  } catch (e) {
    console.error('[channelStore] loadPlatforms failed', e)
  }

  await onChannelPlatformState(({ state }) => {
    useChannelStore.getState().setPlatformState(state)
  })
  await onChannelMessage(({ sessionId }) => {
    const { activeSessionId } = useChannelStore.getState()
    if (sessionId !== activeSessionId) {
      useChannelStore.getState().incrementUnread(sessionId)
    }
  })
}
```

- [ ] **Step 5: Run store test**

Run:

```bash
pnpm vitest run src/stores/channelStore.test.ts
```

Expected: PASS.

- [ ] **Step 6: Run TypeScript check and note expected channel UI failures**

Run:

```bash
pnpm exec tsc -b --pretty false
```

Expected: FAIL because `ChannelPage` and `ChannelConfig` still import old wrapper names and `dingtalkStatus`. These failures are resolved in Tasks 5-6.

- [ ] **Step 7: Commit Task 4**

```bash
git add src/lib/tauri.ts src/stores/channelStore.ts src/stores/channelStore.test.ts
git commit -m "feat(channel): add platform store and IPC wrappers"
```

---

## Task 5: Registration Dialog Uses Platform Domain

**Files:**
- Modify: `src/features/channel/ChannelConfig.tsx`
- Modify: `src/features/channel/ChannelConfig.test.tsx`

- [ ] **Step 1: Replace ChannelConfig tests with domain expectations**

Replace `src/features/channel/ChannelConfig.test.tsx` with:

```tsx
import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ChannelConfig } from './ChannelConfig'
import { useChannelStore } from '@/stores/channelStore'

const beginResult = {
  deviceCode: 'device-1',
  userCode: 'ABCD-EFGH-IJKL',
  verificationUriComplete:
    'https://open-dev.dingtalk.com/openapp/registration/openClaw?user_code=ABCD-EFGH-IJKL&source=OPEN_CLAW',
  verificationUri: 'https://open-dev.dingtalk.com/openapp/registration/openClaw?source=OPEN_CLAW',
  intervalSeconds: 30,
  expiresInSeconds: 7200,
  source: 'OPEN_CLAW',
}

const platformState = {
  platform: 'dingtalk' as const,
  capability: 'available' as const,
  configured: true,
  enabled: true,
  connection: 'connected' as const,
  config: {
    platform: 'dingtalk' as const,
    appKey: 'ding-app-key',
    appSecretMasked: '••••••••••••cret',
    robotCode: 'robot-code',
    robotCodeSource: 'registration' as const,
    source: 'OPEN_CLAW',
    createdAt: '2026-05-07T00:00:00Z',
    updatedAt: '2026-05-07T00:00:00Z',
  },
  lastConnectedAt: null,
  lastError: null,
}

describe('ChannelConfig OPEN_CLAW registration', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useRealTimers()
    useChannelStore.setState({ platforms: {}, conversations: [], activeSessionId: null })
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('renders a QR-only first flow and starts OPEN_CLAW registration automatically', async () => {
    const beginRegistration = vi.fn().mockResolvedValue(beginResult)
    const pollRegistration = vi.fn().mockResolvedValue({ state: 'waiting' })
    useChannelStore.setState({ beginRegistration, pollRegistration })

    render(<ChannelConfig />)

    expect(screen.getByLabelText('钉钉扫码二维码')).toBeInTheDocument()
    expect(screen.queryByText('手动配置')).not.toBeInTheDocument()
    await waitFor(() => {
      expect(beginRegistration).toHaveBeenCalledWith('dingtalk')
    })
    expect(await screen.findByText(/等待你在钉钉页面完成创建/)).toBeInTheDocument()
  })

  it('can regenerate the QR code after a registration error', async () => {
    const beginRegistration = vi
      .fn()
      .mockRejectedValueOnce(new Error('network error'))
      .mockResolvedValueOnce(beginResult)
    const pollRegistration = vi.fn().mockResolvedValue({ state: 'waiting' })
    useChannelStore.setState({ beginRegistration, pollRegistration })

    render(<ChannelConfig />)

    expect(await screen.findByText('network error')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重新生成二维码' }))

    await waitFor(() => {
      expect(beginRegistration).toHaveBeenCalledTimes(2)
    })
    expect(await screen.findByText(/等待你在钉钉页面完成创建/)).toBeInTheDocument()
  })

  it('shows appKey, masked appSecret, and robotCode after polling succeeds', async () => {
    const onSaved = vi.fn()
    const beginRegistration = vi.fn().mockResolvedValue(beginResult)
    const pollRegistration = vi.fn().mockResolvedValue({
      state: 'success',
      config: platformState.config,
      platformState,
    })
    const setPlatformState = vi.fn()
    useChannelStore.setState({ beginRegistration, pollRegistration, setPlatformState })

    render(<ChannelConfig onSaved={onSaved} />)

    await waitFor(() => {
      expect(onSaved).toHaveBeenCalledTimes(1)
    })
    expect(setPlatformState).toHaveBeenCalledWith(platformState)
    expect(await screen.findByText('扫码开通成功')).toBeInTheDocument()
    expect(screen.getByText('AppKey')).toBeInTheDocument()
    expect(screen.getByText('ding-app-key')).toBeInTheDocument()
    expect(screen.getByText('AppSecret')).toBeInTheDocument()
    expect(screen.getByText('••••••••••••cret')).toBeInTheDocument()
    expect(screen.getByText('RobotCode')).toBeInTheDocument()
    expect(screen.getByText('robot-code')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: Run ChannelConfig test and verify it fails**

Run:

```bash
pnpm vitest run src/features/channel/ChannelConfig.test.tsx
```

Expected: FAIL because `ChannelConfig` still uses old IPC wrappers and expects clientSecret.

- [ ] **Step 3: Update ChannelConfig component**

In `src/features/channel/ChannelConfig.tsx`:

1. Replace imports from `@/lib/tauri` with store usage:

```tsx
import { type ChannelConfigView, type ChannelRegistrationBeginResult } from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
```

2. Replace `RegisteredCredentials` with:

```ts
interface RegisteredCredentials {
  config: ChannelConfigView
}
```

3. Inside component, add store actions:

```tsx
const beginRegistration = useChannelStore((s) => s.beginRegistration)
const pollRegistrationAction = useChannelStore((s) => s.pollRegistration)
const setPlatformState = useChannelStore((s) => s.setPlatformState)
```

4. Replace `channelPollDingtalkRegistration(begin.deviceCode)` with:

```tsx
const result = await pollRegistrationAction('dingtalk', begin.deviceCode)
```

5. Replace success handling with:

```tsx
if (result.state === 'success') {
  const config = result.config ?? result.platformState?.config
  if (!config) {
    throw new Error('钉钉已授权，但未返回频道配置')
  }
  if (result.platformState) {
    setPlatformState(result.platformState)
  }
  setRegistrationStatus('success')
  setCredentials({ config })
  setRegistrationMessage('钉钉频道已连接')
  onSaved?.()
  return
}
```

6. Replace `channelBeginDingtalkRegistration()` with:

```tsx
const begin = await beginRegistration('dingtalk')
```

7. Replace success credential rows with:

```tsx
<CredentialRow label="AppKey" value={credentials.config.appKey} />
<CredentialRow label="AppSecret" value={credentials.config.appSecretMasked} />
<CredentialRow label="RobotCode" value={credentials.config.robotCode} />
```

8. Keep the QR and error UI unchanged.

- [ ] **Step 4: Run ChannelConfig test**

Run:

```bash
pnpm vitest run src/features/channel/ChannelConfig.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit Task 5**

```bash
git add src/features/channel/ChannelConfig.tsx src/features/channel/ChannelConfig.test.tsx
git commit -m "feat(channel): use platform registration state in config dialog"
```

---

## Task 6: Read-only Config Details Dialog

**Files:**
- Create: `src/features/channel/ChannelConfigDetails.tsx`
- Create: `src/features/channel/ChannelConfigDetails.test.tsx`

- [ ] **Step 1: Write failing details dialog tests**

Create `src/features/channel/ChannelConfigDetails.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmDialogHost, useConfirmDialogStore } from '@/components/common/ConfirmDialogHost'
import { useChannelStore } from '@/stores/channelStore'
import { ChannelConfigDetails } from './ChannelConfigDetails'

const config = {
  platform: 'dingtalk' as const,
  appKey: 'ding-app-key',
  appSecretMasked: '••••••••••••cret',
  robotCode: 'robot-code',
  robotCodeSource: 'registration' as const,
  source: 'OPEN_CLAW' as const,
  createdAt: '2026-05-07T00:00:00Z',
  updatedAt: '2026-05-07T01:00:00Z',
}

describe('ChannelConfigDetails', () => {
  beforeEach(() => {
    useConfirmDialogStore.setState({ request: null })
    useChannelStore.setState({
      platforms: {},
      conversations: [],
      activeSessionId: null,
      revealSecret: vi.fn().mockResolvedValue('plain-secret'),
    })
  })

  it('renders read-only fields and no QR code', () => {
    render(<ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />)

    expect(screen.getByText('钉钉配置')).toBeInTheDocument()
    expect(screen.getByText('ding-app-key')).toBeInTheDocument()
    expect(screen.getByText('••••••••••••cret')).toBeInTheDocument()
    expect(screen.getByText('robot-code')).toBeInTheDocument()
    expect(screen.queryByLabelText('钉钉扫码二维码')).not.toBeInTheDocument()
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
  })

  it('reveals AppSecret only after second confirmation', async () => {
    const revealSecret = vi.fn().mockResolvedValue('plain-secret')
    useChannelStore.setState({ revealSecret })

    render(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )

    await userEvent.click(screen.getByRole('button', { name: '显示 AppSecret' }))
    expect(revealSecret).not.toHaveBeenCalled()
    await userEvent.click(await screen.findByRole('button', { name: '确认显示' }))

    await waitFor(() => {
      expect(revealSecret).toHaveBeenCalledWith('dingtalk')
    })
    expect(await screen.findByText('plain-secret')).toBeInTheDocument()
  })

  it('clears revealed secret when closed', async () => {
    const onOpenChange = vi.fn()
    const revealSecret = vi.fn().mockResolvedValue('plain-secret')
    useChannelStore.setState({ revealSecret })

    const { rerender } = render(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={onOpenChange} />
        <ConfirmDialogHost />
      </>,
    )
    await userEvent.click(screen.getByRole('button', { name: '显示 AppSecret' }))
    await userEvent.click(await screen.findByRole('button', { name: '确认显示' }))
    expect(await screen.findByText('plain-secret')).toBeInTheDocument()

    rerender(
      <>
        <ChannelConfigDetails config={config} open={false} onOpenChange={onOpenChange} />
        <ConfirmDialogHost />
      </>,
    )
    rerender(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={onOpenChange} />
        <ConfirmDialogHost />
      </>,
    )

    expect(screen.queryByText('plain-secret')).not.toBeInTheDocument()
    expect(screen.getByText('••••••••••••cret')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: Run details test and verify it fails**

Run:

```bash
pnpm vitest run src/features/channel/ChannelConfigDetails.test.tsx
```

Expected: FAIL because component does not exist.

- [ ] **Step 3: Implement ChannelConfigDetails**

Create `src/features/channel/ChannelConfigDetails.tsx`:

```tsx
import { useEffect, useState } from 'react'
import { Eye, EyeOff } from 'lucide-react'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { ChannelConfigView } from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'

interface ChannelConfigDetailsProps {
  config: ChannelConfigView
  open: boolean
  onOpenChange: (open: boolean) => void
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{value}</div>
    </div>
  )
}

export function ChannelConfigDetails({ config, open, onOpenChange }: ChannelConfigDetailsProps) {
  const revealSecret = useChannelStore((s) => s.revealSecret)
  const [revealedSecret, setRevealedSecret] = useState<string | null>(null)
  const [revealing, setRevealing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) {
      setRevealedSecret(null)
      setError(null)
      setRevealing(false)
    }
  }, [open])

  const handleReveal = async () => {
    const confirmed = await requestConfirm({
      title: '显示 AppSecret？',
      description: 'AppSecret 是敏感凭证。确认后会在当前弹窗中显示，关闭弹窗后会自动清除。',
      confirmLabel: '确认显示',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    setRevealing(true)
    setError(null)
    try {
      const secret = await revealSecret(config.platform)
      setRevealedSecret(secret)
    } catch (err) {
      setError(err instanceof Error ? err.message : '读取 AppSecret 失败')
    } finally {
      setRevealing(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl rounded-[28px] bg-white p-0 shadow-2xl">
        <DialogHeader className="px-8 pt-8 text-left">
          <DialogTitle className="text-2xl font-bold">钉钉配置</DialogTitle>
          <DialogDescription>当前配置为只读。需要更换凭证时，请移除后重新扫码配置。</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 px-8 py-6">
          <DetailRow label="AppKey" value={config.appKey} />
          <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">AppSecret</div>
                <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">
                  {revealedSecret ?? config.appSecretMasked}
                </div>
              </div>
              <Button type="button" variant="secondary" size="sm" onClick={handleReveal} disabled={revealing}>
                {revealedSecret ? <EyeOff className="mr-1 h-4 w-4" /> : <Eye className="mr-1 h-4 w-4" />}
                {revealedSecret ? '已显示' : '显示 AppSecret'}
              </Button>
            </div>
          </div>
          <DetailRow label="RobotCode" value={config.robotCode} />
          <DetailRow label="Source" value={config.source} />
          <DetailRow label="创建时间" value={config.createdAt} />
          <DetailRow label="更新时间" value={config.updatedAt} />
          {error && <div className="rounded-xl bg-red-50 px-4 py-3 text-sm font-semibold text-red-500">{error}</div>}
        </div>
      </DialogContent>
    </Dialog>
  )
}
```

- [ ] **Step 4: Run details test**

Run:

```bash
pnpm vitest run src/features/channel/ChannelConfigDetails.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit Task 6**

```bash
git add src/features/channel/ChannelConfigDetails.tsx src/features/channel/ChannelConfigDetails.test.tsx
git commit -m "feat(channel): add read-only config details dialog"
```

---

## Task 7: ChannelPage Domain UI, Switch, Remove Confirmation

**Files:**
- Modify: `src/features/channel/ChannelPage.tsx`
- Modify: `src/features/channel/ChannelPage.test.tsx`

- [ ] **Step 1: Replace ChannelPage tests with domain UI behavior**

Replace `src/features/channel/ChannelPage.test.tsx` with:

```tsx
import '@testing-library/jest-dom'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmDialogHost, useConfirmDialogStore } from '@/components/common/ConfirmDialogHost'
import { useChannelStore } from '@/stores/channelStore'
import { useChatStore } from '@/stores/chatStore'
import { ChannelPage } from './ChannelPage'

const getMessagesMock = vi.hoisted(() => vi.fn())
const getTasksMock = vi.hoisted(() => vi.fn())

vi.mock('@/components/layout/ChatArea', () => ({ ChatArea: () => <main data-testid="channel-chat-content" /> }))
vi.mock('@/components/chat-scene/ChatBottomArea', () => ({ ChatBottomArea: () => <footer data-testid="channel-chat-input" /> }))
vi.mock('@/components/chat/RightPanel', () => ({
  RightPanel: ({ conversationId }: { conversationId: string }) => <aside data-testid="channel-right-panel">{conversationId}</aside>,
}))
vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    getMessages: getMessagesMock,
    getTasks: getTasksMock,
    openGeneratedFile: vi.fn(),
  }
})

const unconfigured = {
  platform: 'dingtalk' as const,
  capability: 'available' as const,
  configured: false,
  enabled: false,
  connection: 'unconfigured' as const,
  config: null,
  lastConnectedAt: null,
  lastError: null,
}

const connected = {
  platform: 'dingtalk' as const,
  capability: 'available' as const,
  configured: true,
  enabled: true,
  connection: 'connected' as const,
  config: {
    platform: 'dingtalk' as const,
    appKey: 'ding-app-key',
    appSecretMasked: '••••••••••••cret',
    robotCode: 'robot-code',
    robotCodeSource: 'registration' as const,
    source: 'OPEN_CLAW' as const,
    createdAt: '2026-05-07T00:00:00Z',
    updatedAt: '2026-05-07T01:00:00Z',
  },
  lastConnectedAt: null,
  lastError: null,
}

const feishu = {
  platform: 'feishu' as const,
  capability: 'comingSoon' as const,
  configured: false,
  enabled: false,
  connection: 'unconfigured' as const,
  config: null,
  lastConnectedAt: null,
  lastError: null,
}

function renderPage(ui = <ChannelPage />) {
  return render(
    <>
      {ui}
      <ConfirmDialogHost />
    </>,
  )
}

describe('ChannelPage domain UI', () => {
  beforeEach(() => {
    useConfirmDialogStore.setState({ request: null })
    useChannelStore.setState({
      platforms: { dingtalk: unconfigured, feishu },
      conversations: [],
      activeSessionId: null,
      loadPlatforms: vi.fn().mockResolvedValue(undefined),
      loadConversations: vi.fn().mockResolvedValue(undefined),
      beginRegistration: vi.fn(),
      pollRegistration: vi.fn(),
      setEnabled: vi.fn().mockResolvedValue(undefined),
      removePlatform: vi.fn().mockResolvedValue(undefined),
      revealSecret: vi.fn().mockResolvedValue('plain-secret'),
    })
    useChatStore.setState({
      conversations: [],
      activeConversationId: null,
      messages: [],
      taskStates: {},
      streamStates: {},
      isStreaming: false,
      streamingContent: '',
      toolExecutions: [],
    })
    getMessagesMock.mockResolvedValue([])
    getTasksMock.mockResolvedValue([])
  })

  it('unconfigured DingTalk shows only the config button', () => {
    renderPage()

    expect(screen.getByRole('heading', { name: 'IM 频道' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '配置钉钉' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '更多钉钉配置' })).not.toBeInTheDocument()
    expect(screen.queryByRole('switch', { name: /钉钉/ })).not.toBeInTheDocument()
  })

  it('configured DingTalk opens read-only config details from menu', async () => {
    useChannelStore.setState({ platforms: { dingtalk: connected, feishu } })
    renderPage()

    await userEvent.click(screen.getByRole('button', { name: '更多钉钉配置' }))
    await userEvent.click(await screen.findByRole('menuitem', { name: '配置' }))

    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText('钉钉配置')).toBeInTheDocument()
    expect(within(dialog).getByText('ding-app-key')).toBeInTheDocument()
    expect(within(dialog).getByText('robot-code')).toBeInTheDocument()
    expect(within(dialog).queryByLabelText('钉钉扫码二维码')).not.toBeInTheDocument()
  })

  it('remove requires confirmation and restores unconfigured state through store action', async () => {
    const removePlatform = vi.fn().mockResolvedValue(undefined)
    useChannelStore.setState({ platforms: { dingtalk: connected, feishu }, removePlatform })
    renderPage()

    await userEvent.click(screen.getByRole('button', { name: '更多钉钉配置' }))
    await userEvent.click(await screen.findByRole('menuitem', { name: '移除' }))
    expect(await screen.findByText('移除钉钉频道？')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '确认移除' }))

    await waitFor(() => {
      expect(removePlatform).toHaveBeenCalledWith('dingtalk')
    })
  })

  it('switch off disables connection without removing config', async () => {
    const setEnabled = vi.fn().mockResolvedValue(undefined)
    const removePlatform = vi.fn().mockResolvedValue(undefined)
    useChannelStore.setState({ platforms: { dingtalk: connected, feishu }, setEnabled, removePlatform })
    renderPage()

    await userEvent.click(screen.getByRole('switch', { name: '钉钉频道已启用' }))

    expect(setEnabled).toHaveBeenCalledWith('dingtalk', false)
    expect(removePlatform).not.toHaveBeenCalled()
  })

  it('switch on reconnects using existing config', async () => {
    const setEnabled = vi.fn().mockResolvedValue(undefined)
    useChannelStore.setState({
      platforms: { dingtalk: { ...connected, enabled: false, connection: 'disconnected' }, feishu },
      setEnabled,
    })
    renderPage()

    await userEvent.click(screen.getByRole('switch', { name: '钉钉频道已停用' }))

    expect(setEnabled).toHaveBeenCalledWith('dingtalk', true)
  })
})
```

- [ ] **Step 2: Run ChannelPage test and verify failure**

Run:

```bash
pnpm vitest run src/features/channel/ChannelPage.test.tsx
```

Expected: FAIL because `ChannelPage` still uses `dingtalkStatus` and old menu behavior.

- [ ] **Step 3: Update ChannelPage imports**

In `src/features/channel/ChannelPage.tsx`:

1. Add:

```tsx
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { ChannelConfigDetails } from './ChannelConfigDetails'
import type { ChannelPlatform, ChannelPlatformState } from '@/lib/tauri'
```

2. Remove `type ChannelStatusState` import.

- [ ] **Step 4: Replace PlatformCardModel**

Replace `PlatformKey` and `PlatformCardModel` with:

```tsx
type PlatformKey = ChannelPlatform

interface PlatformCardModel {
  key: PlatformKey
  name: string
  description: string
  icon: string
  iconClassName: string
  state: ChannelPlatformState
  statusLabel: string
  statusTone: 'success' | 'muted' | 'error' | 'pending'
}
```

Replace `statusMeta` with:

```tsx
function statusMeta(state: ChannelPlatformState) {
  if (state.capability === 'comingSoon') return { label: '未接入', tone: 'muted' as const }
  if (!state.configured) return { label: '未接入', tone: 'muted' as const }
  if (!state.enabled) return { label: '已配置 / 未连接', tone: 'muted' as const }
  switch (state.connection) {
    case 'connected':
      return { label: '已连接', tone: 'success' as const }
    case 'connecting':
      return { label: '连接中', tone: 'pending' as const }
    case 'reconnecting':
      return { label: '重连中', tone: 'pending' as const }
    case 'configError':
      return { label: '配置有误', tone: 'error' as const }
    default:
      return { label: '未接入', tone: 'muted' as const }
  }
}
```

- [ ] **Step 5: Update PlatformCard behavior**

Change `PlatformCard` props to:

```tsx
function PlatformCard({
  platform,
  onRegister,
  onShowDetails,
  onRemove,
  onToggle,
}: {
  platform: PlatformCardModel
  onRegister: () => void
  onShowDetails: () => void
  onRemove: () => void
  onToggle: (enabled: boolean) => void
}) {
```

Replace the action area with:

```tsx
<div className="ml-6 flex shrink-0 items-center gap-4">
  {platform.state.configured && platform.key === 'dingtalk' && (
    <AppDropdown
      ariaLabel="更多钉钉配置"
      trigger={
        <button type="button" className="rounded-full p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
          <MoreHorizontal className="h-5 w-5" />
        </button>
      }
      items={[
        { id: 'configure', label: '配置', onSelect: onShowDetails },
        { id: 'remove', label: '移除', className: 'text-destructive', onSelect: onRemove },
      ]}
    />
  )}
  {platform.state.configured ? (
    <Switch
      checked={platform.state.enabled}
      aria-label={platform.state.enabled ? `${platform.name}频道已启用` : `${platform.name}频道已停用`}
      onCheckedChange={onToggle}
    />
  ) : platform.state.capability === 'available' ? (
    <Button
      type="button"
      className="rounded-full bg-black px-6 text-white hover:bg-black/85"
      onClick={onRegister}
      aria-label={`配置${platform.name}`}
    >
      配置
    </Button>
  ) : (
    <Button type="button" className="rounded-full bg-black px-6 text-white hover:bg-black/85" disabled>
      配置
    </Button>
  )}
</div>
```

- [ ] **Step 6: Update ChannelOverview props**

Change props to:

```tsx
function ChannelOverview({
  platforms,
  onRegisterDingtalk,
  onShowDingtalkDetails,
  onRemoveDingtalk,
  onToggleDingtalk,
}: {
  platforms: PlatformCardModel[]
  onRegisterDingtalk: () => void
  onShowDingtalkDetails: () => void
  onRemoveDingtalk: () => void
  onToggleDingtalk: (enabled: boolean) => void
}) {
```

In `PlatformCard`, wire actions for DingTalk and no-ops for other platforms.

- [ ] **Step 7: Update ChannelPage state and platform derivation**

In `ChannelPage`, replace `dingtalkStatus` selector with:

```tsx
const platformsByKey = useChannelStore((s) => s.platforms)
const setEnabled = useChannelStore((s) => s.setEnabled)
const removePlatform = useChannelStore((s) => s.removePlatform)
const loadPlatforms = useChannelStore((s) => s.loadPlatforms)
```

Add dialog state:

```tsx
const [detailsOpen, setDetailsOpen] = useState(false)
```

In the init effect, call `loadPlatforms()` as well:

```tsx
void initChannelListeners()
void loadPlatforms()
void loadConversations()
```

Create fallback states before `useMemo`:

```tsx
const dingtalkState = platformsByKey.dingtalk ?? {
  platform: 'dingtalk',
  capability: 'available',
  configured: false,
  enabled: false,
  connection: 'unconfigured',
  config: null,
  lastConnectedAt: null,
  lastError: null,
} satisfies ChannelPlatformState
```

Create coming-soon states for other platforms similarly or via a helper:

```tsx
function comingSoon(platform: ChannelPlatform): ChannelPlatformState {
  return { platform, capability: 'comingSoon', configured: false, enabled: false, connection: 'unconfigured', config: null, lastConnectedAt: null, lastError: null }
}
```

Build platform models from states and `statusMeta(state)`.

- [ ] **Step 8: Add remove and toggle handlers**

Inside `ChannelPage`, add:

```tsx
const handleRemoveDingtalk = async () => {
  const confirmed = await requestConfirm({
    title: '移除钉钉频道？',
    description: '这会断开钉钉频道，并删除本地保存的 AppKey 和 AppSecret。已有聊天历史会保留。之后需要重新扫码才能再次配置。',
    confirmLabel: '确认移除',
    cancelLabel: '取消',
    variant: 'destructive',
  })
  if (!confirmed) return
  await removePlatform('dingtalk')
}

const handleToggleDingtalk = async (enabled: boolean) => {
  await setEnabled('dingtalk', enabled)
}
```

- [ ] **Step 9: Render dialogs**

Keep registration `Dialog` with `ChannelConfig`, but rename intent state to `registrationOpen` if desired.

After registration dialog, render details if config exists:

```tsx
{dingtalkState.config && (
  <ChannelConfigDetails
    config={dingtalkState.config}
    open={detailsOpen}
    onOpenChange={setDetailsOpen}
  />
)}
```

- [ ] **Step 10: Run ChannelPage test**

Run:

```bash
pnpm vitest run src/features/channel/ChannelPage.test.tsx
```

Expected: PASS.

- [ ] **Step 11: Commit Task 7**

```bash
git add src/features/channel/ChannelPage.tsx src/features/channel/ChannelPage.test.tsx
git commit -m "feat(channel): render platform domain controls"
```

---

## Task 8: Integration Cleanup and Verification

**Files:**
- Modify as needed: `src/features/channel/*`, `src/stores/channelStore.ts`, `src/lib/tauri.ts`, `src-tauri/src/connector/channel/*`, `src-tauri/src/commands/channel.rs`, `src-tauri/src/lib.rs`
- Test: existing channel frontend/backend tests

- [ ] **Step 1: Search for old channel APIs**

Run:

```bash
rg -n "channelGetStatus|channel_save_config|channelSaveConfig|channelBeginDingtalkRegistration|channelPollDingtalkRegistration|onChannelStatus|CHANNEL_STATUS|ChannelStatusState|dingtalkStatus|channel:status|DingtalkChannelConfig|ChannelStatusPayload|ChannelStatus" src src-tauri
```

Expected: No matches except historical docs or intentionally retained compatibility comments. If matches exist in production code, update them to new platform domain APIs.

- [ ] **Step 2: Run targeted frontend tests**

Run:

```bash
pnpm vitest run src/stores/channelStore.test.ts src/features/channel/ChannelConfig.test.tsx src/features/channel/ChannelConfigDetails.test.tsx src/features/channel/ChannelPage.test.tsx
```

Expected: PASS.

- [ ] **Step 3: Run TypeScript build check**

Run:

```bash
pnpm exec tsc -b --pretty false
```

Expected: PASS.

- [ ] **Step 4: Run Rust path/config/router tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml storage::user_scoped_paths::tests::channel_platform_paths_are_nested_under_user_scope --lib
cargo test --manifest-path src-tauri/Cargo.toml connector::channel::config_store::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml connector::channel::router::tests --lib
```

Expected: PASS.

- [ ] **Step 5: Run Rust check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 6: Run broader channel frontend test folder**

Run:

```bash
pnpm vitest run src/features/channel
```

Expected: PASS.

- [ ] **Step 7: Commit final cleanup**

If any cleanup changes were required:

```bash
git add src src-tauri
git commit -m "test(channel): verify domain redesign integration"
```

If no cleanup changes were required, do not create an empty commit.

---

## Self-review Checklist

- Spec coverage:
  - Domain state split: Task 1, Task 2, Task 3, Task 4.
  - New user-scoped platform paths: Task 1, Task 2.
  - No legacy migration: Task 2 and Task 8 old API/path search.
  - No robot name: Task 1 config schema excludes robot name; Task 5/6 UI excludes robot name.
  - AppSecret masked and reveal-confirmed: Task 2, Task 6.
  - Connected config read-only: Task 6, Task 7.
  - Remove confirmation and credential deletion: Task 3, Task 7.
  - Switch enable/disable without deleting config: Task 2, Task 3, Task 7.
  - Coming-soon platforms: Task 1, Task 2, Task 3, Task 7.

- Type consistency:
  - Frontend `ChannelPlatformState` uses `connection: 'configError'`, not tagged union status.
  - Backend `ChannelConnectionState::ConfigError` stores error text separately in `last_error`.
  - AppSecret reveal returns `string` directly from IPC and is never stored in `channelStore`.
  - Registration poll result returns `client_secret: None` on success; UI uses `config.appSecretMasked`.

- Validation philosophy:
  - Backend tasks start with focused unit tests before implementation.
  - Frontend tasks start with component/store tests before implementation.
  - Full verification avoids broad Cargo filtered test binaries unless targeted checks are insufficient.
