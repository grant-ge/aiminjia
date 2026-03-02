//! Persistent Python session manager for analysis mode.
//!
//! Maintains a long-running Python REPL process per conversation,
//! eliminating cold-start overhead (import, pkl restore) on every call.
//! Falls back to one-shot PythonRunner on crash/timeout via checkpoint recovery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, ChildStderr, Command};
use tokio::sync::Mutex;

use crate::python::runner::ExecutionResult;
use crate::python::sandbox::SandboxConfig;

/// Maximum number of concurrent Python sessions.
const MAX_SESSIONS: usize = 3;

/// Idle timeout before a session is automatically reaped (15 minutes).
const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Seconds to wait for graceful __DONE__ after SIGINT before force-killing.
const INTERRUPT_GRACE_SECS: u64 = 5;

// ---------------------------------------------------------------------------
// Embedded Python scripts (source files alongside this module)
// ---------------------------------------------------------------------------

/// Python REPL loop script. Written to temp dir and executed by the session process.
///
/// Protocol:
///   stdin:   __EXEC__ {uuid} {timeout}\n{code}\n__END__\n
///   stdout:  {user print output}\n__DONE__ {uuid}\n
///   stderr:  exception tracebacks / warnings
///   meta:    {workspace}/temp/_meta_{uuid}.json (structured result)
const REPL_SCRIPT: &str = include_str!("_repl.py");

/// Python code to checkpoint session state (DataFrame + user variables) to pkl files.
const CHECKPOINT_SCRIPT: &str = include_str!("_checkpoint.py");

/// Python code to restore session state from pkl checkpoint files.
const RESTORE_SCRIPT: &str = include_str!("_restore.py");

// ---------------------------------------------------------------------------
// PythonSession
// ---------------------------------------------------------------------------

struct PythonSession {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    #[allow(dead_code)]
    stderr_reader: Mutex<BufReader<ChildStderr>>,
    execution_lock: Mutex<()>,
    created_at: Instant,
    last_used: AtomicU64,
    initialized: AtomicBool,
    conversation_id: String,
    workspace_path: PathBuf,
}

impl PythonSession {
    /// Spawn a new Python REPL process.
    async fn spawn(
        conversation_id: &str,
        workspace_path: &Path,
        python_binary: &Path,
        python_home: Option<&PathBuf>,
    ) -> Result<Self> {
        // Write REPL script to temp (only if missing — content is a compile-time constant)
        let temp_dir = workspace_path.join("temp");
        std::fs::create_dir_all(&temp_dir)?;
        let repl_path = temp_dir.join("_repl.py");
        if !repl_path.exists() {
            std::fs::write(&repl_path, REPL_SCRIPT)
                .context("Failed to write REPL script")?;
        }

        let mut cmd = Command::new(python_binary);
        cmd.arg("-u") // unbuffered
            .arg(&repl_path)
            .current_dir(workspace_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        super::configure_python_env(&mut cmd, python_home.map(|p| p.as_path()));

        let mut child = cmd.spawn()
            .context(format!("Failed to spawn Python REPL: {}", python_binary.display()))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| anyhow!("Failed to capture Python stdin"))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow!("Failed to capture Python stdout"))?;
        let stderr = child.stderr.take()
            .ok_or_else(|| anyhow!("Failed to capture Python stderr"))?;

        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        info!("[SESSION] Spawned Python REPL for conversation {}", conversation_id);

        Ok(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            stderr_reader: Mutex::new(BufReader::new(stderr)),
            execution_lock: Mutex::new(()),
            created_at: Instant::now(),
            last_used: AtomicU64::new(now_ts),
            initialized: AtomicBool::new(false),
            conversation_id: conversation_id.to_string(),
            workspace_path: workspace_path.to_path_buf(),
        })
    }

    /// Check if the underlying process is still running.
    async fn is_alive(&self) -> bool {
        let mut child = self.child.lock().await;
        match child.try_wait() {
            Ok(None) => true,  // still running
            _ => false,        // exited or error
        }
    }

    /// Update last_used timestamp.
    fn touch(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_used.store(now, Ordering::Relaxed);
    }

    /// Get seconds since last use.
    fn idle_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = self.last_used.load(Ordering::Relaxed);
        now.saturating_sub(last)
    }

    /// Initialize the session: inject sandbox preamble + trusted imports + analysis utils.
    /// Called once after spawn, before any user code.
    async fn initialize(&self, sandbox: &SandboxConfig) -> Result<()> {
        if self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }

        let preamble = sandbox.preamble();
        let uuid = uuid::Uuid::new_v4().to_string();

        // Inject: sandbox preamble + make _written_files accessible to _safe_open
        let init_code = format!(
            "{preamble}\n\n# Make _written_files global for _safe_open tracking\n\
             import builtins as _builtins\n\
             _builtins._written_files = _written_files\n",
        );

        self.send_code(&uuid, &init_code, Duration::from_secs(30)).await
            .context("Session initialization (sandbox preamble) failed")?;

        // Load analysis utils if the file exists
        let utils_path = self.workspace_path.join("temp/_analysis_utils.py");
        if utils_path.exists() {
            let uuid2 = uuid::Uuid::new_v4().to_string();
            let load_utils = format!(
                "_au_path = '{}'\nexec(open(_au_path, encoding='utf-8').read())\n",
                utils_path.display().to_string().replace('\\', "\\\\").replace('\'', "\\'")
            );
            self.send_code(&uuid2, &load_utils, Duration::from_secs(30)).await
                .context("Session initialization (analysis utils) failed")?;
        }

        self.initialized.store(true, Ordering::Relaxed);
        info!("[SESSION] Initialized session for conversation {}", self.conversation_id);
        Ok(())
    }

    /// Send code and wait for result. Returns (clean_stdout, stderr_snapshot, meta).
    async fn send_code(
        &self,
        uuid: &str,
        code: &str,
        timeout: Duration,
    ) -> Result<ExecutionResult> {
        self.touch();

        // Send code block via stdin
        {
            let mut stdin = self.stdin.lock().await;
            let header = format!("__EXEC__ {} {}\n", uuid, timeout.as_secs());
            stdin.write_all(header.as_bytes()).await
                .context("Failed to write to Python stdin")?;
            stdin.write_all(code.as_bytes()).await
                .context("Failed to write code to Python stdin")?;
            if !code.ends_with('\n') {
                stdin.write_all(b"\n").await?;
            }
            stdin.write_all(b"__END__\n").await
                .context("Failed to write __END__ marker")?;
            stdin.flush().await?;
        }

        // Read stdout lines until __DONE__ {uuid}
        // Cap at 1MB to prevent unbounded memory growth from runaway scripts.
        const MAX_STDOUT_BYTES: usize = 1_000_000;
        let done_marker = format!("__DONE__ {}", uuid);
        let mut stdout_lines = Vec::new();
        let mut total_bytes: usize = 0;
        let mut output_capped = false;

        let read_result = tokio::time::timeout(timeout, async {
            let mut stdout = self.stdout.lock().await;
            loop {
                let mut line = String::new();
                let n = stdout.read_line(&mut line).await?;
                if n == 0 {
                    return Err(anyhow!("Python process closed stdout unexpectedly"));
                }
                if line.trim() == done_marker {
                    break;
                }
                if !output_capped {
                    total_bytes += line.len();
                    if total_bytes > MAX_STDOUT_BYTES {
                        output_capped = true;
                        stdout_lines.push("[output truncated — exceeded 1MB limit]\n".to_string());
                    } else {
                        stdout_lines.push(line);
                    }
                }
                // When capped, keep draining until __DONE__ but discard content
            }
            Ok::<_, anyhow::Error>(())
        })
        .await;

        let timed_out = match read_result {
            Ok(Ok(())) => false,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                // Timeout — kill and let caller handle restart
                warn!("[SESSION] Execution timed out after {}s for conversation {}",
                    timeout.as_secs(), self.conversation_id);
                let _ = self.kill().await;
                return Err(anyhow!(
                    "Execution timed out after {} seconds. The code took too long.",
                    timeout.as_secs()
                ));
            }
        };

        // Read meta JSON file
        let meta_path = self.workspace_path.join(format!("temp/_meta_{}.json", uuid));
        let (exit_code, execution_time_ms) = if meta_path.exists() {
            match std::fs::read_to_string(&meta_path) {
                Ok(content) => {
                    let _ = std::fs::remove_file(&meta_path);
                    match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(meta) => (
                            meta.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                            meta.get("execution_time_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                        ),
                        Err(_) => (0, 0),
                    }
                }
                Err(_) => (0, 0),
            }
        } else {
            (0, 0)
        };

        // Drain stderr (non-blocking best-effort)
        let stderr_content = self.drain_stderr().await;

        let stdout_str = stdout_lines.concat();

        Ok(ExecutionResult {
            stdout: stdout_str,
            stderr: stderr_content,
            exit_code,
            execution_time_ms,
            timed_out,
        })
    }

    /// Non-blocking drain of available stderr content.
    async fn drain_stderr(&self) -> String {
        let mut stderr = match self.stderr_reader.try_lock() {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        let mut buf = vec![0u8; 65536];
        match tokio::time::timeout(Duration::from_millis(50), stderr.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => String::from_utf8_lossy(&buf[..n]).to_string(),
            _ => String::new(),
        }
    }

    /// Send SIGINT (Unix) or kill (Windows) to interrupt current execution.
    async fn interrupt(&self) -> Result<()> {
        let child = self.child.lock().await;
        if let Some(pid) = child.id() {
            info!("[SESSION] Interrupting Python process (pid={}) for conversation {}",
                pid, self.conversation_id);
            #[cfg(unix)]
            {
                unsafe { libc::kill(pid as i32, libc::SIGINT); }
            }
            #[cfg(windows)]
            {
                // Windows: no reliable SIGINT for non-console processes.
                // Drop lock and kill.
                drop(child);
                let _ = self.kill().await;
            }
        }
        Ok(())
    }

    /// Kill the Python process.
    async fn kill(&self) -> Result<()> {
        let mut child = self.child.lock().await;
        info!("[SESSION] Killing Python process for conversation {}", self.conversation_id);
        let _ = child.kill().await;
        Ok(())
    }

    /// Write checkpoint: send Python code that pickles current state to disk.
    async fn write_checkpoint(&self) -> Result<()> {
        if !self.is_alive().await {
            return Ok(());
        }

        let uuid = uuid::Uuid::new_v4().to_string();
        match self.send_code(&uuid, CHECKPOINT_SCRIPT, Duration::from_secs(30)).await {
            Ok(_) => {
                info!("[SESSION] Checkpoint written for conversation {}", self.conversation_id);
                Ok(())
            }
            Err(e) => {
                warn!("[SESSION] Checkpoint failed for conversation {}: {}", self.conversation_id, e);
                Err(e)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PythonSessionManager
// ---------------------------------------------------------------------------

/// Manages persistent Python sessions, one per active analysis conversation.
pub struct PythonSessionManager {
    /// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) because the lock is
    /// only held for brief HashMap lookups — never across `.await` points.
    /// This avoids the `Send` bound overhead of tokio's Mutex and is safe because
    /// all async operations (is_alive, kill, checkpoint) happen *after* releasing the lock.
    sessions: std::sync::Mutex<HashMap<String, Arc<PythonSession>>>,
    workspace_path: PathBuf,
    python_binary: PathBuf,
    python_home: Option<PathBuf>,
}

impl PythonSessionManager {
    /// Create a new session manager.
    pub fn new(workspace_path: PathBuf, app_handle: Option<&tauri::AppHandle>) -> Self {
        let (python_binary, python_home) = super::runner::resolve_python_path(app_handle);
        Self {
            sessions: std::sync::Mutex::new(HashMap::new()),
            workspace_path,
            python_binary,
            python_home,
        }
    }

    /// Execute code in a persistent session for the given conversation.
    ///
    /// - Lazily spawns a new session on first call.
    /// - Re-uses existing session on subsequent calls.
    /// - Handles crash recovery (restart + checkpoint restore).
    pub async fn execute(
        &self,
        conversation_id: &str,
        code: &str,
        timeout: Duration,
        sandbox: &SandboxConfig,
    ) -> Result<SessionExecResult> {
        let session = self.get_or_create(conversation_id).await?;

        // Serialize execution within the same conversation
        let _lock = session.execution_lock.lock().await;

        // Health check under lock
        if !session.is_alive().await {
            warn!("[SESSION] Process dead for conversation {}, restarting with recovery",
                conversation_id);
            let new_session = self.restart_session(conversation_id).await?;
            new_session.initialize(sandbox).await?;
            self.restore_from_checkpoint(&new_session).await;
            let uuid = uuid::Uuid::new_v4().to_string();
            let result = new_session.send_code(&uuid, code, timeout).await?;
            return Ok(SessionExecResult { result });
        }

        // Initialize if first call
        session.initialize(sandbox).await?;

        let uuid = uuid::Uuid::new_v4().to_string();
        let result = session.send_code(&uuid, code, timeout).await?;
        Ok(SessionExecResult { result })
    }

    /// Interrupt the current execution for a conversation (stop_streaming).
    pub async fn interrupt(&self, conversation_id: &str) -> Result<()> {
        let session = {
            let sessions = self.sessions.lock().unwrap();
            sessions.get(conversation_id).cloned()
        };
        if let Some(session) = session {
            session.interrupt().await?;
        }
        Ok(())
    }

    /// Destroy a session (conversation deleted).
    pub async fn destroy(&self, conversation_id: &str) {
        let session = {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.remove(conversation_id)
        };
        if let Some(session) = session {
            let _ = session.write_checkpoint().await;
            let _ = session.kill().await;
            info!("[SESSION] Destroyed session for conversation {}", conversation_id);
        }
    }

    /// Shutdown all sessions (app exit).
    pub async fn shutdown_all(&self) {
        let sessions: Vec<(String, Arc<PythonSession>)> = {
            let mut map = self.sessions.lock().unwrap();
            map.drain().collect()
        };
        for (conv_id, session) in &sessions {
            let _ = session.write_checkpoint().await;
            let _ = session.kill().await;
            info!("[SESSION] Shutdown session for conversation {}", conv_id);
        }
    }

    /// Reap idle sessions that have exceeded IDLE_TIMEOUT.
    /// Call this periodically (e.g., from a background timer).
    pub async fn reap_idle(&self) {
        let idle_convs: Vec<String> = {
            let sessions = self.sessions.lock().unwrap();
            sessions.iter()
                .filter(|(_, s)| s.idle_seconds() > IDLE_TIMEOUT.as_secs())
                .map(|(k, _)| k.clone())
                .collect()
        };
        for conv_id in idle_convs {
            info!("[SESSION] Reaping idle session for conversation {}", conv_id);
            self.destroy(&conv_id).await;
        }
    }

    /// Return diagnostic information about active Python sessions.
    ///
    /// Useful for debugging and monitoring. Checks process liveness
    /// outside the sync Mutex to avoid holding it across await points.
    pub async fn session_stats(&self) -> Vec<SessionInfo> {
        let snapshots: Vec<(String, Arc<PythonSession>)> = {
            let sessions = self.sessions.lock().unwrap();
            sessions.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        let mut infos = Vec::with_capacity(snapshots.len());
        for (conv_id, session) in snapshots {
            let alive = session.is_alive().await;
            infos.push(SessionInfo {
                conversation_id: conv_id,
                alive,
                idle_seconds: session.idle_seconds(),
                initialized: session.initialized.load(Ordering::Relaxed),
                uptime_seconds: session.created_at.elapsed().as_secs(),
            });
        }
        infos
    }

    // --- internal helpers ---

    async fn get_or_create(&self, conversation_id: &str) -> Result<Arc<PythonSession>> {
        // Fast path: clone Arc under lock, release lock, then await is_alive outside lock.
        // IMPORTANT: We must NOT call block_on() or .await while holding the sync Mutex,
        // as that would block the tokio executor thread and cause deadlocks.
        let maybe_existing = {
            let sessions = self.sessions.lock().unwrap();
            sessions.get(conversation_id).cloned()
        };
        if let Some(session) = maybe_existing {
            if session.is_alive().await {
                session.touch();
                return Ok(session);
            }
            // Dead — will remove below
        }

        // Remove dead session if exists
        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.remove(conversation_id);

            // Evict LRU if at capacity
            if sessions.len() >= MAX_SESSIONS {
                let lru_key = sessions.iter()
                    .min_by_key(|(_, s)| s.last_used.load(Ordering::Relaxed))
                    .map(|(k, _)| k.clone());
                if let Some(key) = lru_key {
                    if let Some(evicted) = sessions.remove(&key) {
                        info!("[SESSION] Evicting LRU session for conversation {}", key);
                        // Write checkpoint in background (don't block)
                        tokio::spawn(async move {
                            let _ = evicted.write_checkpoint().await;
                            let _ = evicted.kill().await;
                        });
                    }
                }
            }
        }

        // Spawn new session
        let session = PythonSession::spawn(
            conversation_id,
            &self.workspace_path,
            &self.python_binary,
            self.python_home.as_ref(),
        ).await?;

        let session = Arc::new(session);
        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.insert(conversation_id.to_string(), session.clone());
        }
        Ok(session)
    }

    async fn restart_session(&self, conversation_id: &str) -> Result<Arc<PythonSession>> {
        // Remove old
        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.remove(conversation_id);
        }
        // Spawn fresh
        let session = PythonSession::spawn(
            conversation_id,
            &self.workspace_path,
            &self.python_binary,
            self.python_home.as_ref(),
        ).await?;
        let session = Arc::new(session);
        {
            let mut sessions = self.sessions.lock().unwrap();
            sessions.insert(conversation_id.to_string(), session.clone());
        }
        Ok(session)
    }

    /// Restore state from pkl checkpoint files (if they exist).
    async fn restore_from_checkpoint(&self, session: &PythonSession) {
        let uuid = uuid::Uuid::new_v4().to_string();
        match session.send_code(&uuid, RESTORE_SCRIPT, Duration::from_secs(30)).await {
            Ok(_) => info!("[SESSION] Checkpoint restored for conversation {}",
                session.conversation_id),
            Err(e) => warn!("[SESSION] Checkpoint restore failed: {}", e),
        }
    }
}

// ---------------------------------------------------------------------------
// SessionExecResult — return type from execute()
// ---------------------------------------------------------------------------

/// Combined result of a session execution.
pub struct SessionExecResult {
    pub result: ExecutionResult,
}

/// Diagnostic snapshot of a single Python session.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub conversation_id: String,
    pub alive: bool,
    pub idle_seconds: u64,
    pub initialized: bool,
    pub uptime_seconds: u64,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repl_script_contains_protocol() {
        assert!(REPL_SCRIPT.contains("__EXEC__"));
        assert!(REPL_SCRIPT.contains("__END__"));
        assert!(REPL_SCRIPT.contains("__DONE__"));
        assert!(REPL_SCRIPT.contains("_meta_"));
        assert!(REPL_SCRIPT.contains("_written_files"));
    }

    #[test]
    fn test_repl_script_handles_keyboard_interrupt() {
        assert!(REPL_SCRIPT.contains("KeyboardInterrupt"));
        assert!(REPL_SCRIPT.contains("exit_code = 130"));
    }
}
