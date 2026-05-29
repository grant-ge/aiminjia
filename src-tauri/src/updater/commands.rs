use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;

use crate::storage::aijia_home::AiJiaHome;
use crate::updater::cache::{CacheCheckResult, UpdaterCache};
use crate::updater::downloader::{download_with_resume, DownloadParams, ProgressSink};
use crate::updater::sanitize::strip_macos_metadata;

fn cache_for(home: &AiJiaHome) -> UpdaterCache {
    UpdaterCache::new(home.global_updater_dir())
}

#[tauri::command]
pub async fn updater_check_cache(
    home: State<'_, Arc<AiJiaHome>>,
    version: String,
    expected_size: u64,
    etag: Option<String>,
) -> Result<CacheCheckResult, String> {
    let cache = cache_for(&home);
    let etag = etag.unwrap_or_default();
    cache
        .check(&version, expected_size, &etag)
        .map_err(|e| e.to_string())
}

struct EmitSink {
    app: AppHandle,
    version: String,
}

impl ProgressSink for EmitSink {
    fn on_progress(&self, downloaded: u64, total: u64) {
        let _ = self.app.emit(
            "updater:download-progress",
            serde_json::json!({
                "version": self.version,
                "downloaded": downloaded,
                "total": total,
            }),
        );
    }
}

#[tauri::command]
pub async fn updater_download(
    app: AppHandle,
    home: State<'_, Arc<AiJiaHome>>,
    url: String,
    version: String,
    expected_size: u64,
    etag: Option<String>,
) -> Result<(), String> {
    let cache = cache_for(&home);
    let params = DownloadParams {
        url,
        version: version.clone(),
        expected_size,
        etag: etag.unwrap_or_default(),
    };
    let sink = EmitSink {
        app: app.clone(),
        version: version.clone(),
    };
    match download_with_resume(&cache, &params, &sink).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = format!("{e:#}");
            let _ = app.emit(
                "updater:download-failed",
                serde_json::json!({
                    "version": version,
                    "error": msg,
                }),
            );
            Err(msg)
        }
    }
}

#[tauri::command]
pub async fn updater_read_cached_bytes(
    home: State<'_, Arc<AiJiaHome>>,
    version: String,
) -> Result<Vec<u8>, String> {
    let cache = cache_for(&home);
    cache.read_complete(&version).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn updater_clear_cache(home: State<'_, Arc<AiJiaHome>>) -> Result<(), String> {
    let cache = cache_for(&home);
    cache.clear().map_err(|e| e.to_string())
}

/// Install the cached update package via the Tauri updater plugin's
/// Rust-side `Update::install(bytes)` API. The frontend JS plugin only
/// exposes a no-arg `install()` that requires a prior `download()` call,
/// so we expose this command to bypass that limitation and install from
/// bytes we already have on disk.
#[tauri::command]
pub async fn updater_install_cached(
    app: AppHandle,
    home: State<'_, Arc<AiJiaHome>>,
    version: String,
) -> Result<(), String> {
    log::info!("[updater_install_cached] start, version={}", version);
    let cache = cache_for(&home);
    let bytes = cache.read_complete(&version).map_err(|e| {
        log::error!("[updater_install_cached] read_complete failed: {}", e);
        e.to_string()
    })?;
    log::info!(
        "[updater_install_cached] loaded {} bytes from cache",
        bytes.len()
    );

    // Strip macOS AppleDouble (`._*`) and `.DS_Store` entries that the Tauri
    // bundler embeds into the tarball. The Rust tar crate that
    // tauri-plugin-updater uses for installation doesn't understand AppleDouble
    // and fails with "failed to unpack `._AIjia.app`" otherwise.
    #[cfg(target_os = "macos")]
    let bytes = match strip_macos_metadata(&bytes) {
        Ok(clean) => {
            log::info!(
                "[updater_install_cached] sanitized: {} bytes → {} bytes",
                bytes.len(),
                clean.len()
            );
            clean
        }
        Err(e) => {
            log::warn!(
                "[updater_install_cached] sanitize failed, falling back to raw bytes: {:#}",
                e
            );
            bytes
        }
    };

    let updater = app.updater().map_err(|e| {
        log::error!("[updater_install_cached] app.updater() failed: {}", e);
        e.to_string()
    })?;
    log::info!("[updater_install_cached] calling updater.check()");
    let update = updater.check().await.map_err(|e| {
        log::error!("[updater_install_cached] check() failed: {}", e);
        e.to_string()
    })?;
    let Some(update) = update else {
        log::warn!("[updater_install_cached] no update available from server");
        return Err("No update available from server (it may have been withdrawn)".to_string());
    };
    log::info!("[updater_install_cached] update.version={}", update.version);
    if update.version != version {
        return Err(format!(
            "Version mismatch: cached package is {} but server now reports {}",
            version, update.version
        ));
    }

    log::info!(
        "[updater_install_cached] calling update.install() with {} bytes",
        bytes.len()
    );
    update.install(bytes).map_err(|e| {
        log::error!("[updater_install_cached] install() failed: {:#}", e);
        format!("{:#}", e)
    })?;
    log::info!("[updater_install_cached] install() returned successfully");
    Ok(())
}

/// Returns the `{os}-{arch}` key used in update.json's `platforms` map for the
/// build that's currently running. Driven by `cfg!` (compile-time constants),
/// so Intel mac builds reliably return `darwin-x86_64` even when the webview's
/// `navigator.userAgent` lies about arch.
pub fn current_platform_key() -> &'static str {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };
    platform_key_for(os, arch)
}

/// Pure helper: maps (os, arch) → manifest platform key. Extracted so we can
/// unit-test all (os, arch) combinations on any host (`current_platform_key`
/// alone is unfalsifiable on the host that matches the buggy default).
fn platform_key_for(os: &str, arch: &str) -> &'static str {
    match (os, arch) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => "unknown",
    }
}

#[tauri::command]
pub fn updater_platform_key() -> &'static str {
    current_platform_key()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_mac_must_get_x86_64_key() {
        assert_eq!(platform_key_for("macos", "x86_64"), "darwin-x86_64");
    }

    #[test]
    fn apple_silicon_mac_gets_aarch64_key() {
        assert_eq!(platform_key_for("macos", "aarch64"), "darwin-aarch64");
    }

    #[test]
    fn windows_x64_gets_windows_x86_64_key() {
        assert_eq!(platform_key_for("windows", "x86_64"), "windows-x86_64");
    }

    #[test]
    fn linux_x64_gets_linux_x86_64_key() {
        assert_eq!(platform_key_for("linux", "x86_64"), "linux-x86_64");
    }

    #[test]
    fn unknown_combination_does_not_collapse_to_aarch64() {
        let key = platform_key_for("plan9", "riscv64");
        assert!(
            !key.ends_with("-aarch64"),
            "unknown OS/arch must not silently fall back to aarch64 (got {key})"
        );
    }

    #[test]
    fn current_platform_key_matches_compile_target() {
        let key = current_platform_key();
        let (os, arch) = key.split_once('-').expect("key has os-arch shape");
        if cfg!(target_os = "macos") {
            assert_eq!(os, "darwin");
        } else if cfg!(target_os = "windows") {
            assert_eq!(os, "windows");
        } else if cfg!(target_os = "linux") {
            assert_eq!(os, "linux");
        }
        if cfg!(target_arch = "x86_64") {
            assert_eq!(arch, "x86_64");
        } else if cfg!(target_arch = "aarch64") {
            assert_eq!(arch, "aarch64");
        }
    }
}
