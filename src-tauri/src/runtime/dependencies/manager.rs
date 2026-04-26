use std::path::Path;
use std::sync::{Arc, Mutex};

use super::{
    InstalledRuntimeResolver, RuntimeArtifactFetchError, RuntimeArtifactFetcher,
    RuntimeDependencyResult, RuntimeHealthChecker, RuntimeHealthError, RuntimeHealthReport,
    RuntimeInstallError, RuntimeInstallPlan, RuntimeInstallResult, RuntimeInstaller,
    RuntimeDownloadCancellation, RuntimeDownloadOptions, RuntimeManifestSource, RuntimePaths, RuntimePlatform,
    RuntimeResolver, RuntimeToolProbe, WorkspaceDependencies,
};

pub type ManagedRuntimeManager = Arc<RuntimeManager>;

#[derive(Debug, Clone)]
pub struct RuntimeManager {
    paths: RuntimePaths,
    bundle_version: String,
    installer: RuntimeInstaller,
    resolver: InstalledRuntimeResolver,
    health_checker: RuntimeHealthChecker,
    manifest_install: Option<RuntimeManifestInstallConfig>,
    active_operation: Arc<Mutex<Option<RuntimeActiveOperation>>>,
}

#[derive(Debug, Clone)]
struct RuntimeActiveOperation {
    operation_id: String,
    cancellation: RuntimeDownloadCancellation,
}

#[derive(Debug, Clone)]
struct RuntimeManifestInstallConfig {
    source: RuntimeManifestSource,
    runtime_name: String,
    platform: RuntimePlatform,
}

#[derive(Debug)]
pub enum RuntimeManagerError {
    Install(RuntimeInstallError),
    Dependency(super::RuntimeDependencyError),
    Health(RuntimeHealthError),
    Fetch(RuntimeArtifactFetchError),
    ManifestNotConfigured,
    OperationInProgress { operation_id: String },
}

impl std::fmt::Display for RuntimeManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Install(error) => write!(f, "{error}"),
            Self::Dependency(error) => write!(f, "{error}"),
            Self::Health(error) => write!(f, "{error}"),
            Self::Fetch(error) => write!(f, "{error}"),
            Self::ManifestNotConfigured => write!(
                f,
                "managed runtime manifest is not configured; set RENLIJIA_RUNTIME_MANIFEST_URL or inject a manifest source"
            ),
            Self::OperationInProgress { operation_id } => {
                write!(f, "managed runtime operation is already in progress: {operation_id}")
            }
        }
    }
}

impl std::error::Error for RuntimeManagerError {}

impl From<RuntimeInstallError> for RuntimeManagerError {
    fn from(value: RuntimeInstallError) -> Self {
        Self::Install(value)
    }
}

impl From<super::RuntimeDependencyError> for RuntimeManagerError {
    fn from(value: super::RuntimeDependencyError) -> Self {
        Self::Dependency(value)
    }
}

impl From<RuntimeHealthError> for RuntimeManagerError {
    fn from(value: RuntimeHealthError) -> Self {
        Self::Health(value)
    }
}

impl From<RuntimeArtifactFetchError> for RuntimeManagerError {
    fn from(value: RuntimeArtifactFetchError) -> Self {
        Self::Fetch(value)
    }
}


impl RuntimeResolver for RuntimeManager {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        match self.resolver.workspace_dependencies() {
            Ok(dependencies) => Ok(dependencies),
            Err(error) if self.manifest_install.is_some() => {
                self.ensure().map_err(|ensure_error| {
                    super::RuntimeDependencyError::ResolverUnavailable(format!(
                        "failed to ensure managed runtime after resolver error ({error}): {ensure_error}"
                    ))
                })?;
                self.resolver.workspace_dependencies()
            }
            Err(error) => Err(error),
        }
    }
}

impl RuntimeManager {
    pub fn new(paths: RuntimePaths, bundle_version: impl Into<String>) -> Self {
        let bundle_root = paths.bundle_root();
        Self {
            installer: RuntimeInstaller::new(paths.clone()),
            resolver: InstalledRuntimeResolver::new(bundle_root),
            paths,
            bundle_version: bundle_version.into(),
            health_checker: RuntimeHealthChecker::default(),
            manifest_install: None,
            active_operation: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub fn with_health_checker(mut self, health_checker: RuntimeHealthChecker) -> Self {
        self.health_checker = health_checker;
        self
    }


    pub fn with_manifest_source(
        mut self,
        source: RuntimeManifestSource,
        runtime_name: impl Into<String>,
        platform: RuntimePlatform,
    ) -> Self {
        self.manifest_install = Some(RuntimeManifestInstallConfig {
            source,
            runtime_name: runtime_name.into(),
            platform,
        });
        self
    }

    pub fn has_manifest_source(&self) -> bool {
        self.manifest_install.is_some()
    }


    pub fn begin_operation(
        &self,
        operation_id: impl Into<String>,
    ) -> Result<RuntimeDownloadCancellation, RuntimeManagerError> {
        let operation_id = operation_id.into();
        let cancellation = RuntimeDownloadCancellation::default();
        let mut active = self.active_operation.lock().map_err(|error| {
            RuntimeManagerError::Dependency(super::RuntimeDependencyError::ResolverUnavailable(
                error.to_string(),
            ))
        })?;
        if let Some(existing) = active.as_ref() {
            return Err(RuntimeManagerError::OperationInProgress {
                operation_id: existing.operation_id.clone(),
            });
        }
        *active = Some(RuntimeActiveOperation {
            operation_id,
            cancellation: cancellation.clone(),
        });
        Ok(cancellation)
    }

    pub fn finish_operation(&self, operation_id: &str) {
        if let Ok(mut active) = self.active_operation.lock() {
            if active
                .as_ref()
                .map(|operation| operation.operation_id.as_str() == operation_id)
                .unwrap_or(false)
            {
                *active = None;
            }
        }
    }

    pub fn cancel_operation(&self, operation_id: &str) -> bool {
        let Ok(active) = self.active_operation.lock() else {
            return false;
        };
        let Some(operation) = active.as_ref() else {
            return false;
        };
        if operation.operation_id != operation_id {
            return false;
        }
        operation.cancellation.cancel();
        true
    }

    pub fn resolver(&self) -> InstalledRuntimeResolver {
        self.resolver.clone()
    }

    pub fn bundle_version(&self) -> &str {
        &self.bundle_version
    }

    pub fn ensure(&self) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        if let Some(config) = &self.manifest_install {
            return self.install_from_configured_manifest_blocking(config);
        }

        self.installer
            .ensure(RuntimeInstallPlan::already_local(
                self.bundle_version.clone(),
            ))
            .map_err(RuntimeManagerError::from)
    }

    pub fn reinstall(&self) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        if let Some(config) = &self.manifest_install {
            return self.install_from_configured_manifest_blocking(config);
        }

        self.installer
            .reinstall(RuntimeInstallPlan::already_local(
                self.bundle_version.clone(),
            ))
            .map_err(RuntimeManagerError::from)
    }


    pub async fn ensure_managed(&self) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        let config = self
            .manifest_install
            .as_ref()
            .ok_or(RuntimeManagerError::ManifestNotConfigured)?;
        self.install_from_configured_manifest(config).await
    }

    pub async fn reinstall_managed(&self) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        let config = self
            .manifest_install
            .as_ref()
            .ok_or(RuntimeManagerError::ManifestNotConfigured)?;
        self.install_from_configured_manifest(config).await
    }



    pub async fn ensure_managed_with_download_options(
        &self,
        options: RuntimeDownloadOptions,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        let config = self
            .manifest_install
            .as_ref()
            .ok_or(RuntimeManagerError::ManifestNotConfigured)?;
        self.install_from_configured_manifest_with_options(config, options)
            .await
    }

    pub async fn reinstall_managed_with_download_options(
        &self,
        options: RuntimeDownloadOptions,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        let config = self
            .manifest_install
            .as_ref()
            .ok_or(RuntimeManagerError::ManifestNotConfigured)?;
        self.install_from_configured_manifest_with_options(config, options)
            .await
    }

    fn install_from_configured_manifest_blocking(
        &self,
        config: &RuntimeManifestInstallConfig,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        match &config.source {
            RuntimeManifestSource::File(path) => self.install_from_manifest_source(
                RuntimeManifestSource::File(path.clone()),
                &config.runtime_name,
                config.platform,
            ),
            RuntimeManifestSource::Url(url) => tauri::async_runtime::block_on(self.install_from_manifest_url(
                url,
                &config.runtime_name,
                config.platform,
            )),
        }
    }

    async fn install_from_configured_manifest(
        &self,
        config: &RuntimeManifestInstallConfig,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        match &config.source {
            RuntimeManifestSource::File(path) => self.install_from_manifest_source(
                RuntimeManifestSource::File(path.clone()),
                &config.runtime_name,
                config.platform,
            ),
            RuntimeManifestSource::Url(url) => {
                self.install_from_manifest_url(url, &config.runtime_name, config.platform)
                    .await
            }
        }
    }


    async fn install_from_configured_manifest_with_options(
        &self,
        config: &RuntimeManifestInstallConfig,
        options: RuntimeDownloadOptions,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        match &config.source {
            RuntimeManifestSource::File(path) => self.install_from_manifest_source(
                RuntimeManifestSource::File(path.clone()),
                &config.runtime_name,
                config.platform,
            ),
            RuntimeManifestSource::Url(url) => {
                let fetched = RuntimeArtifactFetcher::new()
                    .fetch_from_manifest_url_with_options(
                        url,
                        &config.runtime_name,
                        config.platform,
                        &self.paths.downloads_dir(),
                        options,
                    )
                    .await?;
                self.install_fetched_artifact(fetched)
            }
        }
    }

    pub fn install_verified_archive(
        &self,
        archive_path: &Path,
        expected_sha256: &str,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        self.installer
            .install_from_verified_archive(
                RuntimeInstallPlan::reinstall(self.bundle_version.clone()),
                archive_path,
                expected_sha256,
            )
            .map_err(RuntimeManagerError::from)
    }

    pub fn install_from_manifest_source(
        &self,
        manifest_source: RuntimeManifestSource,
        runtime_name: &str,
        platform: RuntimePlatform,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        let fetched = RuntimeArtifactFetcher::new().fetch_from_manifest_source(
            manifest_source,
            runtime_name,
            platform,
            &self.paths.downloads_dir(),
        )?;
        self.install_fetched_artifact(fetched)
    }

    pub async fn install_from_manifest_url(
        &self,
        manifest_url: &str,
        runtime_name: &str,
        platform: RuntimePlatform,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        let fetched = RuntimeArtifactFetcher::new()
            .fetch_from_manifest_url(
                manifest_url,
                runtime_name,
                platform,
                &self.paths.downloads_dir(),
            )
            .await?;
        self.install_fetched_artifact(fetched)
    }

    fn install_fetched_artifact(
        &self,
        fetched: super::FetchedRuntimeArtifact,
    ) -> Result<RuntimeInstallResult, RuntimeManagerError> {
        self.installer
            .install_from_verified_archive(
                RuntimeInstallPlan::reinstall(fetched.bundle_version),
                &fetched.archive_path,
                &fetched.sha256,
            )
            .map_err(RuntimeManagerError::from)
    }


    pub fn cleanup_old_versions(
        &self,
        keep_versions: usize,
    ) -> Result<super::RuntimeCleanupResult, RuntimeManagerError> {
        self.installer
            .cleanup_old_versions(keep_versions)
            .map_err(RuntimeManagerError::from)
    }

    pub fn dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        self.resolver.workspace_dependencies()
    }

    pub fn health(&self) -> Result<RuntimeHealthReport, RuntimeManagerError> {
        let deps = self.dependencies()?;
        let probes = [
            RuntimeToolProbe::new("node", deps.node),
            RuntimeToolProbe::new("npm", deps.npm),
            RuntimeToolProbe::new("npx", deps.npx),
            RuntimeToolProbe::new("python", deps.python),
            RuntimeToolProbe::new("uv", deps.uv),
            RuntimeToolProbe::new("uvx", deps.uvx),
        ];
        self.health_checker
            .check(&probes)
            .map_err(RuntimeManagerError::from)
    }

    pub fn paths(&self) -> &RuntimePaths {
        &self.paths
    }
}
