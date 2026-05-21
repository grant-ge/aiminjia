//! Feishu inbound attachment downloader. Streams raw bytes from
//! `GET /open-apis/im/v1/messages/{message_id}/resources/{file_key}?type={image|file}`
//! to `~/.renlijia/tmp/feishu_downloads/` with sha256-keyed filenames for dedup.
//!
//! Structurally mirrors `super::super::dingtalk::download` (DingtalkFileDownloader):
//! - One-step download (no separate "get download URL" call like dingtalk needs).
//! - Per-attempt retry x3 with 500ms backoff, sha256 dedup, atomic rename.
//! - `FeishuDownloadedFile` mirrors dingtalk's `DownloadedFile`; a future cleanup
//!   (Phase 2) may move both into `im/shared/download.rs` once a 3rd platform
//!   exercises the same shape.
//!
//! Auth: `Authorization: Bearer <tenant_access_token>` via shared
//! `TokenCache<FeishuTokenSource>`. The cache itself is owned by
//! `FeishuConnector` and handed to this downloader at construction time so the
//! same token is reused across `send()` and `download()` paths (one OnceCell
//! in the connector keeps cache construction lazy + singleton).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::connector::im::shared::token::TokenCache as SharedTokenCache;
use crate::connector::im::types::AttachmentKind;

use super::token::FeishuTokenSource;

const FEISHU_API: &str = "https://open.feishu.cn";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// Same shape as `super::super::dingtalk::download::DownloadedFile`. Kept
/// parallel for PR6 scope; can be unified into a shared module later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeishuDownloadedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum FeishuDownloadError {
    #[error("token: {0:#}")]
    Token(anyhow::Error),
    #[error("fetch resource: status={status} body={body}")]
    Fetch { status: u16, body: String },
    #[error("network: {0}")]
    Network(reqwest::Error),
    #[error("io: {0}")]
    Io(std::io::Error),
}

#[derive(Clone)]
pub struct FeishuFileDownloader {
    client: Client,
    token_cache: Arc<SharedTokenCache<FeishuTokenSource>>,
    dest_dir: PathBuf,
    api_base: String,
}

impl FeishuFileDownloader {
    pub fn new(token_cache: Arc<SharedTokenCache<FeishuTokenSource>>, dest_dir: PathBuf) -> Self {
        Self {
            client: Client::builder()
                .timeout(DOWNLOAD_TIMEOUT)
                .build()
                .expect("build reqwest client"),
            token_cache,
            dest_dir,
            api_base: FEISHU_API.to_string(),
        }
    }

    /// Test-only constructor that points the resource endpoint at a mock
    /// server. Mirrors `FeishuTokenSource::new_with_api_base_for_tests` so
    /// callers can satisfy both the token POST and the resource GET against
    /// the same wiremock server. Hidden from production callers.
    #[doc(hidden)]
    pub fn new_with_api_base_for_tests(
        token_cache: Arc<SharedTokenCache<FeishuTokenSource>>,
        dest_dir: PathBuf,
        api_base: String,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(DOWNLOAD_TIMEOUT)
                .build()
                .expect("build reqwest client"),
            token_cache,
            dest_dir,
            api_base,
        }
    }

    /// Download a single resource. `file_key` is the message-content's
    /// `image_key` / `file_key`; `kind` decides the `?type=image|file` query
    /// param required by the Feishu OAPI.
    pub async fn download(
        &self,
        message_id: &str,
        file_key: &str,
        kind: AttachmentKind,
        original_file_name: &str,
    ) -> Result<FeishuDownloadedFile, FeishuDownloadError> {
        let display_name = safe_display_file_name(original_file_name);
        tokio::fs::create_dir_all(&self.dest_dir)
            .await
            .map_err(FeishuDownloadError::Io)?;
        let token = self
            .token_cache
            .get()
            .await
            .map_err(FeishuDownloadError::Token)?;
        self.fetch_with_retries(message_id, file_key, kind, &token, &display_name)
            .await
    }

    async fn fetch_with_retries(
        &self,
        message_id: &str,
        file_key: &str,
        kind: AttachmentKind,
        token: &str,
        display_name: &str,
    ) -> Result<FeishuDownloadedFile, FeishuDownloadError> {
        const MAX_ATTEMPTS: u32 = 3;
        let mut last_error: Option<FeishuDownloadError> = None;
        for attempt in 0..MAX_ATTEMPTS {
            match self
                .fetch_once(message_id, file_key, kind, token, display_name)
                .await
            {
                Ok(file) => return Ok(file),
                Err(error) => {
                    let retryable = is_retryable_error(&error);
                    let last_attempt = attempt + 1 >= MAX_ATTEMPTS;
                    if !retryable || last_attempt {
                        return Err(error);
                    }
                    log::warn!(
                        "[feishu/download] attempt {}/{} retryable error msg_id={} file_key={} err={:#}",
                        attempt + 1,
                        MAX_ATTEMPTS,
                        message_id,
                        file_key,
                        error
                    );
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        Err(last_error.expect("download attempted at least once"))
    }

    async fn fetch_once(
        &self,
        message_id: &str,
        file_key: &str,
        kind: AttachmentKind,
        token: &str,
        display_name: &str,
    ) -> Result<FeishuDownloadedFile, FeishuDownloadError> {
        let type_param = match kind {
            AttachmentKind::Picture => "image",
            AttachmentKind::File => "file",
        };
        let url = format!(
            "{}/open-apis/im/v1/messages/{}/resources/{}",
            self.api_base, message_id, file_key
        );
        let resp = self
            .client
            .get(&url)
            .query(&[("type", type_param)])
            .timeout(DOWNLOAD_TIMEOUT)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(FeishuDownloadError::Network)?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(FeishuDownloadError::Fetch { status, body });
        }
        let mime_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());
        let bytes = resp.bytes().await.map_err(FeishuDownloadError::Network)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = format!("{:x}", hasher.finalize());
        let ext = extension_or_bin(display_name);
        let final_path = self.dest_dir.join(format!("{}.{}", sha256, ext));
        if final_path.exists() {
            return Ok(FeishuDownloadedFile {
                path: final_path,
                file_name: display_name.to_string(),
                size: bytes.len() as u64,
                sha256,
                mime_type,
            });
        }
        let tmp_path = self.dest_dir.join(format!(".tmp_{}", uuid::Uuid::new_v4()));
        let write_result = async {
            let mut file = tokio::fs::File::create(&tmp_path)
                .await
                .map_err(FeishuDownloadError::Io)?;
            file.write_all(&bytes)
                .await
                .map_err(FeishuDownloadError::Io)?;
            file.flush().await.map_err(FeishuDownloadError::Io)?;
            Ok::<(), FeishuDownloadError>(())
        }
        .await;
        if let Err(error) = write_result {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(error);
        }
        if let Err(error) = tokio::fs::rename(&tmp_path, &final_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(FeishuDownloadError::Io(error));
        }
        Ok(FeishuDownloadedFile {
            path: final_path,
            file_name: display_name.to_string(),
            size: bytes.len() as u64,
            sha256,
            mime_type,
        })
    }
}

pub fn safe_display_file_name(original_file_name: &str) -> String {
    let candidate = Path::new(original_file_name)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("attachment.bin")
        .trim();
    let candidate = if candidate.is_empty() {
        "attachment.bin"
    } else {
        candidate
    };
    if crate::storage::safe_filename::ensure_safe_filename(candidate).is_ok() {
        candidate.to_string()
    } else {
        "attachment.bin".to_string()
    }
}

pub fn extension_or_bin(file_name: &str) -> String {
    Path::new(file_name)
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "bin".to_string())
}

/// Decide whether a download error should trigger another attempt.
///
/// - `Network(_)`: transport-level (DNS, TCP reset, request timeout) — always
///   retry. reqwest itself doesn't retry; the cost is one round-trip.
/// - `Fetch { status, .. }`: retry only 5xx + 408 (request timeout) + 425 (too
///   early) + 429 (rate limit). 4xx (404 missing key, 401/403 expired token /
///   bad permissions, 400 malformed url) is non-retryable per HTTP semantics —
///   burning 3 * 500ms backoff serially per bad attachment is pure waste.
/// - `Token(_)` / `Io(_)`: never produced inside `fetch_with_retries`
///   (`Token` is acquired up-front by `download()`; `Io` only fires on the
///   atomic-rename / mkdir path) — listed for exhaustiveness.
fn is_retryable_error(err: &FeishuDownloadError) -> bool {
    match err {
        FeishuDownloadError::Network(_) => true,
        FeishuDownloadError::Fetch { status, .. } => {
            *status >= 500
                || *status == 408 // Request Timeout
                || *status == 425 // Too Early
                || *status == 429 // Too Many Requests
        }
        FeishuDownloadError::Token(_) | FeishuDownloadError::Io(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use wiremock::matchers::{header, method, path as wm_path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Set up wiremock to satisfy the FeishuTokenSource token POST endpoint.
    /// Returns the server so callers can mount additional mocks for the
    /// resource GET path.
    async fn mock_server_with_token(token: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(wm_path("/open-apis/auth/v3/tenant_access_token/internal"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": token,
                "expire": 7200
            })))
            .mount(&server)
            .await;
        server
    }

    fn make_cache_against(server: &MockServer) -> Arc<SharedTokenCache<FeishuTokenSource>> {
        let source = Arc::new(FeishuTokenSource::new_with_api_base_for_tests(
            "test-app-id".into(),
            "test-app-secret".into(),
            server.uri(),
        ));
        Arc::new(SharedTokenCache::new(source))
    }

    #[test]
    fn safe_name_rejects_path_traversal() {
        assert_eq!(safe_display_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_display_file_name("CON"), "attachment.bin");
    }

    #[test]
    fn extension_defaults_to_bin() {
        assert_eq!(extension_or_bin("report.xlsx"), "xlsx");
        assert_eq!(extension_or_bin("README"), "bin");
    }

    #[tokio::test]
    async fn download_image_happy_path() {
        let server = mock_server_with_token("tok-1").await;
        Mock::given(method("GET"))
            .and(wm_path(
                "/open-apis/im/v1/messages/om_msg_1/resources/img_v2_001",
            ))
            .and(query_param("type", "image"))
            .and(header("Authorization", "Bearer tok-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(b"PNGDATA" as &[u8]),
            )
            .mount(&server)
            .await;

        let dir = tempfile::TempDir::new().unwrap();
        let cache = make_cache_against(&server);
        let downloader = FeishuFileDownloader::new_with_api_base_for_tests(
            cache,
            dir.path().to_path_buf(),
            server.uri(),
        );

        let file = downloader
            .download("om_msg_1", "img_v2_001", AttachmentKind::Picture, "pic.png")
            .await
            .expect("download succeeds");

        assert_eq!(file.file_name, "pic.png");
        assert_eq!(file.size, 7);
        assert_eq!(file.mime_type.as_deref(), Some("image/png"));
        assert_eq!(file.path.extension().and_then(|v| v.to_str()), Some("png"));
        assert_eq!(std::fs::read(&file.path).unwrap(), b"PNGDATA");
    }

    #[tokio::test]
    async fn download_file_uses_type_file_query() {
        let server = mock_server_with_token("tok-1").await;
        Mock::given(method("GET"))
            .and(wm_path(
                "/open-apis/im/v1/messages/om_msg_2/resources/file_v2_xyz",
            ))
            .and(query_param("type", "file"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/pdf")
                    .set_body_bytes(b"%PDF-1.4 ..." as &[u8]),
            )
            .mount(&server)
            .await;

        let dir = tempfile::TempDir::new().unwrap();
        let cache = make_cache_against(&server);
        let downloader = FeishuFileDownloader::new_with_api_base_for_tests(
            cache,
            dir.path().to_path_buf(),
            server.uri(),
        );

        let file = downloader
            .download(
                "om_msg_2",
                "file_v2_xyz",
                AttachmentKind::File,
                "report.pdf",
            )
            .await
            .expect("file download succeeds");

        assert_eq!(file.mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(file.path.extension().and_then(|v| v.to_str()), Some("pdf"));
    }

    #[tokio::test]
    async fn download_dedup_when_same_content() {
        let server = mock_server_with_token("tok-1").await;
        Mock::given(method("GET"))
            .and(wm_path(
                "/open-apis/im/v1/messages/om_msg_3/resources/img_v2_002",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"same" as &[u8]))
            .mount(&server)
            .await;

        let dir = tempfile::TempDir::new().unwrap();
        let cache = make_cache_against(&server);
        let downloader = FeishuFileDownloader::new_with_api_base_for_tests(
            cache,
            dir.path().to_path_buf(),
            server.uri(),
        );

        let first = downloader
            .download("om_msg_3", "img_v2_002", AttachmentKind::Picture, "a.png")
            .await
            .unwrap();
        let second = downloader
            .download("om_msg_3", "img_v2_002", AttachmentKind::Picture, "a.png")
            .await
            .unwrap();

        assert_eq!(first.path, second.path);
        let files = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".png"))
            .count();
        assert_eq!(files, 1);
    }

    #[tokio::test]
    async fn download_404_returns_fetch_error() {
        let server = mock_server_with_token("tok-1").await;
        Mock::given(method("GET"))
            .and(wm_path(
                "/open-apis/im/v1/messages/om_msg_4/resources/img_missing",
            ))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let dir = tempfile::TempDir::new().unwrap();
        let cache = make_cache_against(&server);
        let downloader = FeishuFileDownloader::new_with_api_base_for_tests(
            cache,
            dir.path().to_path_buf(),
            server.uri(),
        );

        let err = downloader
            .download("om_msg_4", "img_missing", AttachmentKind::Picture, "a.png")
            .await
            .expect_err("404 propagates");
        assert!(matches!(
            err,
            FeishuDownloadError::Fetch { status: 404, .. }
        ));
    }

    #[tokio::test]
    async fn download_404_short_circuits_no_retry() {
        // 404 is non-retryable per is_retryable_error policy. We verify both:
        //   (a) wiremock receives exactly ONE hit (via `.expect(1)` — strict;
        //       wiremock panics on Drop if the count doesn't match), AND
        //   (b) total elapsed is well below the would-be retry budget
        //       (3 attempts * 500ms = 1500ms of backoff alone). 200ms is a
        //       generous ceiling that still catches accidental retries.
        let server = mock_server_with_token("tok-1").await;
        Mock::given(method("GET"))
            .and(wm_path(
                "/open-apis/im/v1/messages/om_msg_5/resources/img_gone",
            ))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::TempDir::new().unwrap();
        let cache = make_cache_against(&server);
        let downloader = FeishuFileDownloader::new_with_api_base_for_tests(
            cache,
            dir.path().to_path_buf(),
            server.uri(),
        );

        let start = std::time::Instant::now();
        let result = downloader
            .download("om_msg_5", "img_gone", AttachmentKind::Picture, "a.png")
            .await;
        let elapsed = start.elapsed();

        assert!(result.is_err());
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "404 must not trigger backoff retries, took {:?}",
            elapsed
        );
    }

    #[test]
    fn retry_policy_classifies_status_codes() {
        let mk = |s: u16| FeishuDownloadError::Fetch {
            status: s,
            body: String::new(),
        };
        // 4xx (other) non-retryable
        assert!(!is_retryable_error(&mk(400)));
        assert!(!is_retryable_error(&mk(401)));
        assert!(!is_retryable_error(&mk(403)));
        assert!(!is_retryable_error(&mk(404)));
        // 408 / 425 / 429 retryable
        assert!(is_retryable_error(&mk(408)));
        assert!(is_retryable_error(&mk(425)));
        assert!(is_retryable_error(&mk(429)));
        // 5xx retryable
        assert!(is_retryable_error(&mk(500)));
        assert!(is_retryable_error(&mk(502)));
        assert!(is_retryable_error(&mk(503)));
        // Token / Io non-retryable
        assert!(!is_retryable_error(&FeishuDownloadError::Token(
            anyhow::anyhow!("x")
        )));
        assert!(!is_retryable_error(&FeishuDownloadError::Io(
            std::io::Error::new(std::io::ErrorKind::Other, "x")
        )));
    }
}
