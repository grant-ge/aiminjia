//! ConnectorEngine — SaaS connector runtime for fetching data from enterprise systems.

use std::sync::Arc;
use once_cell::sync::Lazy;
use tokio::sync::RwLock;
use log::{info, warn};
use serde_json::Value;

use crate::storage::file_store::AppStorage;
use super::types::*;
use super::credential_store;

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

pub struct ConnectorEngine {
    storage: Arc<AppStorage>,
    config: RwLock<Option<ConnectorConfig>>,
    api_base_url: String,
    session_key: String,
}

impl ConnectorEngine {
    pub fn new(storage: Arc<AppStorage>, api_base_url: String, session_key: String) -> Self {
        Self {
            storage,
            config: RwLock::new(None),
            api_base_url,
            session_key,
        }
    }

    /// Load cached config from local storage on startup.
    pub async fn init(&self) {
        if let Ok(Some(config)) = credential_store::load_config(&self.storage) {
            info!("[CONNECTOR] Loaded cached config with {} apps", config.apps.len());
            *self.config.write().await = Some(config);
        }
    }

    /// Build context string for LLM — app names + capability names/descriptions only.
    /// Only includes enabled apps. Flags apps that need credential setup.
    pub async fn build_saas_context(&self) -> String {
        let config = self.config.read().await;
        let config = match config.as_ref() {
            Some(c) => c,
            None => return String::new(),
        };

        let mut ctx = String::new();
        for app in &config.apps {
            if app.status != "active" {
                continue;
            }
            // Skip apps the employee has not enabled
            let enabled = app.enabled.unwrap_or(false);
            if !enabled {
                continue;
            }

            ctx.push_str(&format!("## {}\n", app.name));
            if let Some(ref summary) = app.summary {
                ctx.push_str(&format!("{}\n", summary));
            }

            // Check if credential is needed but missing
            let auth_required = app.auth_required.unwrap_or(true);
            let has_credential = app.has_credential.unwrap_or(false);
            if auth_required && !has_credential {
                ctx.push_str("⚠ 此系统需要配置凭证才能使用。请提醒用户在 SaaS 工作台设置账号密码，或通过 fetch_saas_data 的 set_credential 操作输入凭证。\n");
            }

            for cap in &app.capabilities {
                ctx.push_str(&format!("- {}: {}\n", cap.name, cap.description));
            }
            ctx.push('\n');
        }
        ctx
    }

    /// Get full sample for a specific capability (called by tool).
    pub async fn get_capability_sample(
        &self,
        app_name: &str,
        capability_name: &str,
    ) -> Result<SaasCapabilitySample, String> {
        let config = self.config.read().await;
        let config = config.as_ref().ok_or("SaaS connector config not loaded")?;

        let (app, cap) = find_app_capability(config, app_name, capability_name)?;

        // Call lotus API for full sample
        let url = format!(
            "{}/api/employee/saas-apps/{}/capabilities/{}/sample",
            self.api_base_url, app.id, cap.id
        );

        let resp = HTTP_CLIENT
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.session_key))
            .send()
            .await
            .map_err(|e| format!("Failed to fetch capability sample: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Lotus API error ({}): {}", status, body));
        }

        let sample: SaasCapabilitySample = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse sample response: {}", e))?;

        Ok(sample)
    }

    /// Execute an API request through the connected SaaS system.
    pub async fn execute_request(
        &self,
        app_name: &str,
        capability_name: &str,
        parameters: Option<Value>,
    ) -> Result<String, String> {
        let config = self.config.read().await;
        let config = config.as_ref().ok_or("SaaS connector config not loaded")?;

        let (app, cap) = find_app_capability(config, app_name, capability_name)?;

        // Check if auth is required
        let auth_required = app.auth_required.unwrap_or(true);

        // Load credential from local store
        let cred = credential_store::load_credential(&self.storage, app.id)
            .map_err(|e| format!("Failed to load credential: {}", e))?;

        if auth_required {
            let cred_ref = cred.as_ref();
            if cred_ref.is_none() || (cred_ref.unwrap().access_token.is_none() && cred_ref.unwrap().api_credential.is_none()) {
                return Err(format!(
                    "'{app_name}' 需要配置凭证才能访问。\n\
                     请告诉用户：\n\
                     1. 在 SaaS 工作台点击该应用的「设置凭证」按钮输入 API Key/Token\n\
                     2. 或者直接告诉我凭证信息，我可以通过 fetch_saas_data(action='set_credential') 帮你设置"
                ));
            }
        }

        let cred = cred.unwrap_or(SaasCredential {
            access_token: None,
            api_credential: None,
            token_expires_at: None,
            oauth_user_info: None,
            status: "none".to_string(),
        });

        // Build URL from base_url + path_template
        let mut path = cap.path_template.clone();

        // Fill path parameters from parameters JSON
        if let Some(ref params) = parameters {
            if let Some(obj) = params.as_object() {
                for (key, value) in obj {
                    let placeholder = format!("{{{}}}", key);
                    let val_str = match value {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    path = path.replace(&placeholder, &val_str);

                    // Also handle [可变:xxx] markers
                    let marker = format!("[可变:{}]", key);
                    path = path.replace(&marker, &val_str);
                }
            }
        }

        // Replace system variables in path
        let now = chrono::Local::now();
        path = path.replace("{{today}}", &now.format("%Y-%m-%d").to_string());
        path = path.replace("{{now}}", &now.format("%Y-%m-%dT%H:%M:%S").to_string());

        // Replace credential-based system variables ({{token}}, {{corp_id}}, {{user_id}})
        let token_str = cred.access_token.as_deref()
            .or(cred.api_credential.as_deref())
            .unwrap_or("");
        path = path.replace("{{token}}", token_str);

        // Extract corp_id and user_id from oauth_user_info if available
        let (corp_id, user_id) = extract_oauth_ids(&cred);
        path = path.replace("{{corp_id}}", &corp_id);
        path = path.replace("{{user_id}}", &user_id);

        let final_url = format!("{}{}", app.base_url.trim_end_matches('/'), path);

        // URL whitelist check: ensure final URL starts with app.base_url
        if !final_url.starts_with(&app.base_url) {
            return Err(format!(
                "URL security check failed: {} does not start with {}",
                final_url, app.base_url
            ));
        }

        info!("[CONNECTOR] Executing {} {} for app '{}'", cap.http_method, final_url, app_name);

        // Build request with credential
        let mut req = match cap.http_method.to_uppercase().as_str() {
            "GET" => HTTP_CLIENT.get(&final_url),
            "POST" => HTTP_CLIENT.post(&final_url),
            "PUT" => HTTP_CLIENT.put(&final_url),
            "DELETE" => HTTP_CLIENT.delete(&final_url),
            "PATCH" => HTTP_CLIENT.patch(&final_url),
            _ => return Err(format!("Unsupported HTTP method: {}", cap.http_method)),
        };

        // Add auth header (engine-controlled, not LLM-controlled — safe by design)
        // LLM only provides parameters, never headers — header whitelist enforced structurally
        if let Some(ref token) = cred.access_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        } else if let Some(ref api_key) = cred.api_credential {
            // TODO: respect app.auth_type (bearer_token / api_key / custom_header)
            // For now, default to Bearer token
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }

        // Add request body for POST/PUT/PATCH, with system variable replacement
        if matches!(cap.http_method.to_uppercase().as_str(), "POST" | "PUT" | "PATCH") {
            if let Some(ref params) = parameters {
                // Replace system variables in the JSON body
                let mut body_str = serde_json::to_string(params).unwrap_or_default();
                body_str = body_str.replace("{{token}}", token_str);
                body_str = body_str.replace("{{corp_id}}", &corp_id);
                body_str = body_str.replace("{{user_id}}", &user_id);
                body_str = body_str.replace("{{today}}", &now.format("%Y-%m-%d").to_string());
                body_str = body_str.replace("{{now}}", &now.format("%Y-%m-%dT%H:%M:%S").to_string());
                if let Ok(replaced_body) = serde_json::from_str::<Value>(&body_str) {
                    req = req.json(&replaced_body);
                } else {
                    req = req.json(params);
                }
            }
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            warn!("[CONNECTOR] Request failed ({}): {}", status.as_u16(), &body[..body.len().min(500)]);
            return Err(format!("API returned error ({}): {}", status.as_u16(), body));
        }

        Ok(body)
    }

    /// Sync config from lotus API.
    pub async fn sync_config(&self) -> Result<(), String> {
        if self.api_base_url.is_empty() || self.session_key.is_empty() {
            return Err("Lotus API not configured (missing lotusApiUrl or sessionKey)".to_string());
        }

        let url = format!("{}/api/employee/saas-apps", self.api_base_url);
        let resp = HTTP_CLIENT
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.session_key))
            .send()
            .await
            .map_err(|e| format!("Failed to fetch SaaS apps: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Lotus API error ({}): {}", status, body));
        }

        let apps: Vec<SaasAppInfo> = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse SaaS apps response: {}", e))?;

        let config = ConnectorConfig {
            apps,
            last_synced: Some(chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()),
        };

        // Save to local cache
        credential_store::save_config(&self.storage, &config)
            .map_err(|e| format!("Failed to save config: {}", e))?;

        info!("[CONNECTOR] Synced config: {} apps", config.apps.len());
        *self.config.write().await = Some(config);
        Ok(())
    }

    /// Get list of apps from cached config.
    pub async fn get_apps(&self) -> Result<Vec<SaasAppInfo>, String> {
        let config = self.config.read().await;
        match config.as_ref() {
            Some(c) => Ok(c.apps.clone()),
            None => Ok(Vec::new()),
        }
    }

    /// Get OAuth URL for connecting an app.
    pub async fn get_oauth_url(&self, app_id: u64) -> Result<String, String> {
        let url = format!("{}/api/employee/saas-apps/{}/oauth-url", self.api_base_url, app_id);
        let resp = HTTP_CLIENT
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.session_key))
            .send()
            .await
            .map_err(|e| format!("Failed to get OAuth URL: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Lotus API error ({}): {}", status, body));
        }

        let body: Value = resp.json().await.map_err(|e| format!("Failed to parse response: {}", e))?;
        body["url"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "OAuth URL not found in response".to_string())
    }

    /// Check OAuth connection status for an app.
    pub async fn check_status(&self, app_id: u64) -> Result<String, String> {
        let url = format!("{}/api/employee/saas-apps/{}/status", self.api_base_url, app_id);
        let resp = HTTP_CLIENT
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.session_key))
            .send()
            .await
            .map_err(|e| format!("Failed to check status: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Lotus API error ({}): {}", status, body));
        }

        let body: Value = resp.json().await.map_err(|e| format!("Failed to parse response: {}", e))?;

        // If credential returned, save it locally
        if let Some(cred_val) = body.get("credential") {
            if let Ok(cred) = serde_json::from_value::<SaasCredential>(cred_val.clone()) {
                credential_store::save_credential(&self.storage, app_id, &cred)
                    .map_err(|e| format!("Failed to save credential: {}", e))?;
            }
        }

        let status_str = body["status"].as_str().unwrap_or("unknown");
        Ok(status_str.to_string())
    }

    /// Disconnect an app — clear local credential.
    pub async fn disconnect(&self, app_id: u64) -> Result<(), String> {
        credential_store::clear_credential(&self.storage, app_id)
            .map_err(|e| format!("Failed to clear credential: {}", e))?;
        info!("[CONNECTOR] Disconnected app {}", app_id);
        Ok(())
    }

    /// Set credential for an app (called by LLM via tool when user provides credentials in chat).
    pub async fn set_credential_by_name(
        &self,
        app_name: &str,
        api_credential: &str,
    ) -> Result<String, String> {
        let config = self.config.read().await;
        let config = config.as_ref().ok_or("SaaS connector config not loaded")?;

        let app = config.apps.iter()
            .find(|a| a.name == app_name)
            .ok_or_else(|| format!("App '{}' not found", app_name))?;

        // Save credential locally
        let cred = SaasCredential {
            access_token: None,
            api_credential: Some(api_credential.to_string()),
            token_expires_at: None,
            oauth_user_info: None,
            status: "active".to_string(),
        };
        credential_store::save_credential(&self.storage, app.id, &cred)
            .map_err(|e| format!("Failed to save credential: {}", e))?;

        // Also set it on the server side
        if !self.api_base_url.is_empty() && !self.session_key.is_empty() {
            let url = format!("{}/api/employee/saas-apps/{}/credential", self.api_base_url, app.id);
            let _ = HTTP_CLIENT
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.session_key))
                .json(&serde_json::json!({"api_credential": api_credential}))
                .send()
                .await;
        }

        info!("[CONNECTOR] Credential set for app '{}' (id={})", app_name, app.id);
        Ok(format!("凭证已保存。'{app_name}' 现在可以正常使用了。"))
    }

    /// Toggle app enabled/disabled for employee.
    pub async fn toggle_app(&self, app_id: u64, enabled: bool) -> Result<(), String> {
        // Update on server
        if !self.api_base_url.is_empty() && !self.session_key.is_empty() {
            let url = format!("{}/api/employee/saas-apps/{}/toggle", self.api_base_url, app_id);
            let resp = HTTP_CLIENT
                .put(&url)
                .header("Authorization", format!("Bearer {}", self.session_key))
                .json(&serde_json::json!({"enabled": enabled}))
                .send()
                .await
                .map_err(|e| format!("Failed to toggle app: {}", e))?;

            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Failed to toggle app: {}", body));
            }
        }

        // Update local cache
        let mut config = self.config.write().await;
        if let Some(ref mut c) = *config {
            if let Some(app) = c.apps.iter_mut().find(|a| a.id == app_id) {
                app.enabled = Some(enabled);
            }
        }
        // Save updated config
        if let Some(ref c) = *config {
            let _ = credential_store::save_config(&self.storage, c);
        }

        info!("[CONNECTOR] App {} set enabled={}", app_id, enabled);
        Ok(())
    }
}

/// Find app and capability by name within config.
fn find_app_capability<'a>(
    config: &'a ConnectorConfig,
    app_name: &str,
    capability_name: &str,
) -> Result<(&'a SaasAppInfo, &'a SaasCapabilityInfo), String> {
    let app = config
        .apps
        .iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| format!("App '{}' not found. Available apps: {}", app_name,
            config.apps.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")))?;

    let cap = app
        .capabilities
        .iter()
        .find(|c| c.name == capability_name)
        .ok_or_else(|| format!("Capability '{}' not found in app '{}'. Available: {}", capability_name, app_name,
            app.capabilities.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")))?;

    Ok((app, cap))
}

/// Extract corp_id and user_id from credential's oauth_user_info JSON.
fn extract_oauth_ids(cred: &SaasCredential) -> (String, String) {
    if let Some(ref info) = cred.oauth_user_info {
        if let Ok(v) = serde_json::from_str::<Value>(info) {
            let corp_id = v.get("corp_id")
                .or_else(|| v.get("corpId"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let user_id = v.get("user_id")
                .or_else(|| v.get("userId"))
                .or_else(|| v.get("open_id"))
                .or_else(|| v.get("openId"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            return (corp_id, user_id);
        }
    }
    (String::new(), String::new())
}
