use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::AiJiaHome;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopResourceItem {
    resource_id: String,
    version: String,
    manifest_url: String,
    #[serde(default)]
    manifest_sha256: String,
}

#[derive(Debug, Deserialize)]
struct DesktopResourceResponse {
    data: Vec<DesktopResourceItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertTeamTemplateSnapshot {
    pub team_id: String,
    pub version: String,
    #[serde(default)]
    pub facilitation_style: String,
    #[serde(default)]
    pub display_i18n: serde_json::Value,
    #[serde(default)]
    pub experts: Vec<serde_json::Value>,
    #[serde(default)]
    pub director_prompt_i18n: serde_json::Value,
}

#[tauri::command]
pub async fn expert_team_template_catalog() -> Result<Vec<ExpertTeamTemplateSnapshot>, String> {
    let cache_dir = expert_team_templates_cache_dir();
    Ok(read_cached_snapshots(&cache_dir))
}

#[tauri::command]
pub async fn expert_team_template_refresh(
    auth: tauri::State<'_, Arc<crate::auth::AuthManager>>,
) -> Result<u32, String> {
    let session_key = auth.get_session_key().await.map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();
    let url = format!(
        "{}/v1/desktop-resources?types=expert_team_template",
        crate::environment::tenant_host()
    );

    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", session_key))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error ({}): {}", status, body));
    }

    let catalog: DesktopResourceResponse = resp.json().await.map_err(|e| e.to_string())?;
    let cache_dir = expert_team_templates_cache_dir();
    let mut downloaded = 0u32;

    for item in catalog.data {
        if item.resource_id.trim().is_empty()
            || item.version.trim().is_empty()
            || item.manifest_url.trim().is_empty()
        {
            continue;
        }
        let path = cache_path_for(&cache_dir, &item.resource_id, &item.version);
        if cache_file_matches_sha256(&path, &item.manifest_sha256) {
            continue;
        }
        match download_and_cache(&client, &cache_dir, &item).await {
            Ok(()) => downloaded += 1,
            Err(e) => log::warn!(
                "[expert_team_template_refresh] {}@{}: {e}",
                item.resource_id,
                item.version
            ),
        }
    }

    Ok(downloaded)
}

fn expert_team_templates_cache_dir() -> PathBuf {
    AiJiaHome::from_home()
        .root()
        .join("expert-team-templates-cache")
}

fn cache_path_for(cache_dir: &Path, team_id: &str, version: &str) -> PathBuf {
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

fn read_cached_snapshots(cache_dir: &Path) -> Vec<ExpertTeamTemplateSnapshot> {
    let Ok(team_dirs) = std::fs::read_dir(cache_dir) else {
        return Vec::new();
    };

    let mut snapshots = Vec::new();
    for team_dir in team_dirs.flatten() {
        let Ok(file_type) = team_dir.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Ok(version_files) = std::fs::read_dir(team_dir.path()) else {
            continue;
        };
        for entry in version_files.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))
                .and_then(|content| {
                    serde_json::from_str::<ExpertTeamTemplateSnapshot>(&content)
                        .with_context(|| format!("parsing {}", path.display()))
                }) {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(e) => log::warn!("[expert_team_template_catalog] {e}"),
            }
        }
    }

    snapshots.sort_by(|a, b| {
        a.team_id
            .cmp(&b.team_id)
            .then_with(|| a.version.cmp(&b.version))
    });
    snapshots
}

async fn download_and_cache(
    client: &reqwest::Client,
    cache_dir: &Path,
    item: &DesktopResourceItem,
) -> anyhow::Result<()> {
    let resp = client
        .get(&item.manifest_url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("GET {}", item.manifest_url))?;
    if !resp.status().is_success() {
        anyhow::bail!("manifest HTTP {} from {}", resp.status(), item.manifest_url);
    }
    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("reading body from {}", item.manifest_url))?;

    if !item.manifest_sha256.is_empty() {
        let mut h = Sha256::new();
        h.update(&bytes);
        let got = hex_lower(&h.finalize());
        if !item.manifest_sha256.eq_ignore_ascii_case(&got) {
            anyhow::bail!(
                "sha256 mismatch for {}: expected {}, got {}",
                item.manifest_url,
                item.manifest_sha256,
                got
            );
        }
    }

    let _: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing expert team snapshot from {}", item.manifest_url))?;
    let path = cache_path_for(cache_dir, &item.resource_id, &item.version);
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid cache path {}", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    write_atomic(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)?;
    Ok(())
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
