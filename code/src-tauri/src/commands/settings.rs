use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use crate::storage::file_store::AppStorage;
use crate::storage::crypto::SecureStorage;
use crate::models::settings::AppSettings;
use crate::llm::providers::LlmProviderTrait;
use crate::llm::providers::{
    claude::ClaudeProvider,
    deepseek_v3::DeepSeekV3Provider,
    openai::OpenAiProvider,
    qwen::QwenProvider,
    volcano::VolcanoProvider,
};

/// Fields that contain sensitive API keys and should be encrypted at rest.
const SENSITIVE_KEYS: &[&str] = &["primaryApiKey", "tavilyApiKey"];

/// Check if a key is sensitive (standard fields or per-provider apiKey:* prefix).
fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEYS.contains(&key) || key.starts_with("apiKey:")
}

/// Get current application settings.
/// Reads from SQLite settings table and deserializes into AppSettings.
/// API key fields are decrypted if SecureStorage is available.
#[tauri::command]
pub async fn get_settings(
    db: State<'_, Arc<AppStorage>>,
    crypto: State<'_, Option<Arc<SecureStorage>>>,
) -> Result<AppSettings, String> {
    let settings_map = db.get_all_settings().map_err(|e| e.to_string())?;

    // If no settings stored yet, return defaults
    if settings_map.is_empty() {
        return Ok(AppSettings::default());
    }

    // Use type-safe parsing instead of JSON deserialization
    // (DB stores all values as strings, including booleans and numbers)
    let mut settings = AppSettings::from_string_map(&settings_map);

    // Decrypt sensitive fields if SecureStorage is available
    if let Some(ss) = crypto.as_ref() {
        settings.primary_api_key = decrypt_if_encrypted(ss, &settings.primary_api_key);
        settings.tavily_api_key = decrypt_if_encrypted(ss, &settings.tavily_api_key);
    }

    Ok(settings)
}

/// Update application settings.
/// Serializes AppSettings fields as individual key-value pairs.
/// API key fields are encrypted before storage if SecureStorage is available.
/// Also persists the API key under `apiKey:{primaryModel}` for per-provider storage.
#[tauri::command]
pub async fn update_settings(
    db: State<'_, Arc<AppStorage>>,
    crypto: State<'_, Option<Arc<SecureStorage>>>,
    settings: AppSettings,
) -> Result<(), String> {
    // Serialize to JSON, then store each field as a separate key
    let json = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
    if let serde_json::Value::Object(map) = json {
        for (key, value) in map {
            let mut value_str = match &value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };

            // Encrypt sensitive fields before storage
            if is_sensitive_key(&key) && !value_str.is_empty() {
                if let Some(ss) = crypto.as_ref() {
                    match ss.encrypt(&value_str) {
                        Ok(encrypted) => value_str = encrypted,
                        Err(e) => {
                            log::warn!("Failed to encrypt '{}': {}, storing as plaintext", key, e);
                        }
                    }
                }
            }

            db.set_setting(&key, &value_str).map_err(|e| e.to_string())?;
        }
    }

    // Also persist API key under apiKey:{provider} for per-provider storage
    if !settings.primary_api_key.is_empty() {
        let per_provider_key = format!("apiKey:{}", settings.primary_model);
        let mut value_str = settings.primary_api_key.clone();
        if let Some(ss) = crypto.as_ref() {
            match ss.encrypt(&value_str) {
                Ok(encrypted) => value_str = encrypted,
                Err(e) => {
                    log::warn!("Failed to encrypt per-provider key: {}, storing as plaintext", e);
                }
            }
        }
        db.set_setting(&per_provider_key, &value_str).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Get the list of providers that have a saved API key.
/// Returns provider identifiers (e.g. "deepseek-v3", "openai") that have non-empty keys.
#[tauri::command]
pub async fn get_configured_providers(
    db: State<'_, Arc<AppStorage>>,
    crypto: State<'_, Option<Arc<SecureStorage>>>,
) -> Result<Vec<String>, String> {
    let prefix_map = db.get_settings_by_prefix("apiKey:").map_err(|e| e.to_string())?;
    let mut providers: Vec<String> = Vec::new();

    for (key, value) in &prefix_map {
        // key format: "apiKey:deepseek-v3"
        if let Some(provider) = key.strip_prefix("apiKey:") {
            // Decrypt to check if non-empty
            let decrypted = if let Some(ss) = crypto.as_ref() {
                decrypt_if_encrypted(ss, value)
            } else {
                value.clone()
            };
            if !decrypted.is_empty() {
                providers.push(provider.to_string());
            }
        }
    }

    // Migration compat: if current primaryModel not in list but primaryApiKey is non-empty,
    // include it
    let settings_map = db.get_all_settings().map_err(|e| e.to_string())?;
    let settings = AppSettings::from_string_map(&settings_map);
    let primary_key = if let Some(ss) = crypto.as_ref() {
        decrypt_if_encrypted(ss, &settings.primary_api_key)
    } else {
        settings.primary_api_key.clone()
    };

    if !primary_key.is_empty() && !providers.contains(&settings.primary_model) {
        providers.push(settings.primary_model.clone());
    }

    Ok(providers)
}

/// Switch the active provider. Loads the API key from per-provider storage
/// and updates primaryModel + primaryApiKey in settings.
#[tauri::command]
pub async fn switch_provider(
    db: State<'_, Arc<AppStorage>>,
    crypto: State<'_, Option<Arc<SecureStorage>>>,
    provider: String,
) -> Result<(), String> {
    // Load the per-provider key
    let per_provider_key = format!("apiKey:{}", provider);
    let stored_key = db.get_setting(&per_provider_key)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();

    let decrypted_key = if let Some(ss) = crypto.as_ref() {
        decrypt_if_encrypted(ss, &stored_key)
    } else {
        stored_key
    };

    // Update primaryModel and primaryApiKey
    db.set_setting("primaryModel", &provider).map_err(|e| e.to_string())?;

    // Encrypt primaryApiKey before storing
    let mut key_to_store = decrypted_key.clone();
    if !key_to_store.is_empty() {
        if let Some(ss) = crypto.as_ref() {
            match ss.encrypt(&key_to_store) {
                Ok(encrypted) => key_to_store = encrypted,
                Err(e) => {
                    log::warn!("Failed to encrypt key during switch: {}", e);
                }
            }
        }
    }
    db.set_setting("primaryApiKey", &key_to_store).map_err(|e| e.to_string())?;

    Ok(())
}

/// Get all per-provider API keys (decrypted). Used by the settings modal to
/// populate all provider tabs. Returns a map of provider → plaintext key.
#[tauri::command]
pub async fn get_all_provider_keys(
    db: State<'_, Arc<AppStorage>>,
    crypto: State<'_, Option<Arc<SecureStorage>>>,
) -> Result<HashMap<String, String>, String> {
    let prefix_map = db.get_settings_by_prefix("apiKey:").map_err(|e| e.to_string())?;
    let mut result: HashMap<String, String> = HashMap::new();

    for (key, value) in &prefix_map {
        if let Some(provider) = key.strip_prefix("apiKey:") {
            let decrypted = if let Some(ss) = crypto.as_ref() {
                decrypt_if_encrypted(ss, value)
            } else {
                value.clone()
            };
            if !decrypted.is_empty() {
                result.insert(provider.to_string(), decrypted);
            }
        }
    }

    // Migration compat: include current primaryApiKey if not already present
    let settings_map = db.get_all_settings().map_err(|e| e.to_string())?;
    let settings = AppSettings::from_string_map(&settings_map);
    let primary_key = if let Some(ss) = crypto.as_ref() {
        decrypt_if_encrypted(ss, &settings.primary_api_key)
    } else {
        settings.primary_api_key.clone()
    };

    if !primary_key.is_empty() && !result.contains_key(&settings.primary_model) {
        result.insert(settings.primary_model.clone(), primary_key);
    }

    Ok(result)
}

/// Batch-save all provider API keys. Used by the settings modal to persist
/// all configured keys at once. Keys map: provider → plaintext key.
/// Empty keys are removed from storage.
#[tauri::command]
pub async fn update_all_provider_keys(
    db: State<'_, Arc<AppStorage>>,
    crypto: State<'_, Option<Arc<SecureStorage>>>,
    keys: HashMap<String, String>,
) -> Result<(), String> {
    for (provider, plaintext_key) in &keys {
        let db_key = format!("apiKey:{}", provider);
        if plaintext_key.is_empty() {
            // Remove empty keys
            db.delete_setting(&db_key).map_err(|e| e.to_string())?;
        } else {
            let mut value_to_store = plaintext_key.clone();
            if let Some(ss) = crypto.as_ref() {
                match ss.encrypt(&value_to_store) {
                    Ok(encrypted) => value_to_store = encrypted,
                    Err(e) => {
                        log::warn!("Failed to encrypt key for '{}': {}, storing as plaintext", provider, e);
                    }
                }
            }
            db.set_setting(&db_key, &value_to_store).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Validate an API key against the provider.
/// Makes a real HTTP validation call using the provider implementation.
#[tauri::command]
pub async fn validate_api_key(
    provider: String,
    api_key: String,
) -> Result<bool, String> {
    if api_key.trim().is_empty() {
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
        _ => {
            // Unknown provider, just check non-empty
            return Ok(true);
        }
    };

    match result {
        Ok(valid) => Ok(valid),
        Err(e) => {
            log::warn!("API key validation failed for provider '{}': {}", provider, e);
            Err(format!("Validation failed: {}", e))
        }
    }
}

/// Attempt to decrypt a value.
/// - Plaintext (no colon): returned as-is
/// - Encrypted (has colon): decrypted and returned
/// - Decryption fails: returns empty string (NOT the ciphertext, to prevent
///   double-encryption when the user saves settings back)
fn decrypt_if_encrypted(ss: &SecureStorage, value: &str) -> String {
    if value.is_empty() {
        return value.to_string();
    }
    // Encrypted format is "nonce_hex:ciphertext_hex" — check for the colon marker
    if value.contains(':') {
        match ss.decrypt(value) {
            Ok(plaintext) => plaintext,
            Err(e) => {
                log::warn!("Failed to decrypt setting value (len={}): {}", value.len(), e);
                String::new()
            }
        }
    } else {
        value.to_string() // Plaintext (legacy)
    }
}
