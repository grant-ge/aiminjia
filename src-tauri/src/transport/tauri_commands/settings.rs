use std::collections::HashMap;
use std::sync::Arc;

use crate::llm::providers::LlmProviderTrait;
use crate::llm::providers::{
    claude::ClaudeProvider, custom::CustomProvider, deepseek_v3::DeepSeekV3Provider,
    openai::OpenAiProvider, qwen::QwenProvider, volcano::VolcanoProvider,
};
use crate::models::settings::AppSettings;
use crate::storage::crypto::SecureStorage;
use crate::storage::file_store::AppStorage;

/// Fields that contain sensitive API keys and should be encrypted at rest.
const SENSITIVE_KEYS: &[&str] = &["primaryApiKey", "tavilyApiKey", "bochaApiKey"];

/// Check if a key is sensitive (standard fields or per-provider apiKey:* prefix).
fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEYS.contains(&key) || key.starts_with("apiKey:")
}

/// Attempt to decrypt a value.
/// - Plaintext (no colon): returned as-is
/// - Encrypted (has colon): decrypted and returned
/// - Decryption fails: returns empty string (NOT the ciphertext, to prevent
///   double-encryption when the user saves settings back)
pub fn decrypt_if_encrypted(ss: &SecureStorage, value: &str) -> String {
    if value.is_empty() {
        return value.to_string();
    }
    // Encrypted format is "nonce_hex:ciphertext_hex" — check for the colon marker
    if value.contains(':') {
        match ss.decrypt(value) {
            Ok(plaintext) => plaintext,
            Err(e) => {
                log::warn!(
                    "Failed to decrypt setting value (len={}): {}",
                    value.len(),
                    e
                );
                String::new()
            }
        }
    } else {
        value.to_string() // Plaintext (legacy)
    }
}

#[derive(Clone)]
pub struct TauriSettingsCommandAdapter {
    db: Arc<AppStorage>,
    crypto: Option<Arc<SecureStorage>>,
}

impl TauriSettingsCommandAdapter {
    pub fn new(db: Arc<AppStorage>, crypto: Option<Arc<SecureStorage>>) -> Self {
        Self { db, crypto }
    }

    pub fn get_settings(&self) -> Result<AppSettings, String> {
        let settings_map = self.db.get_all_settings().map_err(|e| e.to_string())?;

        if settings_map.is_empty() {
            return Ok(AppSettings::default());
        }

        let mut settings = AppSettings::from_string_map(&settings_map);

        if let Some(ss) = self.crypto.as_ref() {
            settings.primary_api_key = decrypt_if_encrypted(ss, &settings.primary_api_key);
            settings.tavily_api_key = decrypt_if_encrypted(ss, &settings.tavily_api_key);
            settings.bocha_api_key = decrypt_if_encrypted(ss, &settings.bocha_api_key);
        }

        Ok(settings)
    }

    pub fn update_settings(&self, settings: AppSettings) -> Result<(), String> {
        let json = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
        if let serde_json::Value::Object(map) = json {
            for (key, value) in map {
                let mut value_str = match &value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };

                if is_sensitive_key(&key) && !value_str.is_empty() {
                    if let Some(ss) = self.crypto.as_ref() {
                        match ss.encrypt(&value_str) {
                            Ok(encrypted) => value_str = encrypted,
                            Err(e) => {
                                log::warn!(
                                    "Failed to encrypt '{}': {}, storing as plaintext",
                                    key,
                                    e
                                );
                            }
                        }
                    }
                }

                self.db
                    .set_setting(&key, &value_str)
                    .map_err(|e| e.to_string())?;
            }
        }

        if !settings.primary_api_key.is_empty() {
            let per_provider_key = format!("apiKey:{}", settings.primary_model);
            let mut value_str = settings.primary_api_key.clone();
            if let Some(ss) = self.crypto.as_ref() {
                match ss.encrypt(&value_str) {
                    Ok(encrypted) => value_str = encrypted,
                    Err(e) => {
                        log::warn!(
                            "Failed to encrypt per-provider key: {}, storing as plaintext",
                            e
                        );
                    }
                }
            }
            self.db
                .set_setting(&per_provider_key, &value_str)
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn get_configured_providers(&self) -> Result<Vec<String>, String> {
        let prefix_map = self
            .db
            .get_settings_by_prefix("apiKey:")
            .map_err(|e| e.to_string())?;
        let mut providers: Vec<String> = Vec::new();

        for (key, value) in &prefix_map {
            if let Some(provider) = key.strip_prefix("apiKey:") {
                let decrypted = if let Some(ss) = self.crypto.as_ref() {
                    decrypt_if_encrypted(ss, value)
                } else {
                    value.clone()
                };
                if !decrypted.is_empty() {
                    providers.push(provider.to_string());
                }
            }
        }

        let settings_map = self.db.get_all_settings().map_err(|e| e.to_string())?;
        let settings = AppSettings::from_string_map(&settings_map);
        let primary_key = if let Some(ss) = self.crypto.as_ref() {
            decrypt_if_encrypted(ss, &settings.primary_api_key)
        } else {
            settings.primary_api_key.clone()
        };

        if !primary_key.is_empty() && !providers.contains(&settings.primary_model) {
            providers.push(settings.primary_model.clone());
        }

        Ok(providers)
    }

    pub fn switch_provider(&self, provider: String) -> Result<(), String> {
        let per_provider_key = format!("apiKey:{}", provider);
        let stored_key = self
            .db
            .get_setting(&per_provider_key)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();

        let decrypted_key = if let Some(ss) = self.crypto.as_ref() {
            decrypt_if_encrypted(ss, &stored_key)
        } else {
            stored_key
        };

        self.db
            .set_setting("primaryModel", &provider)
            .map_err(|e| e.to_string())?;

        let mut key_to_store = decrypted_key.clone();
        if !key_to_store.is_empty() {
            if let Some(ss) = self.crypto.as_ref() {
                match ss.encrypt(&key_to_store) {
                    Ok(encrypted) => key_to_store = encrypted,
                    Err(e) => {
                        log::warn!("Failed to encrypt key during switch: {}", e);
                    }
                }
            }
        }
        self.db
            .set_setting("primaryApiKey", &key_to_store)
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn get_all_provider_keys(&self) -> Result<HashMap<String, String>, String> {
        let prefix_map = self
            .db
            .get_settings_by_prefix("apiKey:")
            .map_err(|e| e.to_string())?;
        let mut result: HashMap<String, String> = HashMap::new();

        for (key, value) in &prefix_map {
            if let Some(provider) = key.strip_prefix("apiKey:") {
                let decrypted = if let Some(ss) = self.crypto.as_ref() {
                    decrypt_if_encrypted(ss, value)
                } else {
                    value.clone()
                };
                if !decrypted.is_empty() {
                    result.insert(provider.to_string(), decrypted);
                }
            }
        }

        let settings_map = self.db.get_all_settings().map_err(|e| e.to_string())?;
        let settings = AppSettings::from_string_map(&settings_map);
        let primary_key = if let Some(ss) = self.crypto.as_ref() {
            decrypt_if_encrypted(ss, &settings.primary_api_key)
        } else {
            settings.primary_api_key.clone()
        };

        if !primary_key.is_empty() && !result.contains_key(&settings.primary_model) {
            result.insert(settings.primary_model.clone(), primary_key);
        }

        Ok(result)
    }

    pub fn update_all_provider_keys(&self, keys: HashMap<String, String>) -> Result<(), String> {
        for (provider, plaintext_key) in &keys {
            let db_key = format!("apiKey:{}", provider);
            if plaintext_key.is_empty() {
                self.db.delete_setting(&db_key).map_err(|e| e.to_string())?;
            } else {
                let mut value_to_store = plaintext_key.clone();
                if let Some(ss) = self.crypto.as_ref() {
                    match ss.encrypt(&value_to_store) {
                        Ok(encrypted) => value_to_store = encrypted,
                        Err(e) => {
                            log::warn!(
                                "Failed to encrypt key for '{}': {}, storing as plaintext",
                                provider,
                                e
                            );
                        }
                    }
                }
                self.db
                    .set_setting(&db_key, &value_to_store)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub async fn validate_api_key(
        &self,
        provider: String,
        api_key: String,
    ) -> Result<bool, String> {
        if provider != "custom" && api_key.trim().is_empty() {
            return Ok(false);
        }

        let result = match provider.as_str() {
            "deepseek-v3" => {
                let p = DeepSeekV3Provider::new(api_key);
                p.validate_key().await
            }
            "qwen-plus" => {
                let p = QwenProvider::new(api_key);
                p.validate_key().await
            }
            "openai" => {
                let p = OpenAiProvider::new(api_key);
                p.validate_key().await
            }
            "claude" => {
                let p = ClaudeProvider::new(api_key, None);
                p.validate_key().await
            }
            "volcano" => {
                let p = VolcanoProvider::new(api_key, String::new());
                p.validate_key().await
            }
            "custom" => {
                let settings_map = self.db.get_all_settings().map_err(|e| e.to_string())?;
                let settings = AppSettings::from_string_map(&settings_map);
                if settings.custom_model_endpoint.is_empty()
                    || settings.custom_model_name.is_empty()
                {
                    return Err("请先填写 API Endpoint 和模型名称".to_string());
                }
                let p = CustomProvider::new(
                    api_key,
                    settings.custom_model_endpoint,
                    settings.custom_model_name,
                );
                p.validate_key().await
            }
            _ => {
                return Ok(true);
            }
        };

        match result {
            Ok(valid) => Ok(valid),
            Err(e) => {
                log::warn!(
                    "API key validation failed for provider '{}': {}",
                    provider,
                    e
                );
                Err(format!("Validation failed: {}", e))
            }
        }
    }
}
