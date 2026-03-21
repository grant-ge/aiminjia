//! Credential store — persists internal app config via AppStorage settings.

use std::sync::Arc;
use anyhow::Result;
use crate::storage::file_store::AppStorage;
use super::types::InternalAppConfig;

const INTERNAL_CONFIG_KEY: &str = "internal_app_config";

pub fn save_internal_config(storage: &Arc<AppStorage>, config: &InternalAppConfig) -> Result<()> {
    let json = serde_json::to_string(config)?;
    storage.set_setting(INTERNAL_CONFIG_KEY, &json)?;
    Ok(())
}

pub fn load_internal_config(storage: &Arc<AppStorage>) -> Result<Option<InternalAppConfig>> {
    match storage.get_setting(INTERNAL_CONFIG_KEY)? {
        Some(json) if !json.is_empty() => {
            let config: InternalAppConfig = serde_json::from_str(&json)?;
            Ok(Some(config))
        }
        _ => Ok(None),
    }
}
