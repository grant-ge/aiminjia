use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::storage::crypto::SecureStorage;

use super::super::dingtalk::registration::OPEN_CLAW_SOURCE;
use super::super::types::{
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

    // ----- Platform-keyed API (PR0d) -----

    /// 通用：返回 `<channels_dir>/<platform>` 的目录。Phase 1+ 新平台
    /// （飞书 / 企微 / Telegram / WhatsApp / 个微）直接复用，避免每个
    /// 平台再加一套独立的 `<plat>_dir` 方法。
    pub fn platform_dir(&self, platform: Platform) -> PathBuf {
        self.channels_dir.join(platform.as_str())
    }

    /// 通用：返回 `<channels_dir>/<platform>/config.json`。
    pub fn platform_config_path(&self, platform: Platform) -> PathBuf {
        self.platform_dir(platform).join("config.json")
    }

    /// 通用：返回 `<channels_dir>/<platform>/sessions.json`。
    pub fn platform_sessions_path(&self, platform: Platform) -> PathBuf {
        self.platform_dir(platform).join("sessions.json")
    }

    pub fn all_platform_states(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<Vec<ChannelPlatformState>> {
        Ok(vec![
            self.dingtalk_state(connection.clone(), last_error.clone())?,
            self.feishu_state(connection.clone(), last_error.clone())?,
            Self::wechat_state_stub(),
            self.wecom_state(connection.clone(), last_error.clone())?,
            self.telegram_state(connection, last_error)?,
        ])
    }

    pub fn coming_soon_state(platform: Platform) -> ChannelPlatformState {
        ChannelPlatformState::coming_soon(platform)
    }

    /// Phase 5 MVP: wechat reports as `available` so the user can click "配置"
    /// and trigger a real iLink scan-to-login flow. Backend doesn't persist
    /// credentials yet (Phase 5 PR3 task), so `configured` / `enabled` stay
    /// false — the card never reaches the "已连接" state in this MVP cut.
    pub fn wechat_state_stub() -> ChannelPlatformState {
        ChannelPlatformState {
            platform: Platform::Wechat,
            capability: ChannelCapability::Available,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        }
    }

    /// PR1 stub: 飞书侧 capability=Available 但 configured=false / enabled=false。
    /// PR2 真正实现读 config / 解密 secret 后替换。
    pub fn feishu_state_stub(
        &self,
        _connection: ChannelConnectionState,
        _last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        Ok(ChannelPlatformState {
            platform: Platform::Feishu,
            capability: ChannelCapability::Available,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        })
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

    // ----- Feishu (Phase 1 PR2) -----

    pub fn read_feishu_config(
        &self,
    ) -> Result<Option<crate::connector::im::feishu::types::FeishuStoredConfig>> {
        let path = self.platform_config_path(Platform::Feishu);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        let config: crate::connector::im::feishu::types::FeishuStoredConfig =
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
        validate_feishu_config(&config)?;
        Ok(Some(config))
    }

    pub fn save_feishu_registration(
        &self,
        app_id: String,
        app_secret_plain: String,
    ) -> Result<ChannelPlatformState> {
        use crate::connector::im::feishu::types::{
            FeishuStoredConfig, FeishuStoredCredentials, FeishuStoredMetadata,
        };
        let app_id = non_empty(app_id, "app_id")?;
        let secret = non_empty(app_secret_plain, "app_secret")?;
        let (app_secret_encrypted, app_secret_storage) = self.encrypt_secret(&secret)?;
        let now = now_rfc3339();
        let existing_created_at = self
            .read_feishu_config()?
            .map(|c| c.metadata.created_at)
            .unwrap_or_else(|| now.clone());
        let config = FeishuStoredConfig {
            schema_version: 1,
            platform: Platform::Feishu,
            configured: true,
            enabled: true,
            credentials: FeishuStoredCredentials {
                app_id,
                app_secret_encrypted,
                app_secret_storage,
            },
            metadata: FeishuStoredMetadata {
                created_at: existing_created_at,
                updated_at: now,
            },
        };
        self.write_feishu_config(&config)?;
        self.feishu_state(ChannelConnectionState::Disconnected, None)
    }

    pub fn decrypt_feishu_config(
        &self,
    ) -> Result<(
        crate::connector::im::feishu::types::FeishuStoredConfig,
        String,
    )> {
        let config = self
            .read_feishu_config()?
            .ok_or_else(|| anyhow::anyhow!("Feishu channel is not configured"))?;
        let secret = match (&config.credentials.app_secret_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.app_secret_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "Feishu AppSecret is marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.app_secret_encrypted.clone()
            }
        };
        Ok((config, secret))
    }

    pub fn feishu_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let Some(config) = self.read_feishu_config()? else {
            return self.feishu_state_stub(connection, last_error);
        };
        let connection = if !config.enabled {
            ChannelConnectionState::Disconnected
        } else {
            connection
        };
        // Mirror `dingtalk_state` → `config_view`: propagate decrypt errors via `?`
        // so callers (and ultimately the UI) see "配置解密失败" rather than a healthy
        // mask with an empty secret behind it. SecureStorage missing / Keychain
        // unavailable / nonce tampered → all surface here.
        Ok(ChannelPlatformState {
            platform: Platform::Feishu,
            capability: ChannelCapability::Available,
            configured: config.configured,
            enabled: config.enabled,
            connection,
            config: Some(self.feishu_config_view(&config)?),
            last_connected_at: None,
            last_error,
        })
    }

    pub fn set_feishu_enabled(&self, enabled: bool) -> Result<ChannelPlatformState> {
        let mut config = self
            .read_feishu_config()?
            .ok_or_else(|| anyhow::anyhow!("Feishu channel is not configured"))?;
        config.enabled = enabled;
        config.metadata.updated_at = now_rfc3339();
        self.write_feishu_config(&config)?;
        let connection = if enabled {
            ChannelConnectionState::Connecting
        } else {
            ChannelConnectionState::Disconnected
        };
        self.feishu_state(connection, None)
    }

    pub fn remove_feishu(&self) -> Result<ChannelPlatformState> {
        let path = self.platform_config_path(Platform::Feishu);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        self.feishu_state_stub(ChannelConnectionState::Unconfigured, None)
    }

    pub fn reveal_feishu_secret(&self) -> Result<String> {
        let (_, secret) = self.decrypt_feishu_config()?;
        Ok(secret)
    }

    // ----- Wecom (Phase 2 PR6a) -----
    //
    // 企微 aibot 凭证 = (bot_id, secret) 两件套。secret 走 SecureStorage（mirrors
    // feishu app_secret / dingtalk app_secret 路径）。`display_name` 是用户给账号
    // 起的别名，可空；UI 列表上展示。落盘到 `<channels_dir>/wecom/config.json`。

    pub fn read_wecom_config(
        &self,
    ) -> Result<Option<crate::connector::im::wecom::types::WecomStoredConfig>> {
        let path = self.platform_config_path(Platform::Wecom);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        let config: crate::connector::im::wecom::types::WecomStoredConfig =
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
        validate_wecom_config(&config)?;
        Ok(Some(config))
    }

    pub fn add_wecom(
        &self,
        bot_id: String,
        secret: String,
        display_name: Option<String>,
    ) -> Result<ChannelPlatformState> {
        use crate::connector::im::wecom::types::{
            WecomStoredConfig, WecomStoredCredentials, WecomStoredMetadata,
        };
        let bot_id = non_empty(bot_id, "bot_id")?;
        let secret = non_empty(secret, "secret")?;
        let display_name = display_name.and_then(normalize_optional);
        let (secret_encrypted, secret_storage) = self.encrypt_secret(&secret)?;
        let now = now_rfc3339();
        let existing_created_at = self
            .read_wecom_config()?
            .map(|c| c.metadata.created_at)
            .unwrap_or_else(|| now.clone());
        let config = WecomStoredConfig {
            schema_version: 1,
            platform: Platform::Wecom,
            configured: true,
            enabled: true,
            credentials: WecomStoredCredentials {
                bot_id,
                secret_encrypted,
                secret_storage,
            },
            display_name,
            metadata: WecomStoredMetadata {
                created_at: existing_created_at,
                updated_at: now,
            },
        };
        self.write_wecom_config(&config)?;
        self.wecom_state(ChannelConnectionState::Disconnected, None)
    }

    pub fn decrypt_wecom_config(
        &self,
    ) -> Result<(
        crate::connector::im::wecom::types::WecomStoredConfig,
        String,
    )> {
        let config = self
            .read_wecom_config()?
            .ok_or_else(|| anyhow::anyhow!("Wecom channel is not configured"))?;
        let secret = match (&config.credentials.secret_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.secret_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "Wecom secret is marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.secret_encrypted.clone()
            }
        };
        Ok((config, secret))
    }

    /// Returns plain `(bot_id, secret)` after decrypting. Returns `None` if not
    /// configured. `async` to match the task contract (PR6b will use this from
    /// the manager, which is already on a tokio runtime); the body itself is
    /// sync-friendly today (SecureStorage decrypt is blocking).
    pub async fn get_wecom_credentials(&self) -> Result<Option<(String, String)>> {
        if self.read_wecom_config()?.is_none() {
            return Ok(None);
        }
        let (config, secret) = self.decrypt_wecom_config()?;
        Ok(Some((config.credentials.bot_id, secret)))
    }

    /// PR6a: 上链 wecom_state 从 PR5 的 stub 升级成真正读取持久化配置。
    /// 没有 wecom.json → 返回 unconfigured-but-available（保持原 stub 行为）。
    /// 有配置 → 返回 configured / enabled / masked secret view，跟 feishu 对齐。
    pub fn wecom_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let Some(config) = self.read_wecom_config()? else {
            return Ok(ChannelPlatformState {
                platform: Platform::Wecom,
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
        // Mirror `feishu_state` → `feishu_config_view`: propagate decrypt errors
        // via `?` so callers (and ultimately the UI) see "配置解密失败" rather
        // than a healthy mask with an empty secret behind it.
        Ok(ChannelPlatformState {
            platform: Platform::Wecom,
            capability: ChannelCapability::Available,
            configured: config.configured,
            enabled: config.enabled,
            connection,
            config: Some(self.wecom_config_view(&config)?),
            last_connected_at: None,
            last_error,
        })
    }

    pub fn set_wecom_enabled(&self, enabled: bool) -> Result<ChannelPlatformState> {
        let mut config = self
            .read_wecom_config()?
            .ok_or_else(|| anyhow::anyhow!("Wecom channel is not configured"))?;
        config.enabled = enabled;
        config.metadata.updated_at = now_rfc3339();
        self.write_wecom_config(&config)?;
        let connection = if enabled {
            ChannelConnectionState::Connecting
        } else {
            ChannelConnectionState::Disconnected
        };
        self.wecom_state(connection, None)
    }

    pub fn remove_wecom(&self) -> Result<ChannelPlatformState> {
        // Clear SecureStorage entry first so a corrupted secure_storage backend
        // doesn't strand an orphaned blob after the config file is gone. The
        // current `SecureStorage` API is content-addressed (encrypt returns a
        // self-describing blob and decrypt parses it), so there is no per-key
        // delete to call — wiping the file removes the only reference. If we
        // ever migrate to a keyring-backed entry we should delete that entry
        // here; the comment is a forward marker for that future PR.
        let path = self.platform_config_path(Platform::Wecom);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(ChannelPlatformState {
            platform: Platform::Wecom,
            capability: ChannelCapability::Available,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        })
    }

    pub fn reveal_wecom_secret(&self) -> Result<String> {
        let (_, secret) = self.decrypt_wecom_config()?;
        Ok(secret)
    }

    fn write_wecom_config(
        &self,
        config: &crate::connector::im::wecom::types::WecomStoredConfig,
    ) -> Result<()> {
        validate_wecom_config(config)?;
        let dir = self.platform_dir(Platform::Wecom);
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(config)?;
        let final_path = self.platform_config_path(Platform::Wecom);
        let temp_path = dir.join(format!(
            ".config.json.{}.{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        write_config_file_securely(&temp_path, content.as_bytes())?;
        std::fs::rename(&temp_path, final_path)?;
        Ok(())
    }

    /// Mirror of `feishu_config_view` — decrypts secret and propagates
    /// SecureStorage errors via `?`. `ChannelConfigView.app_key` is reused to
    /// surface `bot_id` (the camelCase shape on the wire is the same regardless
    /// of the platform's native credential name), so the frontend keeps a
    /// single field to render across platforms.
    fn wecom_config_view(
        &self,
        config: &crate::connector::im::wecom::types::WecomStoredConfig,
    ) -> Result<ChannelConfigView> {
        let secret = match (&config.credentials.secret_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.secret_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "Wecom secret is marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.secret_encrypted.clone()
            }
        };
        Ok(ChannelConfigView {
            platform: Platform::Wecom,
            app_key: config.credentials.bot_id.clone(),
            app_secret_masked: mask_secret(&secret),
            robot_code: String::new(),
            robot_code_source: RobotCodeSource::AppKeyFallback,
            source: crate::connector::im::wecom::types::WECOM_AIBOT_SOURCE.to_string(),
            created_at: config.metadata.created_at.clone(),
            updated_at: config.metadata.updated_at.clone(),
        })
    }

    // ----- Telegram (MVP) ------------------------------------------------------
    //
    // bot_token 走 SecureStorage。bot_id / bot_username / first_name 明文。
    // allowlist 直接落 config.json（尺寸天然有限，单文件原子写盘最简单）。

    pub fn read_telegram_config(
        &self,
    ) -> Result<Option<crate::connector::im::telegram::types::TelegramStoredConfig>> {
        let path = self.platform_config_path(Platform::Telegram);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        let config: crate::connector::im::telegram::types::TelegramStoredConfig =
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
        validate_telegram_config(&config)?;
        Ok(Some(config))
    }

    pub fn save_telegram_registration(
        &self,
        token: String,
        bot_id: String,
        bot_username: String,
        bot_first_name: String,
    ) -> Result<ChannelPlatformState> {
        use crate::connector::im::telegram::types::{
            TelegramBotInfo, TelegramStoredConfig, TelegramStoredCredentials,
            TelegramStoredMetadata,
        };
        let token = non_empty(token, "bot_token")?;
        let bot_id = non_empty(bot_id, "bot_id")?;
        let bot_username = non_empty(bot_username, "bot_username")?;
        let (bot_token_encrypted, bot_token_storage) = self.encrypt_secret(&token)?;
        let now = now_rfc3339();
        let existing = self.read_telegram_config()?;
        let existing_created_at = existing
            .as_ref()
            .map(|c| c.metadata.created_at.clone())
            .unwrap_or_else(|| now.clone());
        let existing_allowlist = existing
            .as_ref()
            .map(|c| c.allowlist.clone())
            .unwrap_or_default();
        let config = TelegramStoredConfig {
            schema_version: 1,
            platform: Platform::Telegram,
            configured: true,
            enabled: true,
            credentials: TelegramStoredCredentials {
                bot_token_encrypted,
                bot_token_storage,
            },
            bot: TelegramBotInfo {
                bot_id,
                bot_username,
                bot_first_name,
            },
            allowlist: existing_allowlist,
            metadata: TelegramStoredMetadata {
                created_at: existing_created_at,
                updated_at: now,
            },
        };
        self.write_telegram_config(&config)?;
        self.telegram_state(ChannelConnectionState::Disconnected, None)
    }

    pub fn decrypt_telegram_config(
        &self,
    ) -> Result<(
        crate::connector::im::telegram::types::TelegramStoredConfig,
        String,
    )> {
        let config = self
            .read_telegram_config()?
            .ok_or_else(|| anyhow::anyhow!("Telegram channel is not configured"))?;
        let token = match (&config.credentials.bot_token_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.bot_token_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => {
                anyhow::bail!(
                    "Telegram bot_token marked SecureStorage but SecureStorage is unavailable"
                )
            }
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.bot_token_encrypted.clone()
            }
        };
        Ok((config, token))
    }

    pub fn telegram_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let Some(config) = self.read_telegram_config()? else {
            return Ok(ChannelPlatformState {
                platform: Platform::Telegram,
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
            platform: Platform::Telegram,
            capability: ChannelCapability::Available,
            configured: config.configured,
            enabled: config.enabled,
            connection,
            config: Some(self.telegram_config_view(&config)?),
            last_connected_at: None,
            last_error,
        })
    }

    pub fn set_telegram_enabled(&self, enabled: bool) -> Result<ChannelPlatformState> {
        let mut config = self
            .read_telegram_config()?
            .ok_or_else(|| anyhow::anyhow!("Telegram channel is not configured"))?;
        config.enabled = enabled;
        config.metadata.updated_at = now_rfc3339();
        self.write_telegram_config(&config)?;
        let connection = if enabled {
            ChannelConnectionState::Connecting
        } else {
            ChannelConnectionState::Disconnected
        };
        self.telegram_state(connection, None)
    }

    pub fn remove_telegram(&self) -> Result<ChannelPlatformState> {
        let path = self.platform_config_path(Platform::Telegram);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(ChannelPlatformState {
            platform: Platform::Telegram,
            capability: ChannelCapability::Available,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        })
    }

    pub fn reveal_telegram_token(&self) -> Result<String> {
        let (_, token) = self.decrypt_telegram_config()?;
        Ok(token)
    }

    // ----- WhatsApp -----

    /// WhatsApp 单账号约定（spec §0.4 #1）：config.json 存在 = 已配对。
    /// 镜像 `telegram_state` 结构，但不暴露 jid/push_name 给 ChannelConfigView。
    pub fn whatsapp_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let config_path = self.platform_config_path(Platform::Whatsapp);
        let configured = match crate::connector::im::whatsapp::config::read(&config_path) {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                log::warn!("[whatsapp] read config.json failed: {e:#}");
                false
            }
        };
        let connection = if !configured {
            ChannelConnectionState::Unconfigured
        } else {
            connection
        };
        Ok(ChannelPlatformState {
            platform: Platform::Whatsapp,
            capability: ChannelCapability::Available,
            configured,
            enabled: configured, // 单账号约定：configured = enabled
            connection,
            config: None, // 不暴露 jid / push_name 给 ChannelConfigView
            last_connected_at: None,
            last_error,
        })
    }

    /// 把一个 user 加入 allowlist。重复加入幂等（按 user_id 去重）。
    pub fn telegram_add_allowlist_entry(
        &self,
        entry: crate::connector::im::telegram::types::AllowlistEntry,
    ) -> Result<()> {
        let mut config = self
            .read_telegram_config()?
            .ok_or_else(|| anyhow::anyhow!("Telegram channel is not configured"))?;
        if !config.allowlist.iter().any(|e| e.user_id == entry.user_id) {
            config.allowlist.push(entry);
            config.metadata.updated_at = now_rfc3339();
            self.write_telegram_config(&config)?;
        }
        Ok(())
    }

    pub fn telegram_remove_allowlist_user(&self, user_id: i64) -> Result<()> {
        let Some(mut config) = self.read_telegram_config()? else {
            return Ok(());
        };
        let before = config.allowlist.len();
        config.allowlist.retain(|e| e.user_id != user_id);
        if config.allowlist.len() != before {
            config.metadata.updated_at = now_rfc3339();
            self.write_telegram_config(&config)?;
        }
        Ok(())
    }

    pub fn telegram_is_in_allowlist(&self, user_id: i64) -> Result<bool> {
        let Some(config) = self.read_telegram_config()? else {
            return Ok(false);
        };
        Ok(config.allowlist.iter().any(|e| e.user_id == user_id))
    }

    fn write_telegram_config(
        &self,
        config: &crate::connector::im::telegram::types::TelegramStoredConfig,
    ) -> Result<()> {
        validate_telegram_config(config)?;
        let dir = self.platform_dir(Platform::Telegram);
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(config)?;
        let final_path = self.platform_config_path(Platform::Telegram);
        let temp_path = dir.join(format!(
            ".config.json.{}.{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        write_config_file_securely(&temp_path, content.as_bytes())?;
        std::fs::rename(&temp_path, final_path)?;
        Ok(())
    }

    fn telegram_config_view(
        &self,
        config: &crate::connector::im::telegram::types::TelegramStoredConfig,
    ) -> Result<ChannelConfigView> {
        let token = match (&config.credentials.bot_token_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.bot_token_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "Telegram bot_token marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.bot_token_encrypted.clone()
            }
        };
        Ok(ChannelConfigView {
            platform: Platform::Telegram,
            app_key: config.bot.bot_username.clone(),
            app_secret_masked: mask_secret(&token),
            robot_code: config.bot.bot_id.clone(),
            robot_code_source: RobotCodeSource::Registration,
            source: crate::connector::im::telegram::types::TELEGRAM_BOT_TOKEN_SOURCE.to_string(),
            created_at: config.metadata.created_at.clone(),
            updated_at: config.metadata.updated_at.clone(),
        })
    }

    // ---- WeChat (iLink scan-to-login, Phase 5) ----------------------------

    pub fn read_wechat_config(
        &self,
    ) -> Result<Option<crate::connector::im::wechat::types::WechatStoredConfig>> {
        let path = self.platform_config_path(Platform::Wechat);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        let config: crate::connector::im::wechat::types::WechatStoredConfig =
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
        validate_wechat_config(&config)?;
        Ok(Some(config))
    }

    /// 扫码登录确认后调一次。`bot_token` 走 SecureStorage 加密，
    /// 其它字段（ilink_bot_id / ilink_user_id / effective_base_url）落明文
    /// JSON。已存在配置则覆盖。
    pub fn save_wechat_registration(
        &self,
        bot_token: String,
        ilink_bot_id: String,
        ilink_user_id: String,
        effective_base_url: String,
    ) -> Result<ChannelPlatformState> {
        use crate::connector::im::wechat::types::{
            WechatStoredBot, WechatStoredConfig, WechatStoredCredentials, WechatStoredMetadata,
        };
        let bot_token = non_empty(bot_token, "bot_token")?;
        let ilink_bot_id = non_empty(ilink_bot_id, "ilink_bot_id")?;
        let ilink_user_id = non_empty(ilink_user_id, "ilink_user_id")?;
        let effective_base_url = non_empty(effective_base_url, "effective_base_url")?;
        let (bot_token_encrypted, bot_token_storage) = self.encrypt_secret(&bot_token)?;
        let now = now_rfc3339();
        let existing_created_at = self
            .read_wechat_config()?
            .map(|c| c.metadata.created_at)
            .unwrap_or_else(|| now.clone());
        let config = WechatStoredConfig {
            schema_version: 1,
            platform: Platform::Wechat,
            configured: true,
            enabled: true,
            credentials: WechatStoredCredentials {
                bot_token_encrypted,
                bot_token_storage,
            },
            bot: WechatStoredBot {
                ilink_bot_id,
                ilink_user_id,
                effective_base_url,
            },
            metadata: WechatStoredMetadata {
                created_at: existing_created_at,
                updated_at: now,
            },
        };
        self.write_wechat_config(&config)?;
        self.wechat_state(ChannelConnectionState::Disconnected, None)
    }

    pub fn decrypt_wechat_config(
        &self,
    ) -> Result<(
        crate::connector::im::wechat::types::WechatStoredConfig,
        String,
    )> {
        let config = self
            .read_wechat_config()?
            .ok_or_else(|| anyhow::anyhow!("WeChat channel is not configured"))?;
        let bot_token = match (&config.credentials.bot_token_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.bot_token_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "WeChat bot_token is marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.bot_token_encrypted.clone()
            }
        };
        Ok((config, bot_token))
    }

    /// 真实状态版（之前是 stub）。
    pub fn wechat_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let Some(config) = self.read_wechat_config()? else {
            return Ok(Self::wechat_state_stub());
        };
        let connection = if !config.enabled {
            ChannelConnectionState::Disconnected
        } else {
            connection
        };
        Ok(ChannelPlatformState {
            platform: Platform::Wechat,
            capability: ChannelCapability::Available,
            configured: config.configured,
            enabled: config.enabled,
            connection,
            config: Some(self.wechat_config_view(&config)?),
            last_connected_at: None,
            last_error,
        })
    }

    pub fn set_wechat_enabled(&self, enabled: bool) -> Result<ChannelPlatformState> {
        let mut config = self
            .read_wechat_config()?
            .ok_or_else(|| anyhow::anyhow!("WeChat channel is not configured"))?;
        config.enabled = enabled;
        config.metadata.updated_at = now_rfc3339();
        self.write_wechat_config(&config)?;
        let connection = if enabled {
            ChannelConnectionState::Connecting
        } else {
            ChannelConnectionState::Disconnected
        };
        self.wechat_state(connection, None)
    }

    pub fn remove_wechat(&self) -> Result<ChannelPlatformState> {
        let path = self.platform_config_path(Platform::Wechat);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(Self::wechat_state_stub())
    }

    fn write_wechat_config(
        &self,
        config: &crate::connector::im::wechat::types::WechatStoredConfig,
    ) -> Result<()> {
        validate_wechat_config(config)?;
        let dir = self.platform_dir(Platform::Wechat);
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(config)?;
        let final_path = self.platform_config_path(Platform::Wechat);
        let temp_path = dir.join(format!(
            ".config.json.{}.{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        write_config_file_securely(&temp_path, content.as_bytes())?;
        std::fs::rename(&temp_path, final_path)?;
        Ok(())
    }

    fn wechat_config_view(
        &self,
        config: &crate::connector::im::wechat::types::WechatStoredConfig,
    ) -> Result<ChannelConfigView> {
        let bot_token = match (&config.credentials.bot_token_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.bot_token_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "WeChat bot_token is marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.bot_token_encrypted.clone()
            }
        };
        Ok(ChannelConfigView {
            platform: Platform::Wechat,
            // `app_key` 字段在 wechat 路径下用来 surface ilink_user_id（前端
            // sidebar 想看的"登录身份"信息），跟企微 surface bot_id 同模式。
            app_key: config.bot.ilink_user_id.clone(),
            app_secret_masked: mask_secret(&bot_token),
            robot_code: config.bot.ilink_bot_id.clone(),
            robot_code_source: RobotCodeSource::Registration,
            source: crate::connector::im::wechat::types::WECHAT_ILINK_SCAN_SOURCE.to_string(),
            created_at: config.metadata.created_at.clone(),
            updated_at: config.metadata.updated_at.clone(),
        })
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

    fn write_feishu_config(
        &self,
        config: &crate::connector::im::feishu::types::FeishuStoredConfig,
    ) -> Result<()> {
        validate_feishu_config(config)?;
        let dir = self.platform_dir(Platform::Feishu);
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(config)?;
        let final_path = self.platform_config_path(Platform::Feishu);
        let temp_path = dir.join(format!(
            ".config.json.{}.{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        write_config_file_securely(&temp_path, content.as_bytes())?;
        std::fs::rename(&temp_path, final_path)?;
        Ok(())
    }

    /// Mirror of `config_view` for feishu — decrypts secret and propagates
    /// SecureStorage / parse errors via `?` so callers see real failures.
    fn feishu_config_view(
        &self,
        config: &crate::connector::im::feishu::types::FeishuStoredConfig,
    ) -> Result<ChannelConfigView> {
        let secret = match (&config.credentials.app_secret_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.app_secret_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "Feishu AppSecret is marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.app_secret_encrypted.clone()
            }
        };
        Ok(ChannelConfigView {
            platform: Platform::Feishu,
            app_key: config.credentials.app_id.clone(),
            app_secret_masked: mask_secret(&secret),
            robot_code: String::new(),
            robot_code_source: RobotCodeSource::AppKeyFallback,
            source: crate::connector::im::feishu::registration::FEISHU_DEVICE_CODE_SOURCE
                .to_string(),
            created_at: config.metadata.created_at.clone(),
            updated_at: config.metadata.updated_at.clone(),
        })
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

fn validate_feishu_config(
    config: &crate::connector::im::feishu::types::FeishuStoredConfig,
) -> Result<()> {
    if config.schema_version != 1 {
        anyhow::bail!(
            "Invalid Feishu config schema_version: expected 1, got {}",
            config.schema_version
        );
    }
    if config.platform != Platform::Feishu {
        anyhow::bail!(
            "Invalid Feishu config platform: expected feishu, got {}",
            config.platform.as_str()
        );
    }
    if !config.configured {
        anyhow::bail!("Invalid Feishu config: configured must be true");
    }
    validate_feishu_non_empty(&config.credentials.app_id, "credentials.app_id")?;
    validate_feishu_non_empty(
        &config.credentials.app_secret_encrypted,
        "credentials.app_secret_encrypted",
    )?;
    validate_feishu_non_empty(&config.metadata.created_at, "metadata.created_at")?;
    validate_feishu_non_empty(&config.metadata.updated_at, "metadata.updated_at")?;
    Ok(())
}

fn validate_feishu_non_empty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("Invalid Feishu config: {field} is required");
    }
    Ok(())
}

fn validate_wecom_config(
    config: &crate::connector::im::wecom::types::WecomStoredConfig,
) -> Result<()> {
    if config.schema_version != 1 {
        anyhow::bail!(
            "Invalid Wecom config schema_version: expected 1, got {}",
            config.schema_version
        );
    }
    if config.platform != Platform::Wecom {
        anyhow::bail!(
            "Invalid Wecom config platform: expected wecom, got {}",
            config.platform.as_str()
        );
    }
    if !config.configured {
        anyhow::bail!("Invalid Wecom config: configured must be true");
    }
    validate_wecom_non_empty(&config.credentials.bot_id, "credentials.bot_id")?;
    validate_wecom_non_empty(
        &config.credentials.secret_encrypted,
        "credentials.secret_encrypted",
    )?;
    validate_wecom_non_empty(&config.metadata.created_at, "metadata.created_at")?;
    validate_wecom_non_empty(&config.metadata.updated_at, "metadata.updated_at")?;
    Ok(())
}

fn validate_wecom_non_empty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("Invalid Wecom config: {field} is required");
    }
    Ok(())
}

fn validate_wechat_config(
    config: &crate::connector::im::wechat::types::WechatStoredConfig,
) -> Result<()> {
    if config.schema_version != 1 {
        anyhow::bail!(
            "Invalid WeChat config schema_version: expected 1, got {}",
            config.schema_version
        );
    }
    if config.platform != Platform::Wechat {
        anyhow::bail!(
            "Invalid WeChat config platform: expected wechat, got {}",
            config.platform.as_str()
        );
    }
    if !config.configured {
        anyhow::bail!("Invalid WeChat config: configured must be true");
    }
    validate_wechat_non_empty(
        &config.credentials.bot_token_encrypted,
        "credentials.bot_token_encrypted",
    )?;
    validate_wechat_non_empty(&config.bot.ilink_bot_id, "bot.ilink_bot_id")?;
    validate_wechat_non_empty(&config.bot.ilink_user_id, "bot.ilink_user_id")?;
    validate_wechat_non_empty(&config.bot.effective_base_url, "bot.effective_base_url")?;
    validate_wechat_non_empty(&config.metadata.created_at, "metadata.created_at")?;
    validate_wechat_non_empty(&config.metadata.updated_at, "metadata.updated_at")?;
    Ok(())
}

fn validate_wechat_non_empty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("Invalid WeChat config: {field} is required");
    }
    Ok(())
}

fn validate_telegram_config(
    config: &crate::connector::im::telegram::types::TelegramStoredConfig,
) -> Result<()> {
    if config.schema_version != 1 {
        anyhow::bail!(
            "Invalid Telegram config schema_version: expected 1, got {}",
            config.schema_version
        );
    }
    if config.platform != Platform::Telegram {
        anyhow::bail!(
            "Invalid Telegram config platform: expected telegram, got {}",
            config.platform.as_str()
        );
    }
    if !config.configured {
        anyhow::bail!("Invalid Telegram config: configured must be true");
    }
    validate_telegram_non_empty(
        &config.credentials.bot_token_encrypted,
        "credentials.bot_token_encrypted",
    )?;
    validate_telegram_non_empty(&config.bot.bot_id, "bot.bot_id")?;
    validate_telegram_non_empty(&config.bot.bot_username, "bot.bot_username")?;
    validate_telegram_non_empty(&config.metadata.created_at, "metadata.created_at")?;
    validate_telegram_non_empty(&config.metadata.updated_at, "metadata.updated_at")?;
    Ok(())
}

fn validate_telegram_non_empty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("Invalid Telegram config: {field} is required");
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

    fn valid_feishu_config() -> crate::connector::im::feishu::types::FeishuStoredConfig {
        use crate::connector::im::feishu::types::{
            FeishuStoredConfig, FeishuStoredCredentials, FeishuStoredMetadata,
        };
        FeishuStoredConfig {
            schema_version: 1,
            platform: Platform::Feishu,
            configured: true,
            enabled: true,
            credentials: FeishuStoredCredentials {
                app_id: "cli_abc123".into(),
                app_secret_encrypted: "super-secret-value".into(),
                app_secret_storage: SecretStorageKind::PlaintextFallback,
            },
            metadata: FeishuStoredMetadata {
                created_at: "2026-05-18T00:00:00Z".into(),
                updated_at: "2026-05-18T00:00:01Z".into(),
            },
        }
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
    fn platform_paths_use_lowercase_platform_subdirectory() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        // 1) 通用 helper 按 Platform::as_str() 拼小写子目录。
        let feishu_dir = store.platform_dir(Platform::Feishu);
        let feishu_cfg = store.platform_config_path(Platform::Feishu);
        let feishu_sess = store.platform_sessions_path(Platform::Feishu);
        assert_eq!(feishu_dir, dir.path().join("channels/feishu"));
        assert_eq!(feishu_cfg, dir.path().join("channels/feishu/config.json"));
        assert_eq!(
            feishu_sess,
            dir.path().join("channels/feishu/sessions.json")
        );
        // 2) Dingtalk 走通用 helper 必须与既有的 dingtalk_* 等价（迁移期共存兼容）。
        assert_eq!(store.platform_dir(Platform::Dingtalk), store.dingtalk_dir());
        assert_eq!(
            store.platform_config_path(Platform::Dingtalk),
            store.dingtalk_config_path()
        );
        assert_eq!(
            store.platform_sessions_path(Platform::Dingtalk),
            store.dingtalk_sessions_path()
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

    #[test]
    fn save_feishu_registration_writes_enabled_config_and_masks_secret() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let state = store
            .save_feishu_registration("cli_abc123".into(), "supersecret".into())
            .unwrap();
        assert!(state.configured);
        assert!(state.enabled);
        let view = state.config.unwrap();
        assert_eq!(view.app_key, "cli_abc123");
        assert_eq!(view.app_secret_masked, "••••••••••••cret");
        assert!(store.platform_config_path(Platform::Feishu).exists());
    }

    #[test]
    fn set_feishu_enabled_false_keeps_config() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store
            .save_feishu_registration("cli_x".into(), "secret".into())
            .unwrap();
        let state = store.set_feishu_enabled(false).unwrap();
        assert!(!state.enabled);
        assert!(state.configured);
        assert!(store.platform_config_path(Platform::Feishu).exists());
    }

    #[test]
    fn remove_feishu_deletes_config() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store
            .save_feishu_registration("cli_x".into(), "secret".into())
            .unwrap();
        let state = store.remove_feishu().unwrap();
        assert!(!state.configured);
        assert!(!store.platform_config_path(Platform::Feishu).exists());
    }

    #[test]
    fn read_feishu_config_rejects_invalid_schema_platform_and_required_fields() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let cases = [
            ("bad schema", "schemaVersion", serde_json::json!(2)),
            ("bad platform", "platform", serde_json::json!("dingtalk")),
            ("not configured", "configured", serde_json::json!(false)),
            ("empty app id", "credentials.appId", serde_json::json!("")),
            (
                "empty app secret",
                "credentials.appSecretEncrypted",
                serde_json::json!(""),
            ),
            ("empty created", "metadata.createdAt", serde_json::json!("")),
            ("empty updated", "metadata.updatedAt", serde_json::json!("")),
        ];

        for (name, path, value) in cases {
            let mut json = serde_json::to_value(valid_feishu_config()).unwrap();
            set_json_path(&mut json, path, value);
            std::fs::create_dir_all(store.platform_dir(Platform::Feishu)).unwrap();
            std::fs::write(
                store.platform_config_path(Platform::Feishu),
                serde_json::to_string_pretty(&json).unwrap(),
            )
            .unwrap();

            assert!(
                store.read_feishu_config().is_err(),
                "case should fail validation: {name}"
            );
        }
    }
}
