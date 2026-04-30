use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub attempt: u8,
    pub max_attempts: u8,
    pub resumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDownloadError {
    Network(String),
    Io(String),
    InvalidStatus(u16),
    Cancelled,
    TooManyRetries(String),
}

impl std::fmt::Display for RuntimeDownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(error) => write!(f, "runtime download network error: {error}"),
            Self::Io(error) => write!(f, "runtime download io error: {error}"),
            Self::InvalidStatus(status) => {
                write!(f, "runtime download returned invalid status: {status}")
            }
            Self::Cancelled => write!(f, "runtime download cancelled"),
            Self::TooManyRetries(error) => write!(f, "runtime download retries exhausted: {error}"),
        }
    }
}

impl std::error::Error for RuntimeDownloadError {}

#[derive(Debug, Clone)]
pub struct RuntimeDownloadRetryPolicy {
    pub max_attempts: u8,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
}

impl Default for RuntimeDownloadRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff_ms: 500,
            max_backoff_ms: 5_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeDownloadCancellation {
    cancelled: Arc<AtomicBool>,
}

impl RuntimeDownloadCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub trait RuntimeDownloadProgressSink: Send + Sync {
    fn on_progress(&self, progress: RuntimeDownloadProgress);
    fn on_retry(&self, attempt: u8, max_attempts: u8, message: &str);
}

#[derive(Clone, Default)]
pub struct RuntimeDownloadOptions {
    pub cancellation: RuntimeDownloadCancellation,
    pub progress: Option<Arc<dyn RuntimeDownloadProgressSink>>,
    pub retry: RuntimeDownloadRetryPolicy,
}

#[derive(Debug, Clone)]
pub struct RuntimeDownloader {
    client: reqwest::Client,
}

impl Default for RuntimeDownloader {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl RuntimeDownloader {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn download_to_file(
        &self,
        url: &str,
        destination: &Path,
        options: RuntimeDownloadOptions,
    ) -> Result<PathBuf, RuntimeDownloadError> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let part_path = destination.with_extension(part_extension(destination));
        let meta_path = destination.with_extension(meta_extension(destination));

        let max_attempts = options.retry.max_attempts.max(1);
        let mut last_error = None;
        for attempt in 1..=max_attempts {
            if options.cancellation.is_cancelled() {
                return Err(RuntimeDownloadError::Cancelled);
            }
            match self
                .download_once(
                    url,
                    destination,
                    &part_path,
                    &meta_path,
                    attempt,
                    max_attempts,
                    &options,
                )
                .await
            {
                Ok(path) => return Ok(path),
                Err(RuntimeDownloadError::Cancelled) => {
                    return Err(RuntimeDownloadError::Cancelled)
                }
                Err(RuntimeDownloadError::InvalidStatus(status)) if status < 500 => {
                    return Err(RuntimeDownloadError::InvalidStatus(status));
                }
                Err(error) => {
                    let message = error.to_string();
                    last_error = Some(message.clone());
                    if attempt == max_attempts {
                        break;
                    }
                    if let Some(sink) = &options.progress {
                        sink.on_retry(attempt + 1, max_attempts, &message);
                    }
                    let backoff = options
                        .retry
                        .initial_backoff_ms
                        .saturating_mul(2_u64.saturating_pow((attempt - 1) as u32))
                        .min(options.retry.max_backoff_ms);
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                }
            }
        }

        Err(RuntimeDownloadError::TooManyRetries(
            last_error.unwrap_or_else(|| "unknown error".to_string()),
        ))
    }

    async fn download_once(
        &self,
        url: &str,
        destination: &Path,
        part_path: &Path,
        meta_path: &Path,
        attempt: u8,
        max_attempts: u8,
        options: &RuntimeDownloadOptions,
    ) -> Result<PathBuf, RuntimeDownloadError> {
        let resumed = part_path.exists();
        let existing_len = if resumed {
            fs::metadata(part_path).map_err(io_error)?.len()
        } else {
            0
        };

        let mut request = self.client.get(url);
        if existing_len > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={existing_len}-"));
        }
        let response = request
            .send()
            .await
            .map_err(|error| RuntimeDownloadError::Network(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(RuntimeDownloadError::InvalidStatus(status.as_u16()));
        }

        let append = existing_len > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
        if existing_len > 0 && !append {
            let _ = fs::remove_file(part_path);
        }
        let starting_bytes = if append { existing_len } else { 0 };
        let total_bytes = response
            .content_length()
            .map(|len| len.saturating_add(starting_bytes));
        write_meta(meta_path, url, starting_bytes, total_bytes)?;

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(part_path)
            .await
            .map_err(|error| RuntimeDownloadError::Io(error.to_string()))?;
        let mut downloaded = starting_bytes;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if options.cancellation.is_cancelled() {
                file.flush()
                    .await
                    .map_err(|error| RuntimeDownloadError::Io(error.to_string()))?;
                return Err(RuntimeDownloadError::Cancelled);
            }
            let chunk = chunk.map_err(|error| RuntimeDownloadError::Network(error.to_string()))?;
            file.write_all(&chunk)
                .await
                .map_err(|error| RuntimeDownloadError::Io(error.to_string()))?;
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            write_meta(meta_path, url, downloaded, total_bytes)?;
            if let Some(sink) = &options.progress {
                sink.on_progress(RuntimeDownloadProgress {
                    downloaded_bytes: downloaded,
                    total_bytes,
                    attempt,
                    max_attempts,
                    resumed: append,
                });
            }
        }
        file.flush()
            .await
            .map_err(|error| RuntimeDownloadError::Io(error.to_string()))?;
        drop(file);
        fs::rename(part_path, destination).map_err(io_error)?;
        let _ = fs::remove_file(meta_path);
        Ok(destination.to_path_buf())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeDownloadPartMeta {
    url: String,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}

fn write_meta(
    path: &Path,
    url: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) -> Result<(), RuntimeDownloadError> {
    let meta = RuntimeDownloadPartMeta {
        url: url.to_string(),
        downloaded_bytes,
        total_bytes,
    };
    let bytes = serde_json::to_vec_pretty(&meta)
        .map_err(|error| RuntimeDownloadError::Io(error.to_string()))?;
    fs::write(path, bytes).map_err(io_error)
}

fn part_extension(destination: &Path) -> String {
    match destination.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => format!("{ext}.part"),
        None => "part".to_string(),
    }
}

fn meta_extension(destination: &Path) -> String {
    match destination.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => format!("{ext}.part.meta.json"),
        None => "part.meta.json".to_string(),
    }
}

fn io_error(error: std::io::Error) -> RuntimeDownloadError {
    RuntimeDownloadError::Io(error.to_string())
}
