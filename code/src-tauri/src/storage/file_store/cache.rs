//! Search cache — per-query JSON files with TTL.
//!
//! Each cache entry is stored as `cache/{query_hash}.json` with an expiration
//! timestamp. Expired entries are cleaned up on demand.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use super::error::StorageResult;
use super::io::{atomic_write_json, read_json_optional};
use super::types::CacheEntry;

fn cache_dir(base_dir: &Path) -> PathBuf {
    base_dir.join("shared").join("cache")
}

fn cache_file_path(base_dir: &Path, query_hash: &str) -> PathBuf {
    cache_dir(base_dir).join(format!("{}.json", query_hash))
}

/// Upsert a search cache entry.
pub fn upsert_search_cache(
    base_dir: &Path,
    query_hash: &str,
    query: &str,
    results: &str,
    expires_at: &str,
) -> StorageResult<()> {
    let dir = cache_dir(base_dir);
    fs::create_dir_all(&dir)?;

    let entry = CacheEntry {
        query_hash: query_hash.to_string(),
        query: query.to_string(),
        results: results.to_string(),
        expires_at: expires_at.to_string(),
    };

    atomic_write_json(&cache_file_path(base_dir, query_hash), &entry)?;
    Ok(())
}

/// Get a search cache entry if it exists and hasn't expired.
pub fn get_search_cache(
    base_dir: &Path,
    query_hash: &str,
) -> StorageResult<Option<CacheEntry>> {
    let entry: Option<CacheEntry> =
        read_json_optional(&cache_file_path(base_dir, query_hash))?;

    match entry {
        Some(e) => {
            let now = Utc::now().to_rfc3339();
            if e.expires_at.as_str() < now.as_str() {
                // Expired — remove and return None
                let _ = fs::remove_file(cache_file_path(base_dir, query_hash));
                Ok(None)
            } else {
                Ok(Some(e))
            }
        }
        None => Ok(None),
    }
}

/// Clean up all expired cache entries.
pub fn cleanup_expired_cache(base_dir: &Path) -> StorageResult<usize> {
    let dir = cache_dir(base_dir);
    if !dir.exists() {
        return Ok(0);
    }

    let now = Utc::now().to_rfc3339();
    let mut cleaned = 0;

    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        if let Ok(Some(cache_entry)) = read_json_optional::<CacheEntry>(&path) {
            if cache_entry.expires_at.as_str() < now.as_str() {
                let _ = fs::remove_file(&path);
                cleaned += 1;
            }
        } else {
            // Corrupted cache file — remove
            let _ = fs::remove_file(&path);
            cleaned += 1;
        }
    }

    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (PathBuf, TempDir) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        (base, dir)
    }

    #[test]
    fn test_cache_crud() {
        let (base, _dir) = setup();

        // Future expiry
        let expires = "2099-12-31T23:59:59Z";
        upsert_search_cache(&base, "abc123", "test query", r#"{"results":[]}"#, expires)
            .unwrap();

        let entry = get_search_cache(&base, "abc123").unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.as_ref().unwrap().query, "test query");
    }

    #[test]
    fn test_cache_expired() {
        let (base, _dir) = setup();

        // Past expiry
        let expires = "2020-01-01T00:00:00Z";
        upsert_search_cache(&base, "abc123", "old query", r#"{"results":[]}"#, expires)
            .unwrap();

        let entry = get_search_cache(&base, "abc123").unwrap();
        assert!(entry.is_none()); // Should be expired
    }

    #[test]
    fn test_cleanup_expired() {
        let (base, _dir) = setup();

        // One valid, one expired
        upsert_search_cache(
            &base,
            "valid",
            "valid query",
            "{}",
            "2099-12-31T23:59:59Z",
        )
        .unwrap();
        upsert_search_cache(
            &base,
            "expired",
            "old query",
            "{}",
            "2020-01-01T00:00:00Z",
        )
        .unwrap();

        let cleaned = cleanup_expired_cache(&base).unwrap();
        assert_eq!(cleaned, 1);

        // Valid should still exist
        assert!(get_search_cache(&base, "valid").unwrap().is_some());
    }
}
