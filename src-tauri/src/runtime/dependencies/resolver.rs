use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::types::{RuntimeDependencyError, RuntimeDependencyResult, WorkspaceDependencies};
use super::{RuntimeLayout, RuntimePlatform, RuntimePlatformError};

pub trait RuntimeResolver: Send + Sync {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies>;
}

pub type ManagedRuntimeResolver = Arc<dyn RuntimeResolver>;

#[derive(Debug, Clone)]
pub struct InstalledRuntimeResolver {
    bundle_root: PathBuf,
}

impl InstalledRuntimeResolver {
    pub fn new(bundle_root: impl Into<PathBuf>) -> Self {
        Self {
            bundle_root: bundle_root.into(),
        }
    }

    fn current_install_dir(&self) -> RuntimeDependencyResult<PathBuf> {
        validate_absolute("runtime_bundle_root", &self.bundle_root)?;

        let current = self.bundle_root.join("current");
        let pointer = fs::read_to_string(&current).map_err(|error| {
            RuntimeDependencyError::ResolverUnavailable(format!(
                "failed to read runtime current pointer {}: {error}",
                current.display()
            ))
        })?;

        let pointer = pointer.trim();
        if pointer.is_empty() || pointer.starts_with('/') || pointer.contains("..") {
            return Err(RuntimeDependencyError::ResolverUnavailable(format!(
                "runtime current pointer is invalid: {pointer}"
            )));
        }

        let install_dir = self.bundle_root.join(pointer);
        validate_absolute("runtime_install_dir", &install_dir)?;
        if !install_dir.starts_with(&self.bundle_root) {
            return Err(RuntimeDependencyError::ResolverUnavailable(format!(
                "runtime current pointer escapes bundle root: {pointer}"
            )));
        }

        Ok(install_dir)
    }
}

impl RuntimeResolver for InstalledRuntimeResolver {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        let install_dir = self.current_install_dir()?;
        let dependencies = WorkspaceDependencies::from_install_dir(&install_dir)?;
        validate_installed_dependencies(&dependencies)?;
        Ok(dependencies)
    }
}

impl WorkspaceDependencies {
    pub fn from_install_dir(install_dir: &Path) -> RuntimeDependencyResult<Self> {
        let platform = RuntimePlatform::current().map_err(runtime_platform_error)?;
        Self::from_install_dir_for_platform(install_dir, platform)
    }

    pub fn from_install_dir_for_platform(
        install_dir: &Path,
        platform: RuntimePlatform,
    ) -> RuntimeDependencyResult<Self> {
        let dependencies =
            RuntimeLayout::for_platform(platform).workspace_dependencies(install_dir);
        validate_dependencies(&dependencies)?;
        Ok(dependencies)
    }
}

#[derive(Debug, Clone)]
pub struct StaticRuntimeResolver {
    dependencies: WorkspaceDependencies,
}

impl StaticRuntimeResolver {
    pub fn new(
        python: PathBuf,
        node: PathBuf,
        npm: PathBuf,
        npx: PathBuf,
        uv: PathBuf,
        uvx: PathBuf,
        node_modules: PathBuf,
        python_site_packages: PathBuf,
    ) -> Self {
        Self {
            dependencies: WorkspaceDependencies {
                python,
                node,
                npm,
                npx,
                uv,
                uvx,
                node_modules,
                python_site_packages,
            },
        }
    }
}

impl RuntimeResolver for StaticRuntimeResolver {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        validate_dependencies(&self.dependencies)?;
        Ok(self.dependencies.clone())
    }
}

fn validate_dependencies(dependencies: &WorkspaceDependencies) -> RuntimeDependencyResult<()> {
    validate_absolute("node", &dependencies.node)?;
    validate_absolute("npm", &dependencies.npm)?;
    validate_absolute("npx", &dependencies.npx)?;
    validate_absolute("python", &dependencies.python)?;
    validate_absolute("uv", &dependencies.uv)?;
    validate_absolute("uvx", &dependencies.uvx)?;
    validate_absolute("node_modules", &dependencies.node_modules)?;
    validate_absolute("python_site_packages", &dependencies.python_site_packages)?;
    Ok(())
}

fn validate_installed_dependencies(
    dependencies: &WorkspaceDependencies,
) -> RuntimeDependencyResult<()> {
    validate_dependencies(dependencies)?;
    validate_existing("node", &dependencies.node)?;
    validate_existing("npm", &dependencies.npm)?;
    validate_existing("npx", &dependencies.npx)?;
    validate_existing("python", &dependencies.python)?;
    validate_existing("uv", &dependencies.uv)?;
    validate_existing("uvx", &dependencies.uvx)?;
    validate_existing_dir("node_modules", &dependencies.node_modules)?;
    validate_existing_dir("python_site_packages", &dependencies.python_site_packages)?;
    let install_manifest = infer_install_dir_from_python_path(&dependencies.python)
        .map(|install_dir| install_dir.join("install.json"))
        .ok_or_else(|| {
            RuntimeDependencyError::ResolverUnavailable(
                "failed to derive runtime install manifest path".to_string(),
            )
        })?;
    validate_existing("install_json", &install_manifest)?;
    Ok(())
}

fn infer_install_dir_from_python_path(python_path: &Path) -> Option<PathBuf> {
    for platform in [
        RuntimePlatform::DarwinArm64,
        RuntimePlatform::DarwinX64,
        RuntimePlatform::LinuxX64,
        RuntimePlatform::WindowsX64,
    ] {
        if let Some(install_dir) =
            RuntimeLayout::for_platform(platform).install_dir_from_python_path(python_path)
        {
            return Some(install_dir);
        }
    }
    None
}

fn runtime_platform_error(error: RuntimePlatformError) -> RuntimeDependencyError {
    RuntimeDependencyError::ResolverUnavailable(error.to_string())
}

fn validate_existing(field: &'static str, path: &PathBuf) -> RuntimeDependencyResult<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(RuntimeDependencyError::MissingExecutable {
            field,
            path: path.clone(),
        })
    }
}

fn validate_existing_dir(field: &'static str, path: &PathBuf) -> RuntimeDependencyResult<()> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(RuntimeDependencyError::MissingExecutable {
            field,
            path: path.clone(),
        })
    }
}

fn validate_absolute(field: &'static str, path: &PathBuf) -> RuntimeDependencyResult<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(RuntimeDependencyError::NonAbsolutePath {
            field,
            path: path.clone(),
        })
    }
}
