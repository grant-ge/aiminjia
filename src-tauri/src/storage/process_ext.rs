//! Windows-specific subprocess helpers.
//!
//! On Windows, every `Command::spawn` for a console-subsystem child (the
//! default) flashes a black `cmd.exe` / `conhost.exe` window unless the
//! parent passes the `CREATE_NO_WINDOW` (0x08000000) creation flag. We do
//! this at every shell-out site (dws CLI, MCP servers, Python runner, git,
//! hooks, where.exe, tasklist, …) — missing one is visible to the user as
//! a black flash mid-conversation.
//!
//! Use the extension traits below instead of remembering the magic constant
//! and the `#[cfg]` guard at every call site.

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Extension trait for `std::process::Command` adding a no-op-on-non-Windows
/// `no_window()` configurator.
pub trait NoWindowExt {
    fn no_window(&mut self) -> &mut Self;
}

impl NoWindowExt for std::process::Command {
    #[cfg(target_os = "windows")]
    fn no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(target_os = "windows"))]
    fn no_window(&mut self) -> &mut Self {
        self
    }
}

impl NoWindowExt for tokio::process::Command {
    #[cfg(target_os = "windows")]
    fn no_window(&mut self) -> &mut Self {
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(target_os = "windows"))]
    fn no_window(&mut self) -> &mut Self {
        self
    }
}
