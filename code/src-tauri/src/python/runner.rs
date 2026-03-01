//! Python code execution — spawn, execute, timeout, collect results.
//!
//! Runs Python code in a subprocess with timeout enforcement.
//! Output is captured from stdout/stderr.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use anyhow::{anyhow, Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::sandbox::SandboxConfig;

/// Result of executing Python code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    /// Standard output from the script.
    pub stdout: String,
    /// Standard error output.
    pub stderr: String,
    /// Process exit code (0 = success).
    pub exit_code: i32,
    /// Execution time in milliseconds.
    pub execution_time_ms: u64,
    /// Whether execution was terminated due to timeout.
    pub timed_out: bool,
}

/// Python code runner with sandbox enforcement.
pub struct PythonRunner {
    workspace_path: PathBuf,
    python_binary: PathBuf,
    python_home: Option<PathBuf>,
    sandbox: SandboxConfig,
}

impl PythonRunner {
    /// Create a new runner for the given workspace.
    ///
    /// If `app_handle` is provided, attempts to locate a bundled Python runtime
    /// inside the Tauri resource directory. Falls back to system `python3`.
    pub fn new(workspace_path: PathBuf, app_handle: Option<&tauri::AppHandle>) -> Self {
        let sandbox = SandboxConfig::for_workspace(&workspace_path);
        let (python_binary, python_home) = resolve_python_path(app_handle);
        Self {
            workspace_path,
            python_binary,
            python_home,
            sandbox,
        }
    }

    /// Create a runner with custom sandbox config.
    pub fn with_config(workspace_path: PathBuf, sandbox: SandboxConfig, app_handle: Option<&tauri::AppHandle>) -> Self {
        let (python_binary, python_home) = resolve_python_path(app_handle);
        Self {
            workspace_path,
            python_binary,
            python_home,
            sandbox,
        }
    }

    /// Execute Python code string.
    ///
    /// 1. Validates code against sandbox rules.
    /// 2. Writes to a temp file (workspace/temp/code_{uuid}.py).
    /// 3. Spawns `python3 -u temp_file.py`.
    /// 4. Enforces timeout.
    /// 5. Captures stdout/stderr.
    /// 6. Cleans up temp file.
    pub async fn execute(&self, code: &str) -> Result<ExecutionResult> {
        // 1. Validate code
        self.sandbox.validate_code(code).map_err(|e| anyhow!("Sandbox violation: {}", e))?;

        // 2. Prepare temp file
        let temp_dir = self.workspace_path.join("temp");
        std::fs::create_dir_all(&temp_dir).context("Failed to create temp directory")?;

        let file_id = uuid::Uuid::new_v4().to_string();
        let temp_file = temp_dir.join(format!("code_{}.py", file_id));

        // Prepend sandbox preamble to user code
        let full_code = format!("{}\n# --- User Code ---\n{}", self.sandbox.preamble(), code);
        std::fs::write(&temp_file, &full_code).context("Failed to write temp Python file")?;

        // 3. Execute
        let result = self.run_python_file(&temp_file).await;

        // 4. Cleanup temp file
        let _ = std::fs::remove_file(&temp_file);

        result
    }

    /// Execute a Python file directly (must already exist).
    pub async fn execute_file(&self, file_path: &Path) -> Result<ExecutionResult> {
        if !file_path.exists() {
            return Err(anyhow!("Python file not found: {}", file_path.display()));
        }
        self.run_python_file(file_path).await
    }

    /// Internal: spawn python and run a file with timeout.
    async fn run_python_file(&self, file_path: &Path) -> Result<ExecutionResult> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(self.sandbox.timeout_seconds as u64);

        let mut cmd = Command::new(&self.python_binary);
        cmd.arg("-u") // unbuffered output
            .arg(file_path)
            .current_dir(&self.workspace_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONIOENCODING", "utf-8")    // Force UTF-8 output on all platforms
            .env("PYTHONLEGACYWINDOWSSTDIO", "0") // Disable legacy Windows stdio
            .kill_on_drop(true);

        // When using bundled Python, set PYTHONHOME and clear PYTHONPATH
        // to isolate from any system Python installation.
        if let Some(ref home) = self.python_home {
            cmd.env("PYTHONHOME", home);
            cmd.env_remove("PYTHONPATH");
        }

        let mut child = cmd
            .spawn()
            .context(format!("Failed to spawn Python process: {}", self.python_binary.display()))?;

        // Take stdout/stderr handles out of the child so they can be read concurrently.
        // This avoids pipe buffer deadlock: if stdout fills its OS buffer while we
        // haven't started reading stderr, the process blocks and we deadlock.
        let mut child_stdout = child.stdout.take();
        let mut child_stderr = child.stderr.take();

        // Wait with timeout
        let result = tokio::time::timeout(timeout, async {
            let stdout_handle = async {
                let mut buf = Vec::new();
                if let Some(ref mut stdout) = child_stdout {
                    let _ = stdout.read_to_end(&mut buf).await;
                }
                buf
            };
            let stderr_handle = async {
                let mut buf = Vec::new();
                if let Some(ref mut stderr) = child_stderr {
                    let _ = stderr.read_to_end(&mut buf).await;
                }
                buf
            };

            let (stdout_buf, stderr_buf) = tokio::join!(stdout_handle, stderr_handle);
            let status = child.wait().await?;
            Ok::<_, anyhow::Error>((stdout_buf, stderr_buf, status))
        })
        .await;

        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok((stdout_buf, stderr_buf, status))) => {
                let mut stdout = String::from_utf8_lossy(&stdout_buf).to_string();
                let mut stderr = String::from_utf8_lossy(&stderr_buf).to_string();

                // Truncate if too large (char-boundary safe)
                if stdout.len() > self.sandbox.max_output_bytes {
                    let mut truncate_at = self.sandbox.max_output_bytes;
                    while truncate_at > 0 && !stdout.is_char_boundary(truncate_at) {
                        truncate_at -= 1;
                    }
                    stdout.truncate(truncate_at);
                    stdout.push_str("\n... [output truncated]");
                }
                if stderr.len() > self.sandbox.max_output_bytes {
                    let mut truncate_at = self.sandbox.max_output_bytes;
                    while truncate_at > 0 && !stderr.is_char_boundary(truncate_at) {
                        truncate_at -= 1;
                    }
                    stderr.truncate(truncate_at);
                    stderr.push_str("\n... [output truncated]");
                }

                Ok(ExecutionResult {
                    stdout,
                    stderr,
                    exit_code: status.code().unwrap_or(-1),
                    execution_time_ms: elapsed,
                    timed_out: false,
                })
            }
            Ok(Err(e)) => Err(anyhow!("Process error: {}", e)),
            Err(_) => {
                // Timeout — kill the process
                let _ = child.kill().await;
                Ok(ExecutionResult {
                    stdout: String::new(),
                    stderr: format!("Execution timed out after {} seconds", self.sandbox.timeout_seconds),
                    exit_code: -1,
                    execution_time_ms: elapsed,
                    timed_out: true,
                })
            }
        }
    }

    /// Check if the configured Python binary is available.
    pub async fn check_python_available(&self) -> Result<String> {
        let output = Command::new(&self.python_binary)
            .arg("--version")
            .output()
            .await
            .context(format!("Python not found: {}", self.python_binary.display()))?;

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if version.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Ok(stderr) // Some systems output version to stderr
        } else {
            Ok(version)
        }
    }
}

/// Resolve the Python binary path.
///
/// 1. Try bundled `{resource_dir}/python-runtime/bin/python3` (macOS/Linux)
/// 2. Try bundled `{resource_dir}/python-runtime/python.exe` (Windows)
/// 3. Fallback to system `python3` (development mode)
///
/// Returns `(python_binary, python_home)`. `python_home` is `Some` only when
/// using the bundled runtime.
fn resolve_python_path(app_handle: Option<&tauri::AppHandle>) -> (PathBuf, Option<PathBuf>) {
    use tauri::Manager;

    if let Some(handle) = app_handle {
        if let Ok(resource_dir) = handle.path().resource_dir() {
            let runtime_dir = resource_dir.join("python-runtime");

            // macOS / Linux
            let unix_bin = runtime_dir.join("bin").join("python3");
            if unix_bin.exists() {
                info!("Using bundled Python: {}", unix_bin.display());
                return (unix_bin, Some(runtime_dir));
            }

            // Windows
            let win_bin = runtime_dir.join("python.exe");
            if win_bin.exists() {
                info!("Using bundled Python: {}", win_bin.display());
                return (win_bin, Some(runtime_dir));
            }
        }
    }

    info!("Using system Python (bundled runtime not found)");
    (PathBuf::from("python3"), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_check_python_available() {
        let runner = PythonRunner::new(PathBuf::from("/tmp"), None);
        let result = runner.check_python_available().await;
        // Python3 should be available on most dev machines
        if let Ok(version) = result {
            assert!(version.contains("Python") || version.contains("python"));
        }
    }

    #[test]
    fn test_execution_result_serialization() {
        let result = ExecutionResult {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: 0,
            execution_time_ms: 100,
            timed_out: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"exitCode\":0"));
        assert!(json.contains("\"timedOut\":false"));
    }

    #[test]
    fn test_resolve_python_path_no_handle() {
        let (binary, home) = resolve_python_path(None);
        assert_eq!(binary, PathBuf::from("python3"));
        assert!(home.is_none());
    }
}
