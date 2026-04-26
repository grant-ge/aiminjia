use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    cache_root: PathBuf,
    bundle_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePathError {
    NonAbsoluteCacheRoot { path: PathBuf },
    UnsafePathSegment { field: &'static str, value: String },
}

impl std::fmt::Display for RuntimePathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonAbsoluteCacheRoot { path } => {
                write!(f, "runtime cache root must be absolute: {}", path.display())
            }
            Self::UnsafePathSegment { field, value } => {
                write!(
                    f,
                    "runtime path {field} must be a safe path segment: {value}"
                )
            }
        }
    }
}

impl std::error::Error for RuntimePathError {}

impl RuntimePaths {
    pub fn new(
        cache_root: PathBuf,
        bundle_id: impl Into<String>,
    ) -> Result<Self, RuntimePathError> {
        if !cache_root.is_absolute() {
            return Err(RuntimePathError::NonAbsoluteCacheRoot { path: cache_root });
        }

        let bundle_id = bundle_id.into();
        validate_safe_path_segment("bundle_id", &bundle_id)?;

        Ok(Self {
            cache_root,
            bundle_id,
        })
    }

    pub fn bundle_root(&self) -> PathBuf {
        self.cache_root.join(&self.bundle_id)
    }

    pub fn current_dir(&self) -> PathBuf {
        self.bundle_root().join("current")
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.bundle_root().join("versions")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.bundle_root().join("downloads")
    }

    pub fn staging_dir(&self) -> PathBuf {
        self.bundle_root().join("staging")
    }

    pub fn version_dir(&self, version: impl AsRef<str>) -> Result<PathBuf, RuntimePathError> {
        let version = version.as_ref();
        validate_safe_path_segment("version", version)?;
        Ok(self.versions_dir().join(version))
    }
}

fn validate_safe_path_segment(field: &'static str, value: &str) -> Result<(), RuntimePathError> {
    let components = Path::new(value).components().collect::<Vec<_>>();
    let is_single_normal_segment = matches!(components.as_slice(), [Component::Normal(_)]);

    if is_single_normal_segment {
        Ok(())
    } else {
        Err(RuntimePathError::UnsafePathSegment {
            field,
            value: value.to_string(),
        })
    }
}
