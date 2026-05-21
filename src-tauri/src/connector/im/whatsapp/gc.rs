//! 25h 附件老化清理。spec v3 §7.2。
//!
//! WhatsApp tmp 目录单平台 cron，不抽 shared（其他平台无）。

use std::path::{Path, PathBuf};
use std::time::Duration;

const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60); // 1h
const TTL: Duration = Duration::from_secs(25 * 60 * 60); // 25h

/// 启动循环：每 1h 调一次 sweep_once。manager 启动期 spawn。
/// 该函数无限循环，调用方应在 tokio::spawn 里跑。
pub async fn run_attachment_gc(dir: PathBuf) {
    loop {
        tokio::time::sleep(SWEEP_INTERVAL).await;
        if let Err(e) = sweep_once(&dir, TTL).await {
            log::warn!("[whatsapp gc] sweep_once failed: {e:#}");
        }
    }
}

/// 单次扫描：dir 下所有 entry，mtime + ttl < now 则 remove_file。
pub async fn sweep_once(dir: &Path, ttl: Duration) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let now = std::time::SystemTime::now();
    let mut read = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = read.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = match metadata.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(age) = now.duration_since(mtime) {
            if age > ttl {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    log::warn!("[whatsapp gc] remove {} failed: {e}", path.display());
                } else {
                    log::info!(
                        "[whatsapp gc] removed expired attachment {}",
                        path.display()
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sweep_skips_nonexistent_dir() {
        // 不存在的目录直接 Ok（启动期可能 tmp dir 还没建）
        let res = sweep_once(Path::new("/tmp/nonexistent_whatsapp_gc_test_xyz"), TTL).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn sweep_removes_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("old.txt");
        tokio::fs::write(&f, b"hello").await.unwrap();
        // 用 filetime crate 改 mtime 是最干净的，但仓库可能没这 dep。
        // 简化：调用 sweep_once 时把 ttl 改成 Duration::ZERO；
        // 任何文件 age > 0 都被删。
        sweep_once(dir.path(), Duration::from_millis(1))
            .await
            .unwrap();
        // sleep 一下让 age 真>1ms
        tokio::time::sleep(Duration::from_millis(10)).await;
        sweep_once(dir.path(), Duration::from_millis(1))
            .await
            .unwrap();
        assert!(!f.exists(), "old file should be deleted");
    }

    #[tokio::test]
    async fn sweep_keeps_fresh_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("fresh.txt");
        tokio::fs::write(&f, b"hi").await.unwrap();
        // ttl=24h，age 一定 < ttl
        sweep_once(dir.path(), Duration::from_secs(24 * 3600))
            .await
            .unwrap();
        assert!(f.exists(), "fresh file must NOT be deleted");
    }
}
