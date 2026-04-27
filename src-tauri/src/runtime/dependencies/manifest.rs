use std::collections::BTreeMap;

use serde::Deserialize;

use super::RuntimePlatform;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifest {
    pub bundle_version: String,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub minimum_app_version: Option<String>,
    #[serde(default)]
    pub default_provider: Option<String>,
    pub source: String,
    #[serde(default)]
    pub rollback: Option<RuntimeRollback>,
    #[serde(default)]
    pub mirrors: Vec<String>,
    pub runtimes: BTreeMap<String, RuntimeSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRollback {
    pub bundle_version: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpec {
    pub version: String,
    pub platforms: BTreeMap<String, RuntimeArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeArtifact {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub archive_format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeManifestError {
    Json(String),
    MissingRuntime {
        name: String,
        platform: String,
    },
    InvalidSha256 {
        name: String,
        sha256: String,
    },
    EmptyRuntimes,
    EmptyPlatforms {
        name: String,
    },
    UntrustedArtifactUrl {
        name: String,
        platform: String,
        url: String,
    },
    InvalidArtifactSize {
        name: String,
        size_bytes: u64,
    },
    UnsupportedArchiveFormat {
        name: String,
        archive_format: String,
    },
}

impl RuntimeManifest {
    pub fn from_json(value: &str) -> Result<Self, RuntimeManifestError> {
        let manifest: Self = serde_json::from_str(value)
            .map_err(|error| RuntimeManifestError::Json(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn artifact(
        &self,
        name: &str,
        platform: RuntimePlatform,
    ) -> Result<&RuntimeArtifact, RuntimeManifestError> {
        let platform_key = platform.manifest_key();
        self.runtimes
            .get(name)
            .and_then(|runtime| runtime.platforms.get(platform_key))
            .ok_or_else(|| RuntimeManifestError::MissingRuntime {
                name: name.to_string(),
                platform: platform_key.to_string(),
            })
    }

    fn validate(&self) -> Result<(), RuntimeManifestError> {
        if self.runtimes.is_empty() {
            return Err(RuntimeManifestError::EmptyRuntimes);
        }

        for (name, spec) in &self.runtimes {
            if spec.platforms.is_empty() {
                return Err(RuntimeManifestError::EmptyPlatforms { name: name.clone() });
            }

            for (platform, artifact) in &spec.platforms {
                if !is_valid_sha256(&artifact.sha256) {
                    return Err(RuntimeManifestError::InvalidSha256 {
                        name: name.clone(),
                        sha256: artifact.sha256.clone(),
                    });
                }

                if !is_trusted_artifact_url(&artifact.url, &self.source) {
                    return Err(RuntimeManifestError::UntrustedArtifactUrl {
                        name: name.clone(),
                        platform: platform.clone(),
                        url: artifact.url.clone(),
                    });
                }

                if matches!(artifact.size_bytes, Some(0)) {
                    return Err(RuntimeManifestError::InvalidArtifactSize {
                        name: name.clone(),
                        size_bytes: 0,
                    });
                }

                if let Some(format) = &artifact.archive_format {
                    if !matches!(format.as_str(), "zip" | "tar.gz" | "tgz") {
                        return Err(RuntimeManifestError::UnsupportedArchiveFormat {
                            name: name.clone(),
                            archive_format: format.clone(),
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

impl std::fmt::Display for RuntimeManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid runtime manifest json: {error}"),
            Self::MissingRuntime { name, platform } => {
                write!(f, "missing runtime artifact for {name} on {platform}")
            }
            Self::InvalidSha256 { name, sha256 } => {
                write!(f, "invalid sha256 for runtime {name}: {sha256}")
            }
            Self::EmptyRuntimes => write!(f, "runtime manifest must include runtimes"),
            Self::EmptyPlatforms { name } => {
                write!(f, "runtime manifest must include platforms for {name}")
            }
            Self::UntrustedArtifactUrl {
                name,
                platform,
                url,
            } => write!(
                f,
                "untrusted runtime artifact url for {name} on {platform}: {url}"
            ),
            Self::InvalidArtifactSize { name, size_bytes } => {
                write!(f, "invalid artifact size for runtime {name}: {size_bytes}")
            }
            Self::UnsupportedArchiveFormat {
                name,
                archive_format,
            } => write!(
                f,
                "unsupported archive format for runtime {name}: {archive_format}"
            ),
        }
    }
}

impl std::error::Error for RuntimeManifestError {}

fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_trusted_artifact_url(value: &str, manifest_source: &str) -> bool {
    if manifest_source == "unit-test"
        || manifest_source == "test-fixture"
        || manifest_source == "local-dev"
    {
        return value
            .strip_prefix("file://")
            .map(|path| std::path::Path::new(path).is_absolute())
            .unwrap_or(false)
            || is_trusted_https_url(value);
    }

    is_trusted_https_url(value)
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
