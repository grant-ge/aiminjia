use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::dingtalk_token::{get_access_token, TokenCache};

const DINGTALK_API: &str = "https://api.dingtalk.com";
const GET_URL_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct DingtalkFileDownloader {
    client: Client,
    token_cache: TokenCache,
    app_key: String,
    app_secret: String,
    dest_dir: PathBuf,
    api_base: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("token: {0:#}")]
    Token(anyhow::Error),
    #[error("get url: status={status} body={body}")]
    GetUrl { status: u16, body: String },
    #[error("network: {0}")]
    Network(reqwest::Error),
    #[error("io: {0}")]
    Io(std::io::Error),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadUrlResponse {
    download_url: String,
}

impl DingtalkFileDownloader {
    pub fn new(
        token_cache: TokenCache,
        app_key: String,
        app_secret: String,
        dest_dir: PathBuf,
    ) -> Self {
        Self::new_with_api_base(token_cache, app_key, app_secret, dest_dir, DINGTALK_API.to_string())
    }

    pub fn new_with_api_base(
        token_cache: TokenCache,
        app_key: String,
        app_secret: String,
        dest_dir: PathBuf,
        api_base: String,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(DOWNLOAD_TIMEOUT)
                .build()
                .expect("build reqwest client"),
            token_cache,
            app_key,
            app_secret,
            dest_dir,
            api_base,
        }
    }

    pub async fn download(
        &self,
        download_code: &str,
        robot_code: &str,
        original_file_name: &str,
    ) -> Result<DownloadedFile, DownloadError> {
        let display_name = safe_display_file_name(original_file_name);
        tokio::fs::create_dir_all(&self.dest_dir)
            .await
            .map_err(DownloadError::Io)?;
        let token = get_access_token(&self.token_cache, &self.app_key, &self.app_secret)
            .await
            .map_err(DownloadError::Token)?;
        let download_url = self
            .get_download_url(download_code, robot_code, &token)
            .await?;
        self.fetch_with_retries(&download_url, &display_name).await
    }

    async fn get_download_url(
        &self,
        download_code: &str,
        robot_code: &str,
        token: &str,
    ) -> Result<String, DownloadError> {
        let resp = self
            .client
            .post(format!("{}/v1.0/robot/messageFiles/download", self.api_base))
            .timeout(GET_URL_TIMEOUT)
            .header("x-acs-dingtalk-access-token", token)
            .json(&serde_json::json!({
                "downloadCode": download_code,
                "robotCode": robot_code,
            }))
            .send()
            .await
            .map_err(DownloadError::Network)?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DownloadError::GetUrl { status, body });
        }
        let data: DownloadUrlResponse = resp.json().await.map_err(DownloadError::Network)?;
        Ok(data.download_url)
    }

    async fn fetch_with_retries(
        &self,
        download_url: &str,
        display_name: &str,
    ) -> Result<DownloadedFile, DownloadError> {
        let mut last_error: Option<DownloadError> = None;
        for attempt in 0..3 {
            match self.fetch_once(download_url, display_name).await {
                Ok(file) => return Ok(file),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }
        Err(last_error.expect("download attempted at least once"))
    }

    async fn fetch_once(
        &self,
        download_url: &str,
        display_name: &str,
    ) -> Result<DownloadedFile, DownloadError> {
        let resp = self
            .client
            .get(download_url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .map_err(DownloadError::Network)?;
        let mime_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());
        let bytes = resp.bytes().await.map_err(DownloadError::Network)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = format!("{:x}", hasher.finalize());
        let ext = extension_or_bin(display_name);
        let final_path = self.dest_dir.join(format!("{}.{}", sha256, ext));
        if final_path.exists() {
            return Ok(DownloadedFile {
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
                .map_err(DownloadError::Io)?;
            file.write_all(&bytes).await.map_err(DownloadError::Io)?;
            file.flush().await.map_err(DownloadError::Io)?;
            Ok::<(), DownloadError>(())
        }
        .await;
        if let Err(error) = write_result {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(error);
        }
        if let Err(error) = tokio::fs::rename(&tmp_path, &final_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(DownloadError::Io(error));
        }
        Ok(DownloadedFile {
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

#[cfg(test)]
mod tests {
    use super::*;

    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
    async fn download_two_step_happy_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/robot/messageFiles/download"))
            .and(header("x-acs-dingtalk-access-token", "token-1"))
            .and(body_json(serde_json::json!({
                "downloadCode": "code-1",
                "robotCode": "robot-1"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "downloadUrl": format!("{}/download/file", server.uri())
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/download/file"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/plain")
                    .set_body_bytes("hello"),
            )
            .mount(&server)
            .await;

        let dir = tempfile::TempDir::new().unwrap();
        let cache = TokenCache::new();
        cache.set("token-1".into(), 7200).await;
        let downloader = DingtalkFileDownloader::new_with_api_base(
            cache,
            "app-key".into(),
            "app-secret".into(),
            dir.path().to_path_buf(),
            server.uri(),
        );

        let file = downloader
            .download("code-1", "robot-1", "note.txt")
            .await
            .expect("download succeeds");

        assert_eq!(file.file_name, "note.txt");
        assert_eq!(file.size, 5);
        assert_eq!(file.mime_type.as_deref(), Some("text/plain"));
        assert_eq!(file.path.extension().and_then(|v| v.to_str()), Some("txt"));
        assert_eq!(std::fs::read(&file.path).unwrap(), b"hello");
    }

    #[tokio::test]
    async fn download_dedup_when_same_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/robot/messageFiles/download"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "downloadUrl": format!("{}/download/file", server.uri())
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/download/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes("same"))
            .mount(&server)
            .await;

        let dir = tempfile::TempDir::new().unwrap();
        let cache = TokenCache::new();
        cache.set("token-1".into(), 7200).await;
        let downloader = DingtalkFileDownloader::new_with_api_base(
            cache,
            "app-key".into(),
            "app-secret".into(),
            dir.path().to_path_buf(),
            server.uri(),
        );

        let first = downloader.download("code-1", "robot-1", "a.bin").await.unwrap();
        let second = downloader.download("code-2", "robot-1", "a.bin").await.unwrap();

        assert_eq!(first.path, second.path);
        let files = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".bin"))
            .count();
        assert_eq!(files, 1);
    }

    #[tokio::test]
    async fn download_geturl_failure_returns_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/robot/messageFiles/download"))
            .respond_with(ResponseTemplate::new(410).set_body_string("expired"))
            .mount(&server)
            .await;

        let dir = tempfile::TempDir::new().unwrap();
        let cache = TokenCache::new();
        cache.set("token-1".into(), 7200).await;
        let downloader = DingtalkFileDownloader::new_with_api_base(
            cache,
            "app-key".into(),
            "app-secret".into(),
            dir.path().to_path_buf(),
            server.uri(),
        );

        let err = downloader
            .download("bad-code", "robot-1", "a.bin")
            .await
            .expect_err("geturl fails");
        assert!(matches!(err, DownloadError::GetUrl { status: 410, .. }));
    }
}
