use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeToolProbe {
    pub name: String,
    pub executable: PathBuf,
}

impl RuntimeToolProbe {
    pub fn new(name: impl Into<String>, executable: PathBuf) -> Self {
        Self {
            name: name.into(),
            executable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeHealthReport {
    versions: BTreeMap<String, String>,
}

impl RuntimeHealthReport {
    pub fn tool_version(&self, name: &str) -> Option<&str> {
        self.versions.get(name).map(|value| value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHealthError {
    CommandFailed { name: String, message: String },
    CommandTimedOut { name: String, timeout_ms: u64 },
}

impl fmt::Display for RuntimeHealthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed { name, message } => {
                write!(f, "runtime health check failed for {name}: {message}")
            }
            Self::CommandTimedOut { name, timeout_ms } => {
                write!(
                    f,
                    "runtime health check timed out for {name} after {timeout_ms}ms"
                )
            }
        }
    }
}

impl Error for RuntimeHealthError {}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeHealthChecker {
    timeout: Duration,
}

impl Default for RuntimeHealthChecker {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
        }
    }
}

impl RuntimeHealthChecker {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    pub fn check(
        &self,
        probes: &[RuntimeToolProbe],
    ) -> Result<RuntimeHealthReport, RuntimeHealthError> {
        let mut versions = BTreeMap::new();

        for probe in probes {
            let output = run_version_command_with_timeout(probe, self.timeout)?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let message = if stderr.is_empty() {
                    format!("command exited with status {}", output.status)
                } else {
                    stderr
                };
                return Err(RuntimeHealthError::CommandFailed {
                    name: probe.name.clone(),
                    message,
                });
            }

            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            versions.insert(probe.name.clone(), version);
        }

        Ok(RuntimeHealthReport { versions })
    }
}

fn run_version_command_with_timeout(
    probe: &RuntimeToolProbe,
    timeout: Duration,
) -> Result<Output, RuntimeHealthError> {
    let mut command = Command::new(&probe.executable);
    command
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Prepend our bundle's bin dir to PATH so that shebang scripts (e.g. npm/npx
    // which start with `#!/usr/bin/env node`) can find the bundled node interpreter
    // rather than relying on a system-wide node that may not exist.
    super::command_env::prepend_bundle_bin_to_path(&mut command, &probe.executable);
    configure_child_process_group(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| RuntimeHealthError::CommandFailed {
            name: probe.name.clone(),
            message: error.to_string(),
        })?;

    let started_at = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| RuntimeHealthError::CommandFailed {
                name: probe.name.clone(),
                message: error.to_string(),
            })?
            .is_some()
        {
            return child
                .wait_with_output()
                .map_err(|error| RuntimeHealthError::CommandFailed {
                    name: probe.name.clone(),
                    message: error.to_string(),
                });
        }

        if started_at.elapsed() >= timeout {
            kill_child_process_tree(&mut child);
            return Err(RuntimeHealthError::CommandTimedOut {
                name: probe.name.clone(),
                timeout_ms: timeout.as_millis().try_into().unwrap_or(u64::MAX),
            });
        }

        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn configure_child_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_child_process_group(_command: &mut Command) {}

fn kill_child_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        let _ = unsafe { libc::killpg(pid, libc::SIGKILL) };
        let _ = child.wait();
        return;
    }

    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}
