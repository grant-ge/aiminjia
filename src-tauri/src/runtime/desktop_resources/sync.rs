use std::cmp::Ordering;

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::runtime::employee::template_store::TemplateSnapshot;
use crate::runtime::expert_team::store::ExpertTeamSnapshot;
use crate::storage::aijia_home::AiJiaHome;

use super::catalog::{compare_versions, resource_key, DesktopResourceIndex, DesktopResourceItem};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopResourcesResponse {
    data: Vec<DesktopResourceItem>,
}

pub fn merge_catalog_items(items: Vec<DesktopResourceItem>) -> DesktopResourceIndex {
    let mut index = DesktopResourceIndex::default();

    for item in items {
        let key = resource_key(&item);
        match index.resources.get_mut(&key) {
            Some(current)
                if compare_versions(&current.version, &item.version) == Ordering::Less =>
            {
                *current = item;
            }
            Some(_) => {}
            None => {
                index.resources.insert(key, item);
            }
        }
    }

    index
}

pub async fn sync_desktop_resources(
    client: &reqwest::Client,
    base_url: &str,
    session_key: &str,
) -> Result<DesktopResourceIndex> {
    let items = fetch_desktop_resource_catalog(client, base_url, session_key).await?;
    let home = AiJiaHome::from_home();

    for item in &items {
        let result = match item.resource_type.as_str() {
            "employee_template" => sync_employee_template(client, &home, item).await,
            "expert_team_template" => sync_expert_team_template(client, &home, item).await,
            _ => Ok(()),
        };

        if let Err(err) = result {
            log::warn!(
                "[desktop-resources] sync skipped {}:{}@{}: {}",
                item.resource_type,
                item.resource_id,
                item.version,
                err
            );
        }
    }

    Ok(merge_catalog_items(items))
}

async fn fetch_desktop_resource_catalog(
    client: &reqwest::Client,
    base_url: &str,
    session_key: &str,
) -> Result<Vec<DesktopResourceItem>> {
    let url = format!(
        "{}/v1/desktop-resources?types=employee_template,expert_team_template&lang=zh-CN",
        base_url.trim_end_matches('/')
    );
    let response = client
        .get(&url)
        .bearer_auth(session_key)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("desktop resource catalog HTTP {status}: {body}");
    }

    let envelope: DesktopResourcesResponse = response
        .json()
        .await
        .context("decoding desktop resource catalog response")?;
    Ok(envelope.data)
}

async fn sync_employee_template(
    client: &reqwest::Client,
    home: &AiJiaHome,
    item: &DesktopResourceItem,
) -> Result<()> {
    let bytes = download_manifest(client, item).await?;
    let snapshot: TemplateSnapshot =
        serde_json::from_slice(&bytes).context("parsing employee template snapshot")?;
    crate::runtime::employee::template_store::write_cache(
        &home.employee_templates_cache_dir(),
        &snapshot,
    )?;
    Ok(())
}

async fn sync_expert_team_template(
    client: &reqwest::Client,
    home: &AiJiaHome,
    item: &DesktopResourceItem,
) -> Result<()> {
    let bytes = download_manifest(client, item).await?;
    let snapshot: ExpertTeamSnapshot =
        serde_json::from_slice(&bytes).context("parsing expert team template snapshot")?;
    crate::runtime::expert_team::store::write_cache(
        &home.expert_team_templates_cache_dir(),
        &snapshot,
    )?;
    Ok(())
}

async fn download_manifest(
    client: &reqwest::Client,
    item: &DesktopResourceItem,
) -> Result<Vec<u8>> {
    let url = item.manifest_url.as_str();
    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("manifest HTTP {status} from {url}");
    }

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("reading body from {url}"))?;
    verify_sha256(&bytes, &item.manifest_sha256, url)?;
    Ok(bytes.to_vec())
}

fn verify_sha256(bytes: &[u8], expected_sha256: &str, url: &str) -> Result<()> {
    if expected_sha256.is_empty() {
        return Ok(());
    }

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let got = hex_lower(&digest);
    if !expected_sha256.eq_ignore_ascii_case(&got) {
        anyhow::bail!("sha256 mismatch for {url}: expected {expected_sha256}, got {got}");
    }

    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
