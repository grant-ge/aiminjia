//! Credential store — persists SaaS connector config and credentials via AppStorage settings.

use std::sync::Arc;
use anyhow::Result;
use crate::storage::file_store::AppStorage;
use super::types::{ConnectorConfig, SaasCredential};

const CONFIG_KEY: &str = "saas_config";

fn cred_key(app_id: u64) -> String {
    format!("saas_cred_{}", app_id)
}

pub fn save_config(storage: &Arc<AppStorage>, config: &ConnectorConfig) -> Result<()> {
    let json = serde_json::to_string(config)?;
    storage.set_setting(CONFIG_KEY, &json)?;
    Ok(())
}

pub fn load_config(storage: &Arc<AppStorage>) -> Result<Option<ConnectorConfig>> {
    match storage.get_setting(CONFIG_KEY)? {
        Some(json) if !json.is_empty() => {
            let config: ConnectorConfig = serde_json::from_str(&json)?;
            Ok(Some(config))
        }
        _ => Ok(None),
    }
}

pub fn save_credential(storage: &Arc<AppStorage>, app_id: u64, credential: &SaasCredential) -> Result<()> {
    let json = serde_json::to_string(credential)?;
    storage.set_setting(&cred_key(app_id), &json)?;
    Ok(())
}

pub fn load_credential(storage: &Arc<AppStorage>, app_id: u64) -> Result<Option<SaasCredential>> {
    match storage.get_setting(&cred_key(app_id))? {
        Some(json) if !json.is_empty() => {
            let cred: SaasCredential = serde_json::from_str(&json)?;
            Ok(Some(cred))
        }
        _ => Ok(None),
    }
}

pub fn clear_credential(storage: &Arc<AppStorage>, app_id: u64) -> Result<()> {
    storage.set_setting(&cred_key(app_id), "")?;
    Ok(())
}
