use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeManifestSource {
    Url(String),
    File(PathBuf),
}

impl RuntimeManifestSource {
    pub fn as_url(&self) -> Option<&str> {
        match self {
            Self::Url(url) => Some(url.as_str()),
            Self::File(_) => None,
        }
    }

    pub fn as_file(&self) -> Option<&Path> {
        match self {
            Self::Url(_) => None,
            Self::File(path) => Some(path.as_path()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDownloadPlan {
    manifest_source: RuntimeManifestSource,
    platform: String,
}

impl RuntimeDownloadPlan {
    pub fn new(manifest_source: RuntimeManifestSource, platform: String) -> Self {
        Self {
            manifest_source,
            platform,
        }
    }

    pub fn manifest_source(&self) -> &RuntimeManifestSource {
        &self.manifest_source
    }

    pub fn platform(&self) -> &str {
        &self.platform
    }

    pub fn uses_shell_script(&self) -> bool {
        false
    }
}
