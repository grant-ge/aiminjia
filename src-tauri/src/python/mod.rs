pub mod analysis_utils;
pub mod parser;
pub mod runner;
pub mod sandbox;
pub mod session;

use std::path::Path;
use tokio::process::Command;

/// Configure shared Python environment variables on a Command.
///
/// Sets UTF-8 encoding, unbuffered output, and optional PYTHONHOME isolation.
/// Also strips known sensitive environment variables so that user-supplied
/// Python code cannot read API keys from the parent process environment.
/// Used by both `PythonRunner` (one-shot) and `PythonSession` (persistent REPL)
/// to ensure identical Python process configuration.
pub(crate) fn configure_python_env(cmd: &mut Command, python_home: Option<&Path>) {
    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONLEGACYWINDOWSSTDIO", "0")
        .env("PYTHONUTF8", "1")
        .kill_on_drop(true);

    if let Some(home) = python_home {
        cmd.env("PYTHONHOME", home);
        cmd.env_remove("PYTHONPATH");
    }

    // Strip sensitive environment variables so user Python code cannot read
    // API keys or credentials from the parent process environment.
    // Use env_remove (not env_clear) to preserve variables Python needs to run.
    let sensitive_vars = [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "TAVILY_API_KEY",
        "BOCHA_API_KEY",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_ACCESS_KEY_ID",
    ];
    for var in &sensitive_vars {
        cmd.env_remove(var);
    }
}
