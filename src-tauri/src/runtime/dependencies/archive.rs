use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    UnsafeEntry { entry: String },
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsafeEntry { entry } => write!(f, "unsafe archive entry path: {entry}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

pub fn validate_archive_entry_path(dest: &Path, entry: &str) -> Result<PathBuf, ArchiveError> {
    let trimmed = entry.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed.contains('\\') {
        return Err(ArchiveError::UnsafeEntry {
            entry: entry.to_string(),
        });
    }

    let entry_path = Path::new(trimmed);
    let mut normalized = PathBuf::new();

    for component in entry_path.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ArchiveError::UnsafeEntry {
                    entry: entry.to_string(),
                });
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(ArchiveError::UnsafeEntry {
            entry: entry.to_string(),
        });
    }

    Ok(dest.join(normalized))
}
