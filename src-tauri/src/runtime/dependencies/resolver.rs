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
        validate_installed_dependencies(&dependencies, &install_dir)?;
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
    install_dir: &Path,
) -> RuntimeDependencyResult<()> {
    validate_dependencies(dependencies)?;
    validate_existing("node", &dependencies.node)?;
    validate_existing("npm", &dependencies.npm)?;
    validate_existing("npx", &dependencies.npx)?;
    validate_existing("python", &dependencies.python)?;
    validate_existing("uv", &dependencies.uv)?;
    validate_existing("uvx", &dependencies.uvx)?;
    let install_manifest = install_dir.join("install.json");
    validate_existing("install_json", &install_manifest)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// 在临时目录里模拟 Windows layout 的已安装 runtime bundle，
    /// 验证 install.json 被正确定位到版本子目录而不是 versions/ 顶层。
    #[test]
    fn installed_resolver_finds_install_json_in_version_subdir() {
        let tmp = TempDir::new().unwrap();
        let bundle_root = tmp.path().join("renlijia-primary-runtime");
        let version = "2026.04.26-runtime.1";
        let install_dir = bundle_root.join("versions").join(version);

        // 写 current 指针
        std::fs::create_dir_all(&bundle_root).unwrap();
        std::fs::write(bundle_root.join("current"), format!("versions/{version}")).unwrap();

        // 用当前平台 layout 创建必要的可执行文件/目录
        let platform = RuntimePlatform::current().expect("should detect platform");
        let layout = RuntimeLayout::for_platform(platform);
        let deps = layout.workspace_dependencies(&install_dir);

        for path in [
            &deps.python,
            &deps.node,
            &deps.npm,
            &deps.npx,
            &deps.uv,
            &deps.uvx,
        ] {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, b"").unwrap();
        }
        for dir in [&deps.node_modules, &deps.python_site_packages] {
            std::fs::create_dir_all(dir).unwrap();
        }

        // 关键：install.json 放在版本子目录，不放在 versions/ 顶层
        std::fs::write(install_dir.join("install.json"), b"{}").unwrap();

        let resolver = InstalledRuntimeResolver::new(&bundle_root);
        let result = resolver.workspace_dependencies();
        assert!(
            result.is_ok(),
            "should resolve correctly on all platforms, got: {:?}",
            result.err()
        );
    }

    /// Runtime 可用性只取决于核心可执行文件是否就绪，不应取决于
    /// `node/node_modules` / `python/lib/site-packages` 这两个包目录是否存在。
    /// 这两个目录的布局会随 Node/Python 版本变化（真实全局包在
    /// `node/lib/node_modules`、`python/lib/python3.12/site-packages`），
    /// 不能用它们误判一个可执行的 Cache runtime 不可用，否则 cache probe 会
    /// false-negative，触发不必要的 reinstall 并清掉用户已装的第三方包。
    #[test]
    fn installed_resolver_resolves_without_package_dirs() {
        let tmp = TempDir::new().unwrap();
        let bundle_root = tmp.path().join("renlijia-primary-runtime");
        let version = "2026.04.26-runtime.1";
        let install_dir = bundle_root.join("versions").join(version);

        std::fs::create_dir_all(&bundle_root).unwrap();
        std::fs::write(bundle_root.join("current"), format!("versions/{version}")).unwrap();

        let platform = RuntimePlatform::current().expect("should detect platform");
        let layout = RuntimeLayout::for_platform(platform);
        let deps = layout.workspace_dependencies(&install_dir);

        for path in [
            &deps.python,
            &deps.node,
            &deps.npm,
            &deps.npx,
            &deps.uv,
            &deps.uvx,
        ] {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, b"").unwrap();
        }

        // 故意不创建 deps.node_modules / deps.python_site_packages 占位目录。
        std::fs::write(install_dir.join("install.json"), b"{}").unwrap();

        let resolver = InstalledRuntimeResolver::new(&bundle_root);
        let result = resolver.workspace_dependencies();
        assert!(
            result.is_ok(),
            "executable runtime must resolve even without package dirs, got: {:?}",
            result.err()
        );
    }
}
