use std::fs;
use std::path::{Path, PathBuf};

use super::{
    RuntimeDownloadOptions, RuntimeDownloader, RuntimeManifest, RuntimeManifestSource, RuntimePlatform,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedRuntimeArtifact {
    pub bundle_version: String,
    pub sha256: String,
    pub archive_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeArtifactFetchError {
    Io(String),
    Manifest(String),
    UnsupportedSource(String),
    UntrustedUrl(String),
    Network(String),
}

impl std::fmt::Display for RuntimeArtifactFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "runtime artifact fetch io error: {error}"),
            Self::Manifest(error) => write!(f, "runtime artifact manifest error: {error}"),
            Self::UnsupportedSource(source) => {
                write!(f, "unsupported runtime artifact source: {source}")
            }
            Self::UntrustedUrl(url) => write!(f, "untrusted runtime artifact url: {url}"),
            Self::Network(error) => write!(f, "runtime artifact fetch network error: {error}"),
        }
    }
}

impl std::error::Error for RuntimeArtifactFetchError {}

#[derive(Debug, Clone, Default)]
pub struct RuntimeArtifactFetcher;

impl RuntimeArtifactFetcher {
    pub fn new() -> Self {
        Self
    }

    pub fn fetch_from_manifest_source(
        &self,
        manifest_source: RuntimeManifestSource,
        runtime_name: &str,
        platform: RuntimePlatform,
        downloads_dir: &Path,
    ) -> Result<FetchedRuntimeArtifact, RuntimeArtifactFetchError> {
        let manifest_text = match manifest_source {
            RuntimeManifestSource::File(path) => fs::read_to_string(path).map_err(io_error)?,
            RuntimeManifestSource::Url(url) => {
                return Err(RuntimeArtifactFetchError::UnsupportedSource(format!(
                    "manifest url requires async fetcher: {url}"
                )))
            }
        };
        self.fetch_from_manifest_text(&manifest_text, runtime_name, platform, downloads_dir)
    }

    pub async fn fetch_from_manifest_url(
        &self,
        manifest_url: &str,
        runtime_name: &str,
        platform: RuntimePlatform,
        downloads_dir: &Path,
    ) -> Result<FetchedRuntimeArtifact, RuntimeArtifactFetchError> {
        self.fetch_from_manifest_url_with_options(
            manifest_url,
            runtime_name,
            platform,
            downloads_dir,
            RuntimeDownloadOptions::default(),
        )
        .await
    }

    pub async fn fetch_from_manifest_url_with_options(
        &self,
        manifest_url: &str,
        runtime_name: &str,
        platform: RuntimePlatform,
        downloads_dir: &Path,
        options: RuntimeDownloadOptions,
    ) -> Result<FetchedRuntimeArtifact, RuntimeArtifactFetchError> {
        if !is_trusted_https_url(manifest_url) {
            return Err(RuntimeArtifactFetchError::UntrustedUrl(
                manifest_url.to_string(),
            ));
        }
        let manifest_text = reqwest::get(manifest_url)
            .await
            .map_err(|error| RuntimeArtifactFetchError::Network(error.to_string()))?
            .error_for_status()
            .map_err(|error| RuntimeArtifactFetchError::Network(error.to_string()))?
            .text()
            .await
            .map_err(|error| RuntimeArtifactFetchError::Network(error.to_string()))?;
        self.fetch_from_manifest_text_async_with_options(
            &manifest_text,
            runtime_name,
            platform,
            downloads_dir,
            options,
        )
        .await
    }

    pub async fn fetch_https_artifact_to_downloads(
        &self,
        artifact_url: &str,
        downloads_dir: &Path,
    ) -> Result<PathBuf, RuntimeArtifactFetchError> {
        self.fetch_https_artifact_to_downloads_with_options(
            artifact_url,
            downloads_dir,
            RuntimeDownloadOptions::default(),
        )
        .await
    }

    pub async fn fetch_https_artifact_to_downloads_with_options(
        &self,
        artifact_url: &str,
        downloads_dir: &Path,
        options: RuntimeDownloadOptions,
    ) -> Result<PathBuf, RuntimeArtifactFetchError> {
        if !is_trusted_https_url(artifact_url) {
            return Err(RuntimeArtifactFetchError::UntrustedUrl(
                artifact_url.to_string(),
            ));
        }
        fs::create_dir_all(downloads_dir).map_err(io_error)?;
        let filename = artifact_url
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty() && !name.contains('/') && !name.contains('\\'))
            .ok_or_else(|| RuntimeArtifactFetchError::UntrustedUrl(artifact_url.to_string()))?;
        let archive_path = downloads_dir.join(filename);
        RuntimeDownloader::new()
            .download_to_file(artifact_url, &archive_path, options)
            .await
            .map_err(|error| RuntimeArtifactFetchError::Network(error.to_string()))
    }

    async fn fetch_from_manifest_text_async(
        &self,
        manifest_text: &str,
        runtime_name: &str,
        platform: RuntimePlatform,
        downloads_dir: &Path,
    ) -> Result<FetchedRuntimeArtifact, RuntimeArtifactFetchError> {
        self.fetch_from_manifest_text_async_with_options(
            manifest_text,
            runtime_name,
            platform,
            downloads_dir,
            RuntimeDownloadOptions::default(),
        )
        .await
    }

    async fn fetch_from_manifest_text_async_with_options(
        &self,
        manifest_text: &str,
        runtime_name: &str,
        platform: RuntimePlatform,
        downloads_dir: &Path,
        options: RuntimeDownloadOptions,
    ) -> Result<FetchedRuntimeArtifact, RuntimeArtifactFetchError> {
        let manifest = RuntimeManifest::from_json(manifest_text)
            .map_err(|error| RuntimeArtifactFetchError::Manifest(error.to_string()))?;
        let artifact = manifest
            .artifact(runtime_name, platform)
            .map_err(|error| RuntimeArtifactFetchError::Manifest(error.to_string()))?;

        fs::create_dir_all(downloads_dir).map_err(io_error)?;
        let archive_path = if artifact.url.starts_with("file://") {
            self.copy_file_artifact_to_downloads(&artifact.url, downloads_dir)?
        } else if is_trusted_https_url(&artifact.url) {
            self.fetch_https_artifact_to_downloads_with_options(&artifact.url, downloads_dir, options)
                .await?
        } else {
            return Err(RuntimeArtifactFetchError::UntrustedUrl(
                artifact.url.clone(),
            ));
        };

        Ok(FetchedRuntimeArtifact {
            bundle_version: manifest.bundle_version.clone(),
            sha256: artifact.sha256.clone(),
            archive_path,
        })
    }

    fn copy_file_artifact_to_downloads(
        &self,
        artifact_url: &str,
        downloads_dir: &Path,
    ) -> Result<PathBuf, RuntimeArtifactFetchError> {
        let source_path = file_url_to_path(artifact_url)?;
        let filename = source_path
            .file_name()
            .ok_or_else(|| RuntimeArtifactFetchError::UntrustedUrl(artifact_url.to_string()))?;
        let archive_path = downloads_dir.join(filename);
        fs::copy(&source_path, &archive_path).map_err(io_error)?;
        Ok(archive_path)
    }

    fn fetch_from_manifest_text(
        &self,
        manifest_text: &str,
        runtime_name: &str,
        platform: RuntimePlatform,
        downloads_dir: &Path,
    ) -> Result<FetchedRuntimeArtifact, RuntimeArtifactFetchError> {
        let manifest = RuntimeManifest::from_json(manifest_text)
            .map_err(|error| RuntimeArtifactFetchError::Manifest(error.to_string()))?;
        let artifact = manifest
            .artifact(runtime_name, platform)
            .map_err(|error| RuntimeArtifactFetchError::Manifest(error.to_string()))?;

        fs::create_dir_all(downloads_dir).map_err(io_error)?;
        let archive_path = if artifact.url.starts_with("file://") {
            self.copy_file_artifact_to_downloads(&artifact.url, downloads_dir)?
        } else if is_trusted_https_url(&artifact.url) {
            return Err(RuntimeArtifactFetchError::UnsupportedSource(format!(
                "artifact url requires async binary download: {}",
                artifact.url
            )));
        } else {
            return Err(RuntimeArtifactFetchError::UntrustedUrl(
                artifact.url.clone(),
            ));
        };

        Ok(FetchedRuntimeArtifact {
            bundle_version: manifest.bundle_version.clone(),
            sha256: artifact.sha256.clone(),
            archive_path,
        })
    }
}

fn file_url_to_path(url: &str) -> Result<PathBuf, RuntimeArtifactFetchError> {
    let Some(path) = url.strip_prefix("file://") else {
        return Err(RuntimeArtifactFetchError::UnsupportedSource(
            url.to_string(),
        ));
    };
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(RuntimeArtifactFetchError::UntrustedUrl(url.to_string()))
    }
}

fn is_trusted_https_url(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or_default();
    !host.is_empty()
        && host != "localhost"
        && !host.starts_with("127.")
        && host != "0.0.0.0"
        && host != "::1"
}

fn io_error(error: std::io::Error) -> RuntimeArtifactFetchError {
    RuntimeArtifactFetchError::Io(error.to_string())
}
