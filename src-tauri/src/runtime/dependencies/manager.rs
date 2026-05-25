use std::path::Path;
use std::sync::{Arc, Mutex};

use super::{
    ChainResolver, InstalledRuntimeResolver, RuntimeArtifactFetchError, RuntimeArtifactFetcher,
    RuntimeDependencyResult, RuntimeDownloadCancellation, RuntimeDownloadOptions,
    RuntimeHealthChecker, RuntimeHealthError, RuntimeHealthReport, RuntimeInstallError,
    RuntimeInstallPlan, RuntimeInstallResult, RuntimeInstaller, RuntimeManifestSource,
    RuntimePaths, RuntimePlatform, RuntimeResolver, RuntimeToolProbe, WorkspaceDependencies,
};

pub type ManagedRuntimeManager = Arc<RuntimeManager>;

#[derive(Clone)]
pub struct RuntimeManager {
    paths: RuntimePaths,
    bundle_version: String,
    installer: RuntimeInstaller,
    /// Effective resolver: starts as `installed_resolver`; `with_primary_resolver`
    /// wraps it in a `ChainResolver(primary, installed_resolver)`.
    resolver: Arc<dyn RuntimeResolver>,
    /// Kept around so the OSS install path keeps a stable handle to the
    /// on-disk-installed runtime even after a primary resolver is chained on top.
    installed_resolver: InstalledRuntimeResolver,
    health_checker: RuntimeHealthChecker,
    manifest_install: Option<RuntimeManifestInstallConfig>,
    active_operation: Arc<Mutex<Option<RuntimeActiveOperation>>>,
}

impl std::fmt::Debug for RuntimeManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeManager")
            .field("paths", &self.paths)
            .field("bundle_version", &self.bundle_version)
            .field("installed_resolver", &self.installed_resolver)
            .field("has_primary_chain", &true)
            .finish_non_exhaustive()
    }
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
        // Lazy ensure only when a manifest source is configured: in that case
        // the user has explicitly opted into managed runtime, so a missing
        // install is a recoverable "first run on this machine" state rather
        // than a hard error. Without a manifest we keep the original resolver
        // error to avoid the historical pitfalls of unconditional lazy install
        // on the chat hot path: blocking the tokio worker for tens of seconds,
        // retrying a permanently failing install on every call, and freezing
        // the UI without feedback. ensure() failures here are wrapped as
        // ResolverUnavailable so callers terminate instead of looping.
        match self.resolver.workspace_dependencies() {
            Ok(dependencies) => Ok(dependencies),
            Err(_) if self.has_manifest_source() => {
                self.ensure().map_err(|ensure_error| {
                    super::RuntimeDependencyError::ResolverUnavailable(ensure_error.to_string())
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
        let installed = InstalledRuntimeResolver::new(bundle_root);
        Self {
            installer: RuntimeInstaller::new(paths.clone()),
            resolver: Arc::new(installed.clone()),
            installed_resolver: installed,
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

    /// Chain a primary resolver in front of the default installed resolver.
    /// The chain tries `primary` first; only on miss does it fall back to the
    /// on-disk installed runtime. Useful for installer-bundled runtimes where
    /// `primary` reads from the app's resource_dir and the installed path
    /// is just an upgrade channel.
    pub fn with_primary_resolver(mut self, primary: Arc<dyn RuntimeResolver>) -> Self {
        let chain = ChainResolver::new(vec![primary, Arc::new(self.installed_resolver.clone())]);
        self.resolver = Arc::new(chain);
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
        self.installed_resolver.clone()
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
            RuntimeManifestSource::Url(url) => {
                log::info!(
                    "[runtime-manager] install_from_configured_manifest_blocking entering for runtime={} platform={:?}",
                    config.runtime_name, config.platform
                );
                // We may be called from inside a tokio multi-thread runtime
                // (e.g. when ensure() is invoked lazily during chat handling).
                // Calling tauri::async_runtime::block_on() in that context panics
                // with "Cannot start a runtime from within a runtime". Use
                // block_in_place + Handle.block_on when a runtime already exists,
                // and fall back to a fresh current-thread runtime when called
                // from sync init code with no runtime context.
                //
                // NOTE: block_in_place only works on multi-thread runtimes.
                // tauri::async_runtime uses multi-thread by default. If the runtime
                // is ever reconfigured to current-thread this will panic — revisit.
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => {
                        log::info!(
                            "[runtime-manager] using block_in_place + existing tokio handle"
                        );
                        tokio::task::block_in_place(|| {
                            handle.block_on(self.install_from_manifest_url(
                                url,
                                &config.runtime_name,
                                config.platform,
                            ))
                        })
                    }
                    Err(_) => {
                        log::info!("[runtime-manager] no current tokio runtime, building current-thread runtime");
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("failed to build temporary tokio runtime");
                        rt.block_on(self.install_from_manifest_url(
                            url,
                            &config.runtime_name,
                            config.platform,
                        ))
                    }
                }
            }
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
