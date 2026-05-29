use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMeta {
    pub version: String,
    pub url: String,
    pub expected_size: u64,
    pub downloaded_size: u64,
    pub complete: bool,
    #[serde(default)]
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CacheStatus {
    Complete,
    Partial,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheCheckResult {
    pub status: CacheStatus,
    pub downloaded_size: u64,
}

pub struct UpdaterCache {
    dir: PathBuf,
}

impl UpdaterCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn meta_path(&self) -> PathBuf {
        self.dir.join("meta.json")
    }

    pub fn package_path(&self, version: &str) -> PathBuf {
        self.dir.join(format!("{}.tar.gz", version))
    }

    pub fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir).context("create updater cache dir")?;
        Ok(())
    }

    pub fn load_meta(&self) -> Option<CacheMeta> {
        let path = self.meta_path();
        let text = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save_meta(&self, meta: &CacheMeta) -> Result<()> {
        self.ensure_dir()?;
        let text = serde_json::to_string_pretty(meta)?;
        let tmp = self.meta_path().with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, self.meta_path())?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        if self.dir.exists() {
            fs::remove_dir_all(&self.dir).context("remove updater cache dir")?;
        }
        Ok(())
    }

    /// Decide cache status given the server's version, expected_size, and optional etag.
    /// Returns Partial only if metadata matches and partial file exists.
    pub fn check(&self, version: &str, expected_size: u64, etag: &str) -> Result<CacheCheckResult> {
        let Some(meta) = self.load_meta() else {
            return Ok(CacheCheckResult {
                status: CacheStatus::None,
                downloaded_size: 0,
            });
        };

        // Version mismatch or ETag mismatch (when both sides have etag) → invalidate.
        // expected_size == 0 is a sentinel "unknown" from callers that didn't HEAD
        // the URL; skip the size check in that case and trust the on-disk file.
        let etag_mismatch = !etag.is_empty() && !meta.etag.is_empty() && meta.etag != etag;
        let size_mismatch = expected_size > 0 && meta.expected_size != expected_size;
        if meta.version != version || size_mismatch || etag_mismatch {
            // Clear the orphaned package from disk. Without this, every cross-version
            // bump leaves a stale `{old}.tar.gz` behind that nothing ever cleans up
            // until the user uninstalls — over time auto-update can leak hundreds of
            // MB on long-running installs.
            let _ = self.clear();
            return Ok(CacheCheckResult {
                status: CacheStatus::None,
                downloaded_size: 0,
            });
        }

        let pkg = self.package_path(version);
        if !pkg.exists() {
            return Ok(CacheCheckResult {
                status: CacheStatus::None,
                downloaded_size: 0,
            });
        }

        // Verify on-disk size matches meta. Use meta.expected_size (the truth
        // from the prior download), not the (possibly-zero) input expected_size.
        let target = if expected_size > 0 {
            expected_size
        } else {
            meta.expected_size
        };
        let actual = fs::metadata(&pkg).map(|m| m.len()).unwrap_or(0);
        if meta.complete && actual == target {
            return Ok(CacheCheckResult {
                status: CacheStatus::Complete,
                downloaded_size: actual,
            });
        }
        // Partial: file exists, smaller than (or equal target with !complete) expected.
        // Don't require actual == meta.downloaded_size because the file may be
        // actively growing while we check — meta is only refreshed on retry
        // boundaries / completion. Any file with 0 < size < target counts as partial.
        if !meta.complete && actual > 0 && (target == 0 || actual <= target) {
            return Ok(CacheCheckResult {
                status: CacheStatus::Partial,
                downloaded_size: actual,
            });
        }

        Ok(CacheCheckResult {
            status: CacheStatus::None,
            downloaded_size: 0,
        })
    }

    /// Read the complete cached package.
    pub fn read_complete(&self, version: &str) -> Result<Vec<u8>> {
        let pkg = self.package_path(version);
        fs::read(&pkg).context("read cached update package")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_cache() -> (tempfile::TempDir, UpdaterCache) {
        let dir = tempdir().unwrap();
        let cache = UpdaterCache::new(dir.path().join("updater"));
        cache.ensure_dir().unwrap();
        (dir, cache)
    }

    #[test]
    fn check_returns_none_when_no_meta() {
        let (_d, cache) = make_cache();
        let r = cache.check("0.5.30", 100, "").unwrap();
        assert_eq!(r.status, CacheStatus::None);
    }

    #[test]
    fn check_returns_complete_when_meta_and_file_match() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.30".into(),
            url: "u".into(),
            expected_size: 5,
            downloaded_size: 5,
            complete: true,
            etag: "e1".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.30"), b"hello").unwrap();
        let r = cache.check("0.5.30", 5, "e1").unwrap();
        assert_eq!(r.status, CacheStatus::Complete);
        assert_eq!(r.downloaded_size, 5);
    }

    #[test]
    fn check_returns_partial_when_size_smaller_and_meta_partial() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.30".into(),
            url: "u".into(),
            expected_size: 10,
            downloaded_size: 5,
            complete: false,
            etag: "".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.30"), b"hello").unwrap();
        let r = cache.check("0.5.30", 10, "").unwrap();
        assert_eq!(r.status, CacheStatus::Partial);
        assert_eq!(r.downloaded_size, 5);
    }

    #[test]
    fn check_invalidates_on_version_mismatch() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.29".into(),
            url: "u".into(),
            expected_size: 5,
            downloaded_size: 5,
            complete: true,
            etag: "".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.29"), b"hello").unwrap();
        let r = cache.check("0.5.30", 5, "").unwrap();
        assert_eq!(r.status, CacheStatus::None);
        // Stale `.tar.gz` for the old version should be wiped, otherwise it
        // leaks across cross-version auto-updates.
        assert!(!cache.package_path("0.5.29").exists());
        assert!(cache.load_meta().is_none());
    }

    #[test]
    fn check_invalidates_on_etag_mismatch() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.30".into(),
            url: "u".into(),
            expected_size: 5,
            downloaded_size: 5,
            complete: true,
            etag: "old-etag".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.30"), b"hello").unwrap();
        let r = cache.check("0.5.30", 5, "new-etag").unwrap();
        assert_eq!(r.status, CacheStatus::None);
    }

    #[test]
    fn clear_removes_everything() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.30".into(),
            url: "u".into(),
            expected_size: 5,
            downloaded_size: 5,
            complete: true,
            etag: "".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.30"), b"hello").unwrap();
        cache.clear().unwrap();
        assert!(cache.load_meta().is_none());
        assert!(!cache.package_path("0.5.30").exists());
    }
}
