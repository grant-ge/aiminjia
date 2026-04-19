pub mod analysis_utils;
pub mod runner;
pub mod parser;
pub mod sandbox;
pub mod session;

use std::path::Path;
use tokio::process::Command;

/// Configure shared Python environment variables on a Command.
///
/// Sets UTF-8 encoding, unbuffered output, and optional PYTHONHOME isolation.
/// Used by both `PythonRunner` (one-shot) and `PythonSession` (persistent REPL)
/// to ensure identical Python process configuration.
pub(crate) fn configure_python_env(cmd: &mut Command, python_home: Option<&Path>) {
    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONLEGACYWINDOWSSTDIO", "0")
        .env("PYTHONUTF8", "1")
        .kill_on_drop(true);

    // Prevent CMD window flash on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    if let Some(home) = python_home {
        cmd.env("PYTHONHOME", home);
        cmd.env_remove("PYTHONPATH");
    }
}
