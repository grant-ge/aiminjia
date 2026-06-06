use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::runtime::employee::template_store::{
    cached_snapshot_matches_sha256, download_snapshot, write_cache, TemplateSnapshot,
};
use crate::storage::{fs_atomic::write_atomic, AiJiaHome};

const DEFAULT_TENANT_BASE_URL: &str = "https://ai-tenant.renlijia.com";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkplaceDirectoryDisplayText {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkplaceDirectoryCategory {
    #[serde(default)]
    pub category_id: String,
    #[serde(default)]
    pub display: WorkplaceDirectoryDisplayText,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub resource_count: i32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkplaceDirectoryRequiredSkill {
    #[serde(default)]
    pub skill_id: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub display: WorkplaceDirectoryDisplayText,
    #[serde(default)]
    pub version_range: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkplaceDirectoryItem {
    #[serde(default)]
    pub resource_type: String,
    #[serde(default)]
    pub resource_id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub workplace_category_id: String,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub display: WorkplaceDirectoryDisplayText,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub manifest_url: String,
    #[serde(default)]
    pub manifest_sha256: String,
    #[serde(default)]
    pub manifest_size: i64,
    #[serde(default)]
    pub min_desktop_version: String,
    #[serde(default)]
    pub required_skills: Vec<WorkplaceDirectoryRequiredSkill>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkplaceDirectoryResponse {
    #[serde(default)]
    pub schema_version: i32,
    #[serde(default)]
    pub categories: Vec<WorkplaceDirectoryCategory>,
    #[serde(default)]
    pub items: Vec<WorkplaceDirectoryItem>,
}

#[tauri::command]
pub async fn workplace_directory_catalog(
    lang: Option<String>,
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
) -> Result<WorkplaceDirectoryResponse, String> {
    let language = normalize_lang(lang);
    let cache_dir = workplace_directory_cache_dir();
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let session_key = match auth.get_session_key().await {
        Ok(session_key) => session_key,
        Err(e) => return cached_directory_or_error(&cache_dir, &language, e),
    };

    let directory = match fetch_workplace_directory(&client, &session_key, &language).await {
        Ok(directory) => directory,
        Err(e) => return cached_directory_or_error(&cache_dir, &language, e),
    };

    if let Err(e) = write_directory_cache(&cache_dir, &language, &directory) {
        log::warn!("[workplace_directory_catalog] cache write failed: {e}");
    }
    prewarm_resource_snapshots(&client, &directory).await;

    Ok(directory)
}

fn normalize_lang(lang: Option<String>) -> String {
    let language = lang.unwrap_or_default().trim().to_string();
    if language.is_empty() {
        "zh-CN".to_string()
    } else {
        language
    }
}

fn tenant_base_url() -> String {
    std::env::var("LOTUS_TENANT_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_TENANT_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

async fn fetch_workplace_directory(
    client: &reqwest::Client,
    session_key: &str,
    language: &str,
) -> anyhow::Result<WorkplaceDirectoryResponse> {
    let url = format!(
        "{}/v1/workplace-directory?types=employee_template,expert_team_template&lang={}",
        tenant_base_url(),
        urlencoding::encode(language)
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {session_key}"))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("API error ({status}): {body}");
    }
    resp.json::<WorkplaceDirectoryResponse>()
        .await
        .with_context(|| format!("decoding workplace directory from {url}"))
}

fn cached_directory_or_error(
    cache_dir: &Path,
    language: &str,
    err: anyhow::Error,
) -> Result<WorkplaceDirectoryResponse, String> {
    if let Some(directory) = read_directory_cache(cache_dir, language) {
        log::warn!(
            "[workplace_directory_catalog] remote load failed, using cached directory: {err}"
        );
        return Ok(directory);
    }
    Err(err.to_string())
}

async fn prewarm_resource_snapshots(
    client: &reqwest::Client,
    directory: &WorkplaceDirectoryResponse,
) {
    for item in &directory.items {
        let result = match item.resource_type.as_str() {
            "employee_template" => cache_employee_template_item(client, item).await,
            "expert_team_template" => cache_expert_team_template_item(client, item).await,
            _ => Ok(false),
        };
        if let Err(e) = result {
            log::warn!(
                "[workplace_directory_catalog] snapshot cache failed for {} {}@{}: {e}",
                item.resource_type,
                item.resource_id,
                item.version
            );
        }
    }
}

async fn cache_employee_template_item(
    client: &reqwest::Client,
    item: &WorkplaceDirectoryItem,
) -> anyhow::Result<bool> {
    if item.resource_id.trim().is_empty()
        || item.version.trim().is_empty()
        || item.manifest_url.trim().is_empty()
    {
        return Ok(false);
    }
    let cache_dir = AiJiaHome::from_home().employee_templates_cache_dir();
    if cached_snapshot_matches_sha256(
        &cache_dir,
        &item.resource_id,
        &item.version,
        &item.manifest_sha256,
    ) {
        return Ok(false);
    }
    let snapshot = download_snapshot(client, &item.manifest_url, &item.manifest_sha256).await?;
    validate_employee_snapshot(item, &snapshot)?;
    write_cache(&cache_dir, &snapshot)?;
    Ok(true)
}

fn validate_employee_snapshot(
    item: &WorkplaceDirectoryItem,
    snapshot: &TemplateSnapshot,
) -> anyhow::Result<()> {
    if snapshot.template_id != item.resource_id || snapshot.version != item.version {
        anyhow::bail!(
            "employee snapshot identity mismatch: directory {}@{}, snapshot {}@{}",
            item.resource_id,
            item.version,
            snapshot.template_id,
            snapshot.version
        );
    }
    Ok(())
}

async fn cache_expert_team_template_item(
    client: &reqwest::Client,
    item: &WorkplaceDirectoryItem,
) -> anyhow::Result<bool> {
    if item.resource_id.trim().is_empty()
        || item.version.trim().is_empty()
        || item.manifest_url.trim().is_empty()
    {
        return Ok(false);
    }
    let cache_dir = AiJiaHome::from_home()
        .root()
        .join("expert-team-templates-cache");
    let path = expert_cache_path_for(&cache_dir, &item.resource_id, &item.version);
    if cache_file_matches_sha256(&path, &item.manifest_sha256) {
        return Ok(false);
    }
    let bytes = download_bytes(client, &item.manifest_url, &item.manifest_sha256).await?;
    let _: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing expert team snapshot from {}", item.manifest_url))?;
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid cache path {}", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    write_atomic(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

fn workplace_directory_cache_dir() -> PathBuf {
    AiJiaHome::from_home()
        .root()
        .join("workplace-directory-cache")
}

fn directory_cache_path_for(cache_dir: &Path, language: &str) -> PathBuf {
    cache_dir.join(format!("{}.json", urlencoding::encode(language)))
}

fn expert_cache_path_for(cache_dir: &Path, team_id: &str, version: &str) -> PathBuf {
    cache_dir.join(team_id).join(format!("{version}.json"))
}

fn cache_file_matches_sha256(path: &Path, expected_sha256: &str) -> bool {
    if !path.exists() {
        return false;
    }
    if expected_sha256.trim().is_empty() {
        return true;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let mut h = Sha256::new();
    h.update(&bytes);
    let got = hex_lower(&h.finalize());
    expected_sha256.eq_ignore_ascii_case(&got)
}

fn read_directory_cache(cache_dir: &Path, language: &str) -> Option<WorkplaceDirectoryResponse> {
    let path = directory_cache_path_for(cache_dir, language);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_directory_cache(
    cache_dir: &Path,
    language: &str,
    directory: &WorkplaceDirectoryResponse,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating {}", cache_dir.display()))?;
    let path = directory_cache_path_for(cache_dir, language);
    let json = serde_json::to_vec_pretty(directory)?;
    write_atomic(&path, &json).with_context(|| format!("writing {}", path.display()))
}

async fn download_bytes(
    client: &reqwest::Client,
    url: &str,
    expected_sha256: &str,
) -> anyhow::Result<Vec<u8>> {
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("manifest HTTP {} from {url}", resp.status());
    }
    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("reading body from {url}"))?;
    if !expected_sha256.is_empty() {
        let mut h = Sha256::new();
        h.update(&bytes);
        let got = hex_lower(&h.finalize());
        if !expected_sha256.eq_ignore_ascii_case(&got) {
            anyhow::bail!("sha256 mismatch for {url}: expected {expected_sha256}, got {got}");
        }
    }
    Ok(bytes.to_vec())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
