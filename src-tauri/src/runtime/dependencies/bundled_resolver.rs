//! `BundledRuntimeResolver` reads node/python/uv from the app's resource dir.
//! Layout: `<resource_dir>/runtime/<platform>/{node,python,uv}/...`
//! Populated at build time by `scripts/prepare-bundled-runtime.{sh,ps1}`.

use std::path::PathBuf;

use super::{
    RuntimeDependencyError, RuntimeDependencyResult, RuntimePlatform, RuntimePlatformError,
    RuntimeResolver, WorkspaceDependencies,
};

#[derive(Debug, Clone)]
pub struct BundledRuntimeResolver {
    resource_dir: PathBuf,
}

impl BundledRuntimeResolver {
    pub fn new(resource_dir: PathBuf) -> Self {
        Self { resource_dir }
    }

    pub fn runtime_dir(&self) -> RuntimeDependencyResult<PathBuf> {
        let platform = RuntimePlatform::current().map_err(platform_err)?;
        Ok(self
            .resource_dir
            .join("runtime")
            .join(platform.manifest_key()))
    }

    pub fn bundled_version(&self) -> Option<String> {
        let dir = self.runtime_dir().ok()?;
        let raw = std::fs::read_to_string(dir.join("bundled-version.json")).ok()?;
        let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
        json.get("bundleVersion")?.as_str().map(str::to_string)
    }
}

impl RuntimeResolver for BundledRuntimeResolver {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        let runtime_dir = self.runtime_dir()?;
        if !runtime_dir.is_dir() {
            return Err(RuntimeDependencyError::ResolverUnavailable(format!(
                "bundled runtime dir not found: {}",
                runtime_dir.display()
            )));
        }
        let platform = RuntimePlatform::current().map_err(platform_err)?;
        let deps = WorkspaceDependencies::from_install_dir_for_platform(&runtime_dir, platform)?;
        validate_existing(&deps)?;
        Ok(deps)
    }
}

fn platform_err(e: RuntimePlatformError) -> RuntimeDependencyError {
    RuntimeDependencyError::ResolverUnavailable(e.to_string())
}

fn validate_existing(deps: &WorkspaceDependencies) -> RuntimeDependencyResult<()> {
    for (field, path) in [
        ("node", &deps.node),
        ("npm", &deps.npm),
        ("npx", &deps.npx),
        ("python", &deps.python),
        ("uv", &deps.uv),
        ("uvx", &deps.uvx),
    ] {
        if !path.is_file() {
            return Err(RuntimeDependencyError::MissingExecutable {
                field,
                path: path.clone(),
            });
        }
    }
    Ok(())
}
