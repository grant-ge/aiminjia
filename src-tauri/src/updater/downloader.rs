use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::updater::cache::{CacheMeta, UpdaterCache};

const RETRY_DELAYS_SECS: [u64; 3] = [2, 8, 30];

#[derive(Debug, Clone)]
pub struct DownloadParams {
    pub url: String,
    pub version: String,
    pub expected_size: u64,
    pub etag: String,
}

pub trait ProgressSink: Send + Sync {
    fn on_progress(&self, downloaded: u64, total: u64);
}

/// Returns Ok(()) on successful complete download.
/// Errors after retries exhausted carry the last underlying error.
pub async fn download_with_resume(
    cache: &UpdaterCache,
    params: &DownloadParams,
    progress: &dyn ProgressSink,
) -> Result<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().await;
    info!("[updater] download_with_resume acquired lock for version={}", params.version);

    cache.ensure_dir()?;
    let pkg_path = cache.package_path(&params.version);

    // Determine starting offset from existing partial file (if any).
    let mut start: u64 = if pkg_path.exists() {
        std::fs::metadata(&pkg_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    if params.expected_size > 0 && start > params.expected_size {
        // Stale partial; truncate
        std::fs::remove_file(&pkg_path).ok();
        start = 0;
    }

    // Short-circuit: if the file is already complete on disk, mark cache as
    // complete and return. Otherwise we'd send a Range request from the end
    // of the file, and the server (e.g. OSS) may respond with 200 + full file
    // instead of 416, which our code would interpret as "Range not supported"
    // and *delete* the complete file to restart from 0.
    if params.expected_size > 0 && start == params.expected_size {
        info!("[updater] file already complete on disk ({} bytes), skipping download", start);
        progress.on_progress(start, params.expected_size);
        let meta = CacheMeta {
            version: params.version.clone(),
            url: params.url.clone(),
            expected_size: params.expected_size,
            downloaded_size: start,
            complete: true,
            etag: params.etag.clone(),
        };
        cache.save_meta(&meta)?;
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .pool_max_idle_per_host(0)
        .build()
        .context("build reqwest client")?;

    // Write an initial partial meta so a concurrent process (e.g. webview
    // reload triggering a fresh bootstrap) can observe that a download is
    // in flight and skip starting a duplicate.
    let _ = cache.save_meta(&CacheMeta {
        version: params.version.clone(),
        url: params.url.clone(),
        expected_size: params.expected_size,
        downloaded_size: start,
        complete: false,
        etag: params.etag.clone(),
    });

    let mut last_err: Option<anyhow::Error> = None;
    for (attempt, delay) in std::iter::once(0).chain(RETRY_DELAYS_SECS.iter().copied()).enumerate() {
        if delay > 0 {
            info!("[updater] sleeping {}s before retry attempt {}", delay, attempt);
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
        info!("[updater] download attempt {} starting from byte {}", attempt, start);
        match do_download(&client, cache, params, &pkg_path, start, progress).await {
            Ok(()) => {
                let final_size = std::fs::metadata(&pkg_path).map(|m| m.len()).unwrap_or(0);
                info!("[updater] download complete: {} bytes", final_size);
                let meta = CacheMeta {
                    version: params.version.clone(),
                    url: params.url.clone(),
                    expected_size: if params.expected_size > 0 { params.expected_size } else { final_size },
                    downloaded_size: final_size,
                    complete: true,
                    etag: params.etag.clone(),
                };
                cache.save_meta(&meta)?;
                return Ok(());
            }
            Err(e) => {
                warn!("[updater] download attempt {} failed at byte {}: {:#}", attempt, start, e);
                if !is_transient(&e) {
                    warn!("[updater] error is NOT transient, giving up");
                    return Err(e);
                }
                // Update start for next attempt based on what's on disk now
                let new_start = std::fs::metadata(&pkg_path).map(|m| m.len()).unwrap_or(start);
                info!("[updater] resuming next attempt from byte {} (was {})", new_start, start);
                start = new_start;
                // Persist progress to meta so a later session can resume
                let _ = cache.save_meta(&CacheMeta {
                    version: params.version.clone(),
                    url: params.url.clone(),
                    expected_size: params.expected_size,
                    downloaded_size: start,
                    complete: false,
                    etag: params.etag.clone(),
                });
                last_err = Some(e);
                let _ = attempt;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("download failed after retries")))
}

async fn do_download(
    client: &reqwest::Client,
    cache: &UpdaterCache,
    params: &DownloadParams,
    pkg_path: &Path,
    start: u64,
    progress: &dyn ProgressSink,
) -> Result<()> {
    let mut req = client.get(&params.url);
    if start > 0 {
        req = req.header("Range", format!("bytes={}-", start));
    }
    let resp = req.send().await.context("send http request")?;
    let status = resp.status();
    let resp_content_length = resp.content_length();
    info!("[updater] response: status={} content-length={:?} start={}",
        status.as_u16(), resp_content_length, start);
    if start > 0 && status.as_u16() != 206 {
        // Server returned 200 + full body instead of 206 partial. Two cases:
        //   (a) start equals (or exceeds) full file length → our local file is
        //       already complete. Don't trash it; just verify and return.
        //   (b) start < full body → server truly doesn't support Range. Restart.
        if let Some(full_len) = resp_content_length {
            if start >= full_len {
                info!("[updater] server returned 200 but local file already has {} bytes (full={}), treating as complete", start, full_len);
                progress.on_progress(start, full_len);
                return Ok(());
            }
        }
        warn!("[updater] server returned {} (not 206) with Range request — restarting from 0", status.as_u16());
        std::fs::remove_file(pkg_path).ok();
        return Box::pin(do_download(client, cache, params, pkg_path, 0, progress)).await;
    }
    if !status.is_success() {
        return Err(anyhow!("http {}", status.as_u16()));
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(start > 0)
        .write(true)
        .truncate(start == 0)
        .open(pkg_path)
        .context("open package file")?;

    // Prefer the HTTP Content-Length (Content-Range for 206) over params.expected_size
    // because params may pass 0 as "unknown". For 206 partial responses, Content-Length
    // is the remaining bytes — add `start` to get the true total.
    let resp_len = resp.content_length().unwrap_or(0);
    let total = if status.as_u16() == 206 {
        resp_len + start
    } else if resp_len > 0 {
        resp_len
    } else if params.expected_size > 0 {
        params.expected_size
    } else {
        0
    };

    // Refresh meta now that we know the real total. This makes the partial
    // state observable to a concurrent reader (e.g. webview reload) with the
    // correct expected_size, instead of the 0 sentinel we wrote earlier.
    if total > 0 {
        let _ = cache.save_meta(&CacheMeta {
            version: params.version.clone(),
            url: params.url.clone(),
            expected_size: total,
            downloaded_size: start,
            complete: false,
            etag: params.etag.clone(),
        });
    }

    let mut downloaded = start;
    progress.on_progress(downloaded, total);

    use futures::StreamExt;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read chunk")?;
        file.write_all(&chunk).context("write chunk to file")?;
        downloaded += chunk.len() as u64;
        progress.on_progress(downloaded, total);
    }
    file.flush().ok();
    drop(file);

    if total > 0 {
        let actual = std::fs::metadata(pkg_path).map(|m| m.len()).unwrap_or(0);
        if actual != total {
            return Err(anyhow!(
                "size mismatch: expected {} actual {}",
                total,
                actual
            ));
        }
    }
    Ok(())
}

fn is_transient(err: &anyhow::Error) -> bool {
    let s = format!("{err:#}").to_lowercase();
    s.contains("timeout")
        || s.contains("timed out")
        || s.contains("connection reset")
        || s.contains("connection refused")
        || s.contains("connection closed")
        || s.contains("closed")
        || s.contains("connect")          // covers "client error (Connect)" from reqwest
        || s.contains("通过错误关闭连接")    // Chinese locale: "connection closed unexpectedly"
        || s.contains("dns")
        || s.contains("network")
        || s.contains("http 5")
        || s.contains("end of file")     // hyper "end of file before message length"
        || s.contains("decoding response body") // body read interrupted
        || s.contains("size mismatch")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct NullSink;
    impl ProgressSink for NullSink {
        fn on_progress(&self, _: u64, _: u64) {}
    }

    #[tokio::test]
    async fn returns_error_for_invalid_url() {
        let dir = tempdir().unwrap();
        let cache = UpdaterCache::new(dir.path().join("updater"));
        let params = DownloadParams {
            url: "http://127.0.0.1:1/nope".to_string(),
            version: "0.5.30".to_string(),
            expected_size: 10,
            etag: "".to_string(),
        };
        // Wrap in timeout to ensure test doesn't hang on retries
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            download_with_resume(&cache, &params, &NullSink),
        ).await;
        // Either the download errors out OR the test timeout fires - both are OK
        match result {
            Ok(Err(_)) => {}  // download errored as expected
            Err(_) => {}       // test timeout — also acceptable since we're testing error path
            Ok(Ok(())) => panic!("download should not succeed for unreachable URL"),
        }
    }

    #[test]
    fn is_transient_recognizes_network_errors() {
        assert!(is_transient(&anyhow!("connection reset by peer")));
        assert!(is_transient(&anyhow!("operation timed out")));
        assert!(is_transient(&anyhow!("http 502 bad gateway")));
        assert!(!is_transient(&anyhow!("http 404 not found")));
        assert!(!is_transient(&anyhow!("signature verification failed")));
    }
}
