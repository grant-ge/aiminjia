use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDependencies {
    pub python: PathBuf,
    pub node: PathBuf,
    pub npm: PathBuf,
    pub npx: PathBuf,
    pub uv: PathBuf,
    pub uvx: PathBuf,
    pub node_modules: PathBuf,
    pub python_site_packages: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDependencyError {
    NonAbsolutePath { field: &'static str, path: PathBuf },
    MissingExecutable { field: &'static str, path: PathBuf },
    ResolverUnavailable(String),
}

impl std::fmt::Display for RuntimeDependencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonAbsolutePath { field, path } => {
                write!(
                    f,
                    "runtime dependency {field} must be absolute: {}",
                    path.display()
                )
            }
            Self::MissingExecutable { field, path } => {
                write!(
                    f,
                    "runtime dependency {field} is missing: {}",
                    path.display()
                )
            }
            Self::ResolverUnavailable(reason) => {
                write!(f, "runtime resolver is unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for RuntimeDependencyError {}

pub type RuntimeDependencyResult<T> = Result<T, RuntimeDependencyError>;
