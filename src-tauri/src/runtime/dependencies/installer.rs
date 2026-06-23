use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::{
    validate_archive_entry_path, verify_sha256, RuntimeHealthChecker, RuntimeLayout, RuntimePaths,
    RuntimePlatform, RuntimePlatformError, RuntimeToolProbe, WorkspaceDependencies,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInstallPlan {
    pub bundle_version: String,
    pub force: bool,
}

impl RuntimeInstallPlan {
    pub fn already_local(bundle_version: impl Into<String>) -> Self {
        Self {
            bundle_version: bundle_version.into(),
            force: false,
        }
    }

    pub fn reinstall(bundle_version: impl Into<String>) -> Self {
        Self {
            bundle_version: bundle_version.into(),
            force: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInstallResult {
    pub bundle_version: String,
    pub install_dir: PathBuf,
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCleanupResult {
    pub removed_versions: Vec<String>,
    pub kept_versions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInstallError {
    Io(String),
    InvalidPath(String),
    MissingPayload(String),
    SmokeTest(String),
}

impl std::fmt::Display for RuntimeInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "runtime install io error: {error}"),
            Self::InvalidPath(path) => write!(f, "runtime install path is invalid: {path}"),
            Self::MissingPayload(path) => write!(f, "runtime install payload is missing: {path}"),
            Self::SmokeTest(error) => write!(f, "runtime install smoke test failed: {error}"),
        }
    }
}

impl std::error::Error for RuntimeInstallError {}

#[derive(Debug, Clone)]
pub struct RuntimeInstaller {
    paths: RuntimePaths,
    platform_override: Option<RuntimePlatform>,
}

impl RuntimeInstaller {
    pub fn new(paths: RuntimePaths) -> Self {
        Self {
            paths,
            platform_override: None,
        }
    }

    pub fn new_for_platform(paths: RuntimePaths, platform: RuntimePlatform) -> Self {
        Self {
            paths,
            platform_override: Some(platform),
        }
    }

    fn current_layout(&self) -> Result<RuntimeLayout, RuntimeInstallError> {
        match self.platform_override {
            Some(platform) => Ok(RuntimeLayout::for_platform(platform)),
            None => RuntimeLayout::current().map_err(runtime_platform_error),
        }
    }

    fn should_smoke_test_runtime_payload(&self) -> bool {
        let Ok(current) = RuntimePlatform::current() else {
            return false;
        };
        let target = self.platform_override.unwrap_or(current);
        matches!(
            (target, current),
            (RuntimePlatform::DarwinArm64, RuntimePlatform::DarwinArm64)
                | (RuntimePlatform::DarwinX64, RuntimePlatform::DarwinX64)
                | (RuntimePlatform::LinuxX64, RuntimePlatform::LinuxX64)
                | (RuntimePlatform::WindowsX64, RuntimePlatform::WindowsX64)
        )
    }

    pub fn ensure(
        &self,
        plan: RuntimeInstallPlan,
    ) -> Result<RuntimeInstallResult, RuntimeInstallError> {
        let bundle_version = plan.bundle_version;
        let force = plan.force;
        let version_dir = self
            .paths
            .version_dir(&bundle_version)
            .map_err(|_| RuntimeInstallError::InvalidPath(bundle_version.clone()))?;
        let staging_dir = self.safe_staging_dir(&bundle_version)?;
        let bundle_root = self.paths.bundle_root();
        let current_path = self.paths.current_dir();

        self.assert_within_bundle_root(&version_dir)?;
        self.assert_within_bundle_root(&staging_dir)?;
        self.assert_within_bundle_root(&current_path)?;

        if !force && self.is_already_local(&bundle_version, &version_dir, &current_path)? {
            self.validate_runtime_payload(&version_dir)?;
            return Ok(RuntimeInstallResult {
                bundle_version,
                install_dir: version_dir,
                skipped: true,
            });
        }

        fs::create_dir_all(&bundle_root).map_err(io_error)?;
        fs::create_dir_all(self.paths.downloads_dir()).map_err(io_error)?;
        fs::create_dir_all(self.paths.staging_dir()).map_err(io_error)?;
        fs::create_dir_all(self.paths.versions_dir()).map_err(io_error)?;

        if staging_dir.exists() {
            if staging_dir.is_dir() {
                fs::remove_dir_all(&staging_dir).map_err(io_error)?;
            } else {
                return Err(RuntimeInstallError::Io(format!(
                    "staging path is not a directory: {}",
                    staging_dir.display()
                )));
            }
        }

        if version_dir.exists() {
            if !version_dir.is_dir() {
                return Err(RuntimeInstallError::Io(format!(
                    "version path is not a directory: {}",
                    version_dir.display()
                )));
            }

            if !force {
                self.validate_runtime_payload(&version_dir)?;
                self.write_install_manifest(&version_dir, &bundle_version)?;
                self.write_current_pointer(&current_path, &bundle_version)?;

                return Ok(RuntimeInstallResult {
                    bundle_version,
                    install_dir: version_dir,
                    skipped: false,
                });
            }
        }

        fs::create_dir_all(&staging_dir).map_err(io_error)?;
        self.write_install_manifest(&staging_dir, &bundle_version)?;
        self.create_dev_payload(&staging_dir).map_err(io_error)?;
        self.validate_runtime_payload(&staging_dir)?;
        // The already-local path creates managed-runtime stubs for development
        // and tests. Real downloaded archives are still smoke-tested before
        // promotion in `install_from_local_archive`.

        let replaced_backup = self.replace_staging_with_version_dir(&staging_dir, &version_dir)?;
        if let Err(error) = self.write_current_pointer(&current_path, &bundle_version) {
            let _ = fs::remove_dir_all(&version_dir);
            if let Some(backup) = replaced_backup {
                let _ = fs::rename(&backup, &version_dir);
            }
            return Err(error);
        }
        if let Some(backup) = replaced_backup {
            let _ = fs::remove_dir_all(backup);
        }

        Ok(RuntimeInstallResult {
            bundle_version,
            install_dir: version_dir,
            skipped: false,
        })
    }

    pub fn reinstall(
        &self,
        plan: RuntimeInstallPlan,
    ) -> Result<RuntimeInstallResult, RuntimeInstallError> {
        self.ensure(RuntimeInstallPlan {
            bundle_version: plan.bundle_version,
            force: true,
        })
    }

    pub fn cleanup_old_versions(
        &self,
        keep_versions: usize,
    ) -> Result<RuntimeCleanupResult, RuntimeInstallError> {
        let versions_dir = self.paths.versions_dir();
        if !versions_dir.exists() {
            return Ok(RuntimeCleanupResult {
                removed_versions: Vec::new(),
                kept_versions: Vec::new(),
            });
        }
        self.assert_within_bundle_root(&versions_dir)?;

        let current_version = self.current_version_name()?;
        let mut versions = Vec::new();
        for entry in fs::read_dir(&versions_dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let file_type = entry.file_type().map_err(io_error)?;
            if !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if self.paths.version_dir(&name).is_err() {
                continue;
            }
            versions.push(name);
        }
        versions.sort();
        versions.reverse();

        let mut kept_versions = Vec::new();
        let mut removed_versions = Vec::new();
        for version in versions {
            let should_keep = Some(version.as_str()) == current_version.as_deref()
                || kept_versions.len() < keep_versions;
            if should_keep {
                kept_versions.push(version);
                continue;
            }
            let version_dir = self
                .paths
                .version_dir(&version)
                .map_err(|_| RuntimeInstallError::InvalidPath(version.clone()))?;
            self.assert_within_bundle_root(&version_dir)?;
            fs::remove_dir_all(&version_dir).map_err(io_error)?;
            removed_versions.push(version);
        }
        kept_versions.sort();
        removed_versions.sort();
        Ok(RuntimeCleanupResult {
            removed_versions,
            kept_versions,
        })
    }

    fn current_version_name(&self) -> Result<Option<String>, RuntimeInstallError> {
        let current_path = self.paths.current_dir();
        let content = match fs::read_to_string(&current_path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        let pointer = content.trim();
        let Some(version) = pointer.strip_prefix("versions/") else {
            return Ok(None);
        };
        if version.contains('/')
            || version.contains('\\')
            || version.contains("..")
            || version.is_empty()
        {
            return Ok(None);
        }
        Ok(Some(version.to_string()))
    }

    pub fn install_from_verified_archive(
        &self,
        plan: RuntimeInstallPlan,
        archive_path: &Path,
        expected_sha256: &str,
    ) -> Result<RuntimeInstallResult, RuntimeInstallError> {
        verify_sha256(archive_path, expected_sha256)
            .map_err(|error| RuntimeInstallError::Io(error.to_string()))?;
        self.install_from_local_archive(plan, archive_path)
    }

    pub fn install_from_local_archive(
        &self,
        plan: RuntimeInstallPlan,
        archive_path: &Path,
    ) -> Result<RuntimeInstallResult, RuntimeInstallError> {
        let RuntimeInstallPlan {
            bundle_version,
            force,
        } = plan;
        let version_dir = self
            .paths
            .version_dir(&bundle_version)
            .map_err(|_| RuntimeInstallError::InvalidPath(bundle_version.clone()))?;
        let staging_dir = self.safe_staging_dir(&bundle_version)?;
        let bundle_root = self.paths.bundle_root();
        let current_path = self.paths.current_dir();

        self.assert_within_bundle_root(&version_dir)?;
        self.assert_within_bundle_root(&staging_dir)?;
        self.assert_within_bundle_root(&current_path)?;

        if !force && self.is_already_local(&bundle_version, &version_dir, &current_path)? {
            self.validate_runtime_payload(&version_dir)?;
            return Ok(RuntimeInstallResult {
                bundle_version,
                install_dir: version_dir,
                skipped: true,
            });
        }

        fs::create_dir_all(&bundle_root).map_err(io_error)?;
        fs::create_dir_all(self.paths.downloads_dir()).map_err(io_error)?;
        fs::create_dir_all(self.paths.staging_dir()).map_err(io_error)?;
        fs::create_dir_all(self.paths.versions_dir()).map_err(io_error)?;

        if staging_dir.exists() {
            if staging_dir.is_dir() {
                fs::remove_dir_all(&staging_dir).map_err(io_error)?;
            } else {
                return Err(RuntimeInstallError::Io(format!(
                    "staging path is not a directory: {}",
                    staging_dir.display()
                )));
            }
        }
        fs::create_dir_all(&staging_dir).map_err(io_error)?;

        if let Err(error) = self.extract_archive(archive_path, &staging_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if let Err(error) = self.write_install_manifest(&staging_dir, &bundle_version) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if let Err(error) = self.ensure_compatibility_directories(&staging_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if let Err(error) = self.validate_runtime_payload(&staging_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if self.should_smoke_test_runtime_payload() {
            if let Err(error) = self.smoke_test_runtime_payload(&staging_dir) {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(error);
            }
        }

        let replaced_backup = self.replace_staging_with_version_dir(&staging_dir, &version_dir)?;
        if let Err(error) = self.write_current_pointer(&current_path, &bundle_version) {
            let _ = fs::remove_dir_all(&version_dir);
            if let Some(backup) = replaced_backup {
                let _ = fs::rename(&backup, &version_dir);
            }
            return Err(error);
        }
        if let Some(backup) = replaced_backup {
            let _ = fs::remove_dir_all(backup);
        }

        Ok(RuntimeInstallResult {
            bundle_version,
            install_dir: version_dir,
            skipped: false,
        })
    }

    pub fn install_from_directory(
        &self,
        plan: RuntimeInstallPlan,
        source_dir: &Path,
    ) -> Result<RuntimeInstallResult, RuntimeInstallError> {
        let RuntimeInstallPlan {
            bundle_version,
            force,
        } = plan;
        if !source_dir.is_dir() {
            return Err(RuntimeInstallError::MissingPayload(
                source_dir.display().to_string(),
            ));
        }

        let version_dir = self
            .paths
            .version_dir(&bundle_version)
            .map_err(|_| RuntimeInstallError::InvalidPath(bundle_version.clone()))?;
        let staging_dir = self.safe_staging_dir(&bundle_version)?;
        let bundle_root = self.paths.bundle_root();
        let current_path = self.paths.current_dir();

        self.assert_within_bundle_root(&version_dir)?;
        self.assert_within_bundle_root(&staging_dir)?;
        self.assert_within_bundle_root(&current_path)?;

        if !force && self.is_already_local(&bundle_version, &version_dir, &current_path)? {
            self.validate_runtime_payload(&version_dir)?;
            return Ok(RuntimeInstallResult {
                bundle_version,
                install_dir: version_dir,
                skipped: true,
            });
        }

        fs::create_dir_all(&bundle_root).map_err(io_error)?;
        fs::create_dir_all(self.paths.downloads_dir()).map_err(io_error)?;
        fs::create_dir_all(self.paths.staging_dir()).map_err(io_error)?;
        fs::create_dir_all(self.paths.versions_dir()).map_err(io_error)?;

        if staging_dir.exists() {
            if staging_dir.is_dir() {
                fs::remove_dir_all(&staging_dir).map_err(io_error)?;
            } else {
                return Err(RuntimeInstallError::Io(format!(
                    "staging path is not a directory: {}",
                    staging_dir.display()
                )));
            }
        }
        fs::create_dir_all(&staging_dir).map_err(io_error)?;

        if let Err(error) = self.copy_runtime_directory(source_dir, &staging_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if let Err(error) = self.write_install_manifest(&staging_dir, &bundle_version) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if let Err(error) = self.ensure_compatibility_directories(&staging_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if let Err(error) = self.validate_runtime_payload(&staging_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
        if self.should_smoke_test_runtime_payload() {
            if let Err(error) = self.smoke_test_runtime_payload(&staging_dir) {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(error);
            }
        }

        let replaced_backup = self.replace_staging_with_version_dir(&staging_dir, &version_dir)?;
        if let Err(error) = self.write_current_pointer(&current_path, &bundle_version) {
            let _ = fs::remove_dir_all(&version_dir);
            if let Some(backup) = replaced_backup {
                let _ = fs::rename(&backup, &version_dir);
            }
            return Err(error);
        }
        if let Some(backup) = replaced_backup {
            let _ = fs::remove_dir_all(backup);
        }

        Ok(RuntimeInstallResult {
            bundle_version,
            install_dir: version_dir,
            skipped: false,
        })
    }

    fn safe_staging_dir(&self, bundle_version: &str) -> Result<PathBuf, RuntimeInstallError> {
        let version_dir = self
            .paths
            .version_dir(bundle_version)
            .map_err(|_| RuntimeInstallError::InvalidPath(bundle_version.to_string()))?;
        let version_name = version_dir
            .file_name()
            .ok_or_else(|| RuntimeInstallError::InvalidPath(bundle_version.to_string()))?;
        Ok(self.paths.staging_dir().join(version_name))
    }

    fn extract_archive(&self, archive_path: &Path, dest: &Path) -> Result<(), RuntimeInstallError> {
        let file_name = archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
            self.extract_tar_gz_archive(archive_path, dest)
        } else {
            self.extract_zip_archive(archive_path, dest)
        }
    }

    fn extract_tar_gz_archive(
        &self,
        archive_path: &Path,
        dest: &Path,
    ) -> Result<(), RuntimeInstallError> {
        let file = fs::File::open(archive_path).map_err(io_error)?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        let entries = archive
            .entries()
            .map_err(|error| RuntimeInstallError::Io(error.to_string()))?;

        for entry in entries {
            let mut entry = entry.map_err(|error| RuntimeInstallError::Io(error.to_string()))?;
            let entry_path = entry
                .path()
                .map_err(|error| RuntimeInstallError::Io(error.to_string()))?;
            let entry_name = entry_path.to_string_lossy().into_owned();
            let out_path = validate_archive_entry_path(dest, &entry_name)
                .map_err(|error| RuntimeInstallError::InvalidPath(error.to_string()))?;
            self.assert_within_path(dest, &out_path)?;

            let entry_type = entry.header().entry_type();
            if entry_type.is_dir() {
                fs::create_dir_all(&out_path).map_err(io_error)?;
                continue;
            }
            if entry_type.is_symlink() {
                let link_target = entry
                    .link_name()
                    .map_err(|error| RuntimeInstallError::Io(error.to_string()))?
                    .ok_or_else(|| {
                        RuntimeInstallError::InvalidPath(format!(
                            "archive symlink has no target: {entry_name}"
                        ))
                    })?;
                self.create_safe_symlink(dest, &out_path, &link_target)?;
                continue;
            }
            if !entry_type.is_file() {
                continue;
            }
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(io_error)?;
            }
            entry
                .unpack(&out_path)
                .map_err(|error| RuntimeInstallError::Io(error.to_string()))?;
        }

        Ok(())
    }

    fn extract_zip_archive(
        &self,
        archive_path: &Path,
        dest: &Path,
    ) -> Result<(), RuntimeInstallError> {
        let file = fs::File::open(archive_path).map_err(io_error)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|error| RuntimeInstallError::Io(error.to_string()))?;

        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| RuntimeInstallError::Io(error.to_string()))?;
            let out_path = validate_archive_entry_path(dest, entry.name())
                .map_err(|error| RuntimeInstallError::InvalidPath(error.to_string()))?;
            self.assert_within_path(dest, &out_path)?;

            if entry.is_dir() {
                fs::create_dir_all(&out_path).map_err(io_error)?;
                continue;
            }

            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(io_error)?;
            }
            let mut output = fs::File::create(&out_path).map_err(io_error)?;
            std::io::copy(&mut entry, &mut output).map_err(io_error)?;

            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&out_path, fs::Permissions::from_mode(mode))
                    .map_err(io_error)?;
            }
        }

        Ok(())
    }

    fn create_safe_symlink(
        &self,
        dest: &Path,
        out_path: &Path,
        link_target: &Path,
    ) -> Result<(), RuntimeInstallError> {
        if link_target.is_absolute() {
            return Err(RuntimeInstallError::InvalidPath(format!(
                "archive symlink target is absolute: {} -> {}",
                out_path.display(),
                link_target.display()
            )));
        }
        let parent = out_path
            .parent()
            .ok_or_else(|| RuntimeInstallError::InvalidPath(out_path.display().to_string()))?;
        let resolved = normalize_relative_path(parent, link_target).ok_or_else(|| {
            RuntimeInstallError::InvalidPath(format!(
                "archive symlink target is unsafe: {} -> {}",
                out_path.display(),
                link_target.display()
            ))
        })?;
        self.assert_within_path(dest, &resolved)?;
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        create_symlink(link_target, out_path).map_err(io_error)
    }

    fn replace_staging_with_version_dir(
        &self,
        staging_dir: &Path,
        version_dir: &Path,
    ) -> Result<Option<PathBuf>, RuntimeInstallError> {
        if !version_dir.exists() {
            fs::rename(staging_dir, version_dir).map_err(io_error)?;
            return Ok(None);
        }

        let version_name = version_dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| RuntimeInstallError::InvalidPath(version_dir.display().to_string()))?;
        let backup_dir = self
            .paths
            .staging_dir()
            .join(format!("{version_name}.previous"));
        self.assert_within_bundle_root(&backup_dir)?;
        if backup_dir.exists() {
            fs::remove_dir_all(&backup_dir).map_err(io_error)?;
        }
        fs::rename(version_dir, &backup_dir).map_err(io_error)?;
        if let Err(error) = fs::rename(staging_dir, version_dir) {
            let _ = fs::rename(&backup_dir, version_dir);
            return Err(io_error(error));
        }
        Ok(Some(backup_dir))
    }

    fn is_already_local(
        &self,
        bundle_version: &str,
        version_dir: &Path,
        current_path: &Path,
    ) -> Result<bool, RuntimeInstallError> {
        let current_target = match fs::read_to_string(current_path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error(error)),
        };

        let expected_target = format!("versions/{bundle_version}");
        if current_target.trim() != expected_target {
            return Ok(false);
        }

        let install_manifest = version_dir.join("install.json");
        let content = match fs::read_to_string(&install_manifest) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(io_error(error)),
        };

        let bundle_version_matches = serde_json::from_str::<serde_json::Value>(&content)
            .ok()
            .and_then(|value| {
                value
                    .get("bundleVersion")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| value == bundle_version)
            })
            .unwrap_or(false);

        Ok(bundle_version_matches)
    }

    fn write_install_manifest(
        &self,
        install_dir: &Path,
        bundle_version: &str,
    ) -> Result<(), RuntimeInstallError> {
        let install_manifest_path = install_dir.join("install.json");
        self.assert_within_bundle_root(&install_manifest_path)?;
        let layout = self.current_layout()?;
        let bytes = serde_json::to_vec_pretty(&json!({
            "bundleVersion": bundle_version,
            "platform": layout.platform().manifest_key(),
            "source": {
                "kind": "managed-runtime",
            },
            "runtimes": {
                "node": {
                    "version": "unknown",
                    "path": "node",
                    "binPaths": {
                        "node": layout.node(),
                        "npm": layout.npm(),
                        "npx": layout.npx()
                    }
                },
                "python": {
                    "version": "unknown",
                    "path": "python",
                    "binPaths": {
                        "python": layout.python()
                    }
                },
                "uv": {
                    "version": "unknown",
                    "path": "uv",
                    "binPaths": {
                        "uv": layout.uv(),
                        "uvx": layout.uvx()
                    }
                }
            },
            "paths": {
                "node": layout.node(),
                "npm": layout.npm(),
                "npx": layout.npx(),
                "python": layout.python(),
                "uv": layout.uv(),
                "uvx": layout.uvx(),
                "nodeModules": layout.node_modules(),
                "pythonSitePackages": layout.python_site_packages()
            }
        }))
        .map_err(|error| RuntimeInstallError::Io(error.to_string()))?;
        fs::write(install_manifest_path, bytes).map_err(io_error)
    }

    fn create_dev_payload(&self, install_dir: &Path) -> std::io::Result<()> {
        let layout = self.current_layout().map_err(std::io::Error::other)?;
        for relative in layout.executable_paths() {
            let path = install_dir.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            if !path.exists() {
                let tool_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("runtime");
                fs::write(
                    &path,
                    format!(
                        "#!/usr/bin/env sh
echo {tool_name} managed-runtime-stub
"
                    ),
                )?;
                make_executable(&path)?;
            }
        }
        for relative in layout.directory_paths() {
            fs::create_dir_all(install_dir.join(relative))?;
        }
        Ok(())
    }

    fn ensure_compatibility_directories(
        &self,
        install_dir: &Path,
    ) -> Result<(), RuntimeInstallError> {
        let layout = self.current_layout()?;
        for relative in layout.directory_paths() {
            fs::create_dir_all(install_dir.join(relative)).map_err(io_error)?;
        }
        Ok(())
    }

    fn copy_runtime_directory(
        &self,
        source: &Path,
        dest: &Path,
    ) -> Result<(), RuntimeInstallError> {
        self.copy_runtime_directory_into(source, dest, dest)
    }

    fn copy_runtime_directory_into(
        &self,
        source: &Path,
        dest: &Path,
        root: &Path,
    ) -> Result<(), RuntimeInstallError> {
        for entry in fs::read_dir(source).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let source_path = entry.path();
            let dest_path = dest.join(entry.file_name());
            let file_type = entry.file_type().map_err(io_error)?;

            if file_type.is_dir() {
                fs::create_dir_all(&dest_path).map_err(io_error)?;
                self.copy_runtime_directory_into(&source_path, &dest_path, root)?;
                continue;
            }

            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).map_err(io_error)?;
            }

            if file_type.is_symlink() {
                let target = fs::read_link(&source_path).map_err(io_error)?;
                self.create_safe_symlink(root, &dest_path, &target)?;
                continue;
            }

            if file_type.is_file() {
                fs::copy(&source_path, &dest_path).map_err(io_error)?;
                let permissions = fs::metadata(&source_path).map_err(io_error)?.permissions();
                fs::set_permissions(&dest_path, permissions).map_err(io_error)?;
            }
        }
        Ok(())
    }

    fn validate_runtime_payload(&self, install_dir: &Path) -> Result<(), RuntimeInstallError> {
        let layout = self.current_layout()?;
        for relative in layout.executable_paths() {
            let path = install_dir.join(relative);
            if !path.is_file() {
                return Err(RuntimeInstallError::MissingPayload(
                    path.display().to_string(),
                ));
            }
        }
        Ok(())
    }

    fn smoke_test_runtime_payload(&self, install_dir: &Path) -> Result<(), RuntimeInstallError> {
        let deps = WorkspaceDependencies::from_install_dir(install_dir)
            .map_err(|error| RuntimeInstallError::SmokeTest(error.to_string()))?;
        RuntimeHealthChecker::default()
            .check(&[
                RuntimeToolProbe::new("node", deps.node),
                RuntimeToolProbe::new("npm", deps.npm),
                RuntimeToolProbe::new("npx", deps.npx),
                RuntimeToolProbe::new("python", deps.python),
                RuntimeToolProbe::new("uv", deps.uv),
                RuntimeToolProbe::new("uvx", deps.uvx),
            ])
            .map_err(|error| RuntimeInstallError::SmokeTest(error.to_string()))?;
        Ok(())
    }

    fn write_current_pointer(
        &self,
        current_path: &Path,
        bundle_version: &str,
    ) -> Result<(), RuntimeInstallError> {
        self.assert_within_bundle_root(current_path)?;
        if let Some(parent) = current_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }

        if current_path.is_dir() {
            return Err(RuntimeInstallError::Io(format!(
                "current pointer path must be a file, got directory: {}",
                current_path.display()
            )));
        }

        let temp_path = current_path.with_extension("tmp");
        self.assert_within_bundle_root(&temp_path)?;
        fs::write(&temp_path, format!("versions/{bundle_version}")).map_err(io_error)?;
        replace_file(&temp_path, current_path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            io_error(error)
        })
    }

    fn assert_within_path(&self, root: &Path, path: &Path) -> Result<(), RuntimeInstallError> {
        if path.starts_with(root) {
            Ok(())
        } else {
            Err(RuntimeInstallError::InvalidPath(path.display().to_string()))
        }
    }

    fn assert_within_bundle_root(&self, path: &Path) -> Result<(), RuntimeInstallError> {
        if path.starts_with(self.paths.bundle_root()) {
            Ok(())
        } else {
            Err(RuntimeInstallError::InvalidPath(path.display().to_string()))
        }
    }
}

fn runtime_platform_error(error: RuntimePlatformError) -> RuntimeInstallError {
    RuntimeInstallError::Io(error.to_string())
}

fn io_error(error: std::io::Error) -> RuntimeInstallError {
    RuntimeInstallError::Io(error.to_string())
}

fn normalize_relative_path(base: &Path, relative: &Path) -> Option<PathBuf> {
    let mut normalized = base.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(segment) => normalized.push(segment),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    // Windows dev/test environments may not have symlink privileges; copying is
    // sufficient for runtime bin aliases after the target file has been extracted.
    let source = link
        .parent()
        .map(|parent| parent.join(target))
        .unwrap_or_else(|| target.to_path_buf());
    fs::copy(source, link).map(|_| ())
}

#[cfg(unix)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let source = wide(source);
    let destination = wide(destination);
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(source, destination)
}
