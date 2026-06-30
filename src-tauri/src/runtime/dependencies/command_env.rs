//! Helpers to make sub-commands invoke our managed runtime tools (node/npm/npx/uv/uvx)
//! reliably across user environments.
//!
//! Why: scripts like npm/npx have a `#!/usr/bin/env node` shebang, which makes
//! the kernel search PATH for `node`. Without prepending our managed runtime's
//! bin dir to PATH, the kernel either uses the user's system node (wrong
//! version) or fails to find one entirely.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{RuntimeDependencyResult, RuntimeResolver, WorkspaceDependencies};

#[derive(Debug)]
pub struct ManagedRuntimePreference {
    enabled: AtomicBool,
}

impl ManagedRuntimePreference {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}

impl Default for ManagedRuntimePreference {
    fn default() -> Self {
        Self::new(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRuntimeProcessEnv {
    vars: Vec<(OsString, OsString)>,
}

impl ManagedRuntimeProcessEnv {
    pub fn from_resolver(
        resolver: &dyn RuntimeResolver,
    ) -> RuntimeDependencyResult<ManagedRuntimeProcessEnv> {
        let deps = resolver.workspace_dependencies()?;
        Ok(Self::from_dependencies(&deps))
    }

    pub fn from_dependencies(deps: &WorkspaceDependencies) -> ManagedRuntimeProcessEnv {
        Self::from_dependencies_with_existing_path(deps, std::env::var_os("PATH"))
    }

    pub fn from_dependencies_with_existing_path(
        deps: &WorkspaceDependencies,
        existing_path: Option<OsString>,
    ) -> ManagedRuntimeProcessEnv {
        let node_prefix = node_prefix_from_modules_dir(&deps.node_modules);
        let npm_cache = node_prefix.join(".npm-cache");
        let path = managed_path_for_dependencies(deps, existing_path);

        ManagedRuntimeProcessEnv {
            vars: vec![
                (OsString::from("PATH"), path),
                (
                    OsString::from("NODE_PATH"),
                    deps.node_modules.as_os_str().to_os_string(),
                ),
                (
                    OsString::from("npm_config_prefix"),
                    node_prefix.as_os_str().to_os_string(),
                ),
                (
                    OsString::from("npm_config_cache"),
                    npm_cache.as_os_str().to_os_string(),
                ),
            ],
        }
    }

    pub fn vars(&self) -> &[(OsString, OsString)] {
        &self.vars
    }

    pub fn get(&self, key: &str) -> Option<&OsStr> {
        self.vars
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_os_str())
    }

    pub fn apply_to_command(&self, command: &mut Command) {
        for (key, value) in &self.vars {
            command.env(key, value);
        }
    }

    pub fn apply_to_tokio_command(&self, command: &mut tokio::process::Command) {
        for (key, value) in &self.vars {
            command.env(key, value);
        }
    }
}

fn managed_path_for_dependencies(
    deps: &WorkspaceDependencies,
    existing_path: Option<OsString>,
) -> OsString {
    let mut paths = Vec::<PathBuf>::new();
    for dir in [&deps.node, &deps.python, &deps.uv]
        .into_iter()
        .filter_map(|path| path.parent().map(Path::to_path_buf))
    {
        if !paths.contains(&dir) {
            paths.push(dir);
        }
    }

    if let Some(existing) = existing_path {
        for path in std::env::split_paths(&existing) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    std::env::join_paths(paths).unwrap_or_else(|_| OsString::new())
}

fn node_prefix_from_modules_dir(node_modules: &Path) -> PathBuf {
    let parent = node_modules.parent().unwrap_or(node_modules);
    if parent
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name == "lib")
    {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

/// Prepend the managed runtime bin directory containing `tool_path` to the child's PATH.
/// On macOS/Linux the runtime layout puts `node`, `npm`, `npx` together in
/// `<install_dir>/node/bin/`; on Windows they live in `<install_dir>/node/`.
/// Either way, prepending the parent of the executable is sufficient because
/// any sibling tool the script may spawn lives in the same directory.
pub fn prepend_bundle_bin_to_path(command: &mut Command, tool_path: &Path) {
    let Some(bin_dir) = tool_path.parent() else {
        return;
    };
    let new_path = match std::env::var_os("PATH") {
        Some(existing) => {
            let mut paths: Vec<std::path::PathBuf> = std::env::split_paths(&existing).collect();
            // de-dup if our dir is already in the list (don't grow PATH unbounded over time)
            paths.retain(|p| p != bin_dir);
            paths.insert(0, bin_dir.to_path_buf());
            std::env::join_paths(paths).unwrap_or_else(|_| std::ffi::OsString::from(bin_dir))
        }
        None => std::ffi::OsString::from(bin_dir),
    };
    log::debug!(
        "[command_env] prepending managed runtime bin to PATH: bin_dir={} for tool={}",
        bin_dir.display(),
        tool_path.display(),
    );
    command.env("PATH", new_path);
}

/// Prepend the managed runtime bin directory containing `tool_path` to a tokio Command's PATH.
/// Same logic as `prepend_bundle_bin_to_path` but operates on `tokio::process::Command`.
pub fn prepend_bundle_bin_to_path_tokio(command: &mut tokio::process::Command, tool_path: &Path) {
    let Some(bin_dir) = tool_path.parent() else {
        return;
    };
    let new_path = match std::env::var_os("PATH") {
        Some(existing) => {
            let mut paths: Vec<std::path::PathBuf> = std::env::split_paths(&existing).collect();
            paths.retain(|p| p != bin_dir);
            paths.insert(0, bin_dir.to_path_buf());
            std::env::join_paths(paths).unwrap_or_else(|_| std::ffi::OsString::from(bin_dir))
        }
        None => std::ffi::OsString::from(bin_dir),
    };
    log::debug!(
        "[command_env] prepending managed runtime bin to PATH (tokio): bin_dir={} for tool={}",
        bin_dir.display(),
        tool_path.display(),
    );
    command.env("PATH", new_path);
}
