//! Helpers to make sub-commands invoke our bundled runtime tools (node/npm/npx/uv/uvx)
//! reliably across user environments.
//!
//! Why: scripts like npm/npx have a `#!/usr/bin/env node` shebang, which makes
//! the kernel search PATH for `node`. Without prepending our bundle's bin dir
//! to PATH, the kernel either uses the user's system node (wrong version) or
//! fails to find one entirely.

use std::path::Path;
use std::process::Command;

/// Prepend the bundle bin directory containing `tool_path` to the child's PATH.
/// On macOS/Linux the bundle layout puts `node`, `npm`, `npx` together in
/// `<install_dir>/node/bin/`; on Windows they live in `<install_dir>/node/`.
/// Either way, prepending the parent of the executable is sufficient because
/// any sibling tool the script may spawn lives in the same directory.
pub fn prepend_bundle_bin_to_path(command: &mut Command, tool_path: &Path) {
    let Some(bin_dir) = tool_path.parent() else {
        return;
    };
    let new_path = match std::env::var_os("PATH") {
        Some(existing) => {
            let mut paths: Vec<std::path::PathBuf> =
                std::env::split_paths(&existing).collect();
            // de-dup if our dir is already in the list (don't grow PATH unbounded over time)
            paths.retain(|p| p != bin_dir);
            paths.insert(0, bin_dir.to_path_buf());
            std::env::join_paths(paths)
                .unwrap_or_else(|_| std::ffi::OsString::from(bin_dir))
        }
        None => std::ffi::OsString::from(bin_dir),
    };
    log::debug!(
        "[command_env] prepending bundle bin to PATH: bin_dir={} for tool={}",
        bin_dir.display(),
        tool_path.display(),
    );
    command.env("PATH", new_path);
}

/// Prepend the bundle bin directory containing `tool_path` to a tokio Command's PATH.
/// Same logic as `prepend_bundle_bin_to_path` but operates on `tokio::process::Command`.
pub fn prepend_bundle_bin_to_path_tokio(
    command: &mut tokio::process::Command,
    tool_path: &Path,
) {
    let Some(bin_dir) = tool_path.parent() else {
        return;
    };
    let new_path = match std::env::var_os("PATH") {
        Some(existing) => {
            let mut paths: Vec<std::path::PathBuf> =
                std::env::split_paths(&existing).collect();
            paths.retain(|p| p != bin_dir);
            paths.insert(0, bin_dir.to_path_buf());
            std::env::join_paths(paths)
                .unwrap_or_else(|_| std::ffi::OsString::from(bin_dir))
        }
        None => std::ffi::OsString::from(bin_dir),
    };
    log::debug!(
        "[command_env] prepending bundle bin to PATH (tokio): bin_dir={} for tool={}",
        bin_dir.display(),
        tool_path.display(),
    );
    command.env("PATH", new_path);
}
