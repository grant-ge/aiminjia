use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::storage::crypto::SecureStorage;

use super::dingtalk_registration::OPEN_CLAW_SOURCE;
use super::types::{
    ChannelCapability, ChannelConfigView, ChannelConnectionState, ChannelPlatformState,
    DingtalkStoredBot, DingtalkStoredConfig, DingtalkStoredCredentials, DingtalkStoredMetadata,
    DingtalkStoredRegistration, Platform, RobotCodeSource, SecretStorageKind,
};

#[derive(Clone)]
pub struct ChannelConfigStore {
    channels_dir: PathBuf,
    secure_storage: Option<Arc<SecureStorage>>,
}

impl ChannelConfigStore {
    pub fn new(channels_dir: PathBuf, secure_storage: Option<Arc<SecureStorage>>) -> Self {
        Self {
            channels_dir,
            secure_storage,
        }
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
        Ok(vec![
            self.dingtalk_state(connection, last_error)?,
            Self::coming_soon_state(Platform::Feishu),
            Self::coming_soon_state(Platform::Wechat),
            Self::coming_soon_state(Platform::Wecom),
        ])
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
        let robot_code = robot_code.and_then(normalize_optional);
        let (robot_code, robot_code_source) = match robot_code {
            Some(value) => (value, RobotCodeSource::Registration),
            None => (trimmed_app_key.clone(), RobotCodeSource::AppKeyFallback),
        };
        let (app_secret_encrypted, app_secret_storage) = self.encrypt_secret(&trimmed_secret)?;

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
                app_secret_storage,
            },
            bot: DingtalkStoredBot {
                robot_code,
                robot_code_source,
            },
            registration: DingtalkStoredRegistration {
                source: OPEN_CLAW_SOURCE.to_string(),
            },
            metadata: DingtalkStoredMetadata {
                created_at: existing_created_at,
                updated_at: now,
            },
        };

        self.write_dingtalk_config(&config)?;
        self.dingtalk_state(ChannelConnectionState::Disconnected, None)
    }

    pub fn set_dingtalk_enabled(&self, enabled: bool) -> Result<ChannelPlatformState> {
        let mut config = self
            .read_dingtalk_config()?
            .ok_or_else(|| anyhow::anyhow!("DingTalk channel is not configured"))?;
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
            .ok_or_else(|| anyhow::anyhow!("DingTalk channel is not configured"))?;
        self.decrypt_secret(&config.credentials)
    }

    pub fn read_dingtalk_config(&self) -> Result<Option<DingtalkStoredConfig>> {
        let path = self.dingtalk_config_path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        let config = serde_json::from_str::<DingtalkStoredConfig>(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        validate_dingtalk_config(&config)?;
        Ok(Some(config))
    }

    pub fn decrypt_dingtalk_config(&self) -> Result<(DingtalkStoredConfig, String)> {
        let config = self
            .read_dingtalk_config()?
            .ok_or_else(|| anyhow::anyhow!("DingTalk channel is not configured"))?;
        let secret = self.decrypt_secret(&config.credentials)?;
        Ok((config, secret))
    }

    fn write_dingtalk_config(&self, config: &DingtalkStoredConfig) -> Result<()> {
        validate_dingtalk_config(config)?;
        let dir = self.dingtalk_dir();
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(config)?;
        let final_path = self.dingtalk_config_path();
        let temp_path = dir.join(format!(
            ".config.json.{}.{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        write_config_file_securely(&temp_path, content.as_bytes())?;
        std::fs::rename(&temp_path, final_path)?;
        Ok(())
    }

    fn config_view(&self, config: &DingtalkStoredConfig) -> Result<ChannelConfigView> {
        let secret = self.decrypt_secret(&config.credentials)?;
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

    fn encrypt_secret(&self, secret: &str) -> Result<(String, SecretStorageKind)> {
        match &self.secure_storage {
            Some(storage) => Ok((storage.encrypt(secret)?, SecretStorageKind::SecureStorage)),
            None => Ok((secret.to_string(), SecretStorageKind::PlaintextFallback)),
        }
    }

    fn decrypt_secret(&self, credentials: &DingtalkStoredCredentials) -> Result<String> {
        match (&credentials.app_secret_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&credentials.app_secret_encrypted)
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "DingTalk AppSecret is marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                Ok(credentials.app_secret_encrypted.clone())
            }
        }
    }
}

fn validate_dingtalk_config(config: &DingtalkStoredConfig) -> Result<()> {
    if config.schema_version != 1 {
        anyhow::bail!(
            "Invalid DingTalk config schema_version: expected 1, got {}",
            config.schema_version
        );
    }
    if config.platform != Platform::Dingtalk {
        anyhow::bail!(
            "Invalid DingTalk config platform: expected dingtalk, got {}",
            config.platform.as_str()
        );
    }
    if !config.configured {
        anyhow::bail!("Invalid DingTalk config: configured must be true");
    }
    validate_non_empty_field(&config.credentials.app_key, "credentials.app_key")?;
    validate_non_empty_field(
        &config.credentials.app_secret_encrypted,
        "credentials.app_secret_encrypted",
    )?;
    validate_non_empty_field(&config.bot.robot_code, "bot.robot_code")?;
    validate_non_empty_field(&config.registration.source, "registration.source")?;
    if config.registration.source != OPEN_CLAW_SOURCE {
        anyhow::bail!(
            "Invalid DingTalk config registration.source: expected {}, got {}",
            OPEN_CLAW_SOURCE,
            config.registration.source
        );
    }
    validate_non_empty_field(&config.metadata.created_at, "metadata.created_at")?;
    validate_non_empty_field(&config.metadata.updated_at, "metadata.updated_at")?;
    Ok(())
}

fn validate_non_empty_field(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("Invalid DingTalk config: {field} is required");
    }
    Ok(())
}

fn write_config_file_securely(path: &std::path::Path, content: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content)?;
        file.sync_all()?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, content)?;
        Ok(())
    }
}

fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn non_empty(value: String, field: &str) -> Result<String> {
    normalize_optional(value).ok_or_else(|| anyhow::anyhow!("{field} is required"))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 4 {
        return "••••••••••••".to_string();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn valid_config() -> DingtalkStoredConfig {
        DingtalkStoredConfig {
            schema_version: 1,
            platform: Platform::Dingtalk,
            configured: true,
            enabled: true,
            credentials: DingtalkStoredCredentials {
                app_key: "ding-app-key".into(),
                app_secret_encrypted: "super-secret-value".into(),
                app_secret_storage: SecretStorageKind::PlaintextFallback,
            },
            bot: DingtalkStoredBot {
                robot_code: "robot-001".into(),
                robot_code_source: RobotCodeSource::Registration,
            },
            registration: DingtalkStoredRegistration {
                source: OPEN_CLAW_SOURCE.into(),
            },
            metadata: DingtalkStoredMetadata {
                created_at: "2026-05-07T00:00:00Z".into(),
                updated_at: "2026-05-07T00:00:01Z".into(),
            },
        }
    }

    fn store_in(dir: &TempDir) -> ChannelConfigStore {
        ChannelConfigStore::new(dir.path().join("channels"), None)
    }

    #[test]
    fn mask_secret_hides_short_secrets() {
        assert_eq!(mask_secret("abc"), "••••••••••••");
        assert_eq!(mask_secret("abcd"), "••••••••••••");
        assert_eq!(mask_secret("abcde"), "••••••••••••bcde");
    }

    #[test]
    fn save_registration_marks_plaintext_fallback_storage() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);

        store
            .save_dingtalk_registration("ding-app-key".into(), "super-secret-value".into(), None)
            .unwrap();

        let config = store.read_dingtalk_config().unwrap().unwrap();
        assert_eq!(
            config.credentials.app_secret_storage,
            SecretStorageKind::PlaintextFallback
        );
        assert_eq!(
            store.reveal_dingtalk_secret().unwrap(),
            "super-secret-value"
        );
    }

    #[test]
    fn secure_marked_secret_requires_secure_storage() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let mut config = valid_config();
        config.credentials.app_secret_encrypted = "nonce:ciphertext".into();
        config.credentials.app_secret_storage = SecretStorageKind::SecureStorage;
        store.write_dingtalk_config(&config).unwrap();

        let err = store.reveal_dingtalk_secret().unwrap_err().to_string();
        assert!(
            err.contains("SecureStorage") && err.contains("unavailable"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn read_dingtalk_config_rejects_invalid_schema_platform_and_required_fields() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let cases = [
            ("bad schema", "schemaVersion", serde_json::json!(2)),
            ("bad platform", "platform", serde_json::json!("feishu")),
            ("not configured", "configured", serde_json::json!(false)),
            ("empty app key", "credentials.appKey", serde_json::json!("")),
            (
                "empty app secret",
                "credentials.appSecretEncrypted",
                serde_json::json!(""),
            ),
            ("empty source", "registration.source", serde_json::json!("")),
            ("empty robot", "bot.robotCode", serde_json::json!("")),
            ("empty created", "metadata.createdAt", serde_json::json!("")),
            ("empty updated", "metadata.updatedAt", serde_json::json!("")),
        ];

        for (name, path, value) in cases {
            let mut json = serde_json::to_value(valid_config()).unwrap();
            set_json_path(&mut json, path, value);
            std::fs::create_dir_all(store.dingtalk_dir()).unwrap();
            std::fs::write(
                store.dingtalk_config_path(),
                serde_json::to_string_pretty(&json).unwrap(),
            )
            .unwrap();

            assert!(
                store.read_dingtalk_config().is_err(),
                "case should fail validation: {name}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn write_dingtalk_config_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);

        store.write_dingtalk_config(&valid_config()).unwrap();

        let mode = std::fs::metadata(store.dingtalk_config_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    fn set_json_path(value: &mut serde_json::Value, path: &str, replacement: serde_json::Value) {
        let mut current = value;
        let parts: Vec<&str> = path.split('.').collect();
        for key in &parts[..parts.len() - 1] {
            current = current.get_mut(*key).unwrap();
        }
        current[parts[parts.len() - 1]] = replacement;
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
        assert!(
            persisted.contains("super-secret-value"),
            "plaintext fallback is expected when SecureStorage is unavailable"
        );
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

        let state = store
            .dingtalk_state(ChannelConnectionState::Disconnected, None)
            .unwrap();
        let view = state.config.unwrap();
        assert_eq!(view.app_secret_masked, "••••••••••••alue");
        assert!(!serde_json::to_string(&view)
            .unwrap()
            .contains("super-secret-value"));
        assert_eq!(
            store.reveal_dingtalk_secret().unwrap(),
            "super-secret-value"
        );
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
