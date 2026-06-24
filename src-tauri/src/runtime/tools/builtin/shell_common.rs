//! 跨平台 shell 工具共享辅助函数。
//! BashTool（Unix）和 PowerShellTool（Windows）都使用这些函数，确保两种 shell
//! 在输出截断、cancellation、stderr 合并、grep/find 等命令的语义豁免上行为一致。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::runtime::agent::output_writer::{self, TranscriptLine};
use crate::runtime::cancellation::CancellationReason;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::executor::ToolError;
use crate::telemetry::{
    diagnostics_workspace, record_diagnostic, DiagnosticEvent, DiagnosticLevel, DiagnosticSource,
};

pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;

pub struct OptionalTranscriptTarget {
    path: PathBuf,
    flushed_bytes: usize,
}

pub type OptionalTranscriptPath = Arc<Mutex<Option<OptionalTranscriptTarget>>>;

pub fn optional_transcript_path() -> OptionalTranscriptPath {
    Arc::new(Mutex::new(None))
}

pub fn enable_optional_transcript_path(
    target: &OptionalTranscriptPath,
    path: PathBuf,
    flushed_bytes: usize,
) {
    *target.lock().expect("optional transcript path poisoned") = Some(OptionalTranscriptTarget {
        path,
        flushed_bytes,
    });
}

pub fn append_transcript_bytes(path: &Path, bytes: &[u8], context: &str) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let decoded = crate::storage::console_decode::decode_console_bytes(bytes);
    if decoded.is_empty() {
        return true;
    }
    match output_writer::append_line(path, &TranscriptLine::tool(decoded)) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("[{context}] transcript append failed: {e}");
            false
        }
    }
}

pub struct CommandSemantics {
    pub is_error: bool,
    pub message: Option<&'static str>,
}

pub enum ExitKind {
    Completed(std::process::ExitStatus),
    TimedOut,
    Cancelled(Option<CancellationReason>),
}

pub fn base_command(command: &str) -> &str {
    command
        .split('|')
        .next_back()
        .unwrap_or(command)
        .split_whitespace()
        .next()
        .unwrap_or("")
}

pub fn interpret_command_result(command: &str, exit_code: i32) -> CommandSemantics {
    match base_command(command) {
        "grep" | "rg" => CommandSemantics {
            is_error: exit_code >= 2,
            message: (exit_code == 1).then_some("No matches found"),
        },
        "find" => CommandSemantics {
            is_error: exit_code >= 2,
            message: (exit_code == 1).then_some("Some directories were inaccessible"),
        },
        "diff" => CommandSemantics {
            is_error: exit_code >= 2,
            message: (exit_code == 1).then_some("Files differ"),
        },
        "test" | "[" => CommandSemantics {
            is_error: exit_code >= 2,
            message: (exit_code == 1).then_some("Condition is false"),
        },
        _ => CommandSemantics {
            is_error: exit_code != 0,
            message: None,
        },
    }
}

pub fn format_command_failure(
    command: &str,
    exit_code: i32,
    output: &str,
    semantic_message: Option<&str>,
) -> String {
    let mut message = semantic_message
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Command failed with exit code {exit_code}"));
    if exit_code == 127 {
        message.push_str(
            ". The command is not available in this shell; use an installed alternative, a small script in an available runtime, or continue with already verified evidence instead of retrying the same missing command.",
        );
    }
    if semantic_message.is_some() {
        message.push_str(&format!(" (exit code {exit_code})"));
    }
    if !command.is_empty() {
        message.push_str(&format!(": {command}"));
    }
    let trimmed = output.trim();
    if !trimmed.is_empty() {
        message.push('\n');
        message.push_str(trimmed);
    }
    message
}

pub fn format_cancel_message(reason: Option<CancellationReason>, output: &str) -> String {
    let prefix = match reason {
        Some(CancellationReason::Interrupt) => "Command interrupted",
        Some(CancellationReason::SiblingError) => "Command cancelled due to sibling error",
        _ => "Command cancelled",
    };
    if output.trim().is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}\n{}", output.trim())
    }
}

pub fn truncated_to_max_bytes(content: &str, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[..end].to_string(), true)
}

pub fn content_from_output(output: &str, semantic_message: Option<&str>) -> String {
    if output.trim().is_empty() {
        semantic_message.unwrap_or("").to_string()
    } else {
        output.to_string()
    }
}

pub async fn read_merged_streams<R1, R2>(stdout: R1, stderr: R2) -> std::io::Result<(Vec<u8>, bool)>
where
    R1: tokio::io::AsyncRead + Unpin,
    R2: tokio::io::AsyncRead + Unpin,
{
    // Delegate to the with-progress variant with a no-op callback so old
    // callers (PowerShell, tests) keep the existing signature.
    read_merged_streams_with_progress_and_optional_transcript(stdout, stderr, |_, _| {}, None).await
}

/// Same as [`read_merged_streams`] but invokes `on_chunk` synchronously
/// after every successful append.
///
/// Contract:
/// - `captured` is the **current full buffer** (already capped at
///   `MAX_OUTPUT_BYTES`); the callback must NOT mutate it. Take a tail
///   snapshot if you need to forward something.
/// - `total_received_bytes` is the cumulative byte count *including*
///   bytes that were dropped by the cap — so the UI can show "12 KB
///   received" even after the buffer plateaus.
/// - The callback runs in the same task as the IO loop; keep it cheap
///   (e.g. update a `tokio::sync::watch` sender) and let a separate
///   throttling task do the actual emit.
pub async fn read_merged_streams_with_progress<R1, R2, F>(
    stdout: R1,
    stderr: R2,
    on_chunk: F,
) -> std::io::Result<(Vec<u8>, bool)>
where
    R1: tokio::io::AsyncRead + Unpin,
    R2: tokio::io::AsyncRead + Unpin,
    F: FnMut(&[u8], u64) + Send,
{
    read_merged_streams_with_progress_and_optional_transcript(stdout, stderr, on_chunk, None).await
}

pub async fn read_merged_streams_with_progress_and_optional_transcript<R1, R2, F>(
    mut stdout: R1,
    mut stderr: R2,
    mut on_chunk: F,
    transcript_path: Option<OptionalTranscriptPath>,
) -> std::io::Result<(Vec<u8>, bool)>
where
    R1: tokio::io::AsyncRead + Unpin,
    R2: tokio::io::AsyncRead + Unpin,
    F: FnMut(&[u8], u64) + Send,
{
    let mut captured = Vec::new();
    let mut stdout_buf = [0u8; 8192];
    let mut stderr_buf = [0u8; 8192];
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut truncated = false;
    let mut total_received_bytes: u64 = 0;

    while stdout_open || stderr_open {
        tokio::select! {
            read = stdout.read(&mut stdout_buf), if stdout_open => {
                let read = read?;
                if read == 0 {
                    stdout_open = false;
                } else {
                    let captured_len_before = captured.len();
                    append_capped_bytes(&mut captured, &stdout_buf[..read], &mut truncated);
                    total_received_bytes = total_received_bytes.saturating_add(read as u64);
                    on_chunk(&captured, total_received_bytes);
                    append_optional_transcript_progress(
                        transcript_path.as_ref(),
                        &captured,
                        captured_len_before,
                        &stdout_buf[..read],
                    );
                }
            }
            read = stderr.read(&mut stderr_buf), if stderr_open => {
                let read = read?;
                if read == 0 {
                    stderr_open = false;
                } else {
                    let captured_len_before = captured.len();
                    append_capped_bytes(&mut captured, &stderr_buf[..read], &mut truncated);
                    total_received_bytes = total_received_bytes.saturating_add(read as u64);
                    on_chunk(&captured, total_received_bytes);
                    append_optional_transcript_progress(
                        transcript_path.as_ref(),
                        &captured,
                        captured_len_before,
                        &stderr_buf[..read],
                    );
                }
            }
        }
    }

    flush_optional_transcript_captured(transcript_path.as_ref(), &captured);

    Ok((captured, truncated))
}

fn append_optional_transcript_progress(
    target: Option<&OptionalTranscriptPath>,
    captured: &[u8],
    captured_len_before: usize,
    chunk: &[u8],
) {
    let Some(target) = target else {
        return;
    };
    let mut guard = target.lock().expect("optional transcript path poisoned");
    let Some(state) = guard.as_mut() else {
        return;
    };

    let start = state.flushed_bytes.min(captured.len());
    if start < captured.len()
        && append_transcript_bytes(
            &state.path,
            &captured[start..],
            "shell foreground->background",
        )
    {
        state.flushed_bytes = captured.len();
    }

    let captured_from_current = captured
        .len()
        .saturating_sub(captured_len_before)
        .min(chunk.len());
    if captured_from_current < chunk.len() {
        let _ = append_transcript_bytes(
            &state.path,
            &chunk[captured_from_current..],
            "shell foreground->background",
        );
    }
}

fn flush_optional_transcript_captured(target: Option<&OptionalTranscriptPath>, captured: &[u8]) {
    let Some(target) = target else {
        return;
    };
    let mut guard = target.lock().expect("optional transcript path poisoned");
    let Some(state) = guard.as_mut() else {
        return;
    };
    let start = state.flushed_bytes.min(captured.len());
    if start < captured.len()
        && append_transcript_bytes(
            &state.path,
            &captured[start..],
            "shell foreground->background final flush",
        )
    {
        state.flushed_bytes = captured.len();
    }
}

/// Returns the last `n` lines of `s`, ASCII-byte counted. Used to build a
/// compact tail for the live progress event (full output still goes to
/// `metrics.jsonl` and the final tool result).
pub fn tail_n_lines(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let lines: Vec<&str> = s.lines().collect();
    let total = lines.len();
    if total <= n {
        return s.to_string();
    }
    lines[total - n..].join("\n")
}

fn append_capped_bytes(captured: &mut Vec<u8>, chunk: &[u8], truncated: &mut bool) {
    if captured.len() < MAX_OUTPUT_BYTES {
        let remaining = MAX_OUTPUT_BYTES - captured.len();
        let copy_len = remaining.min(chunk.len());
        captured.extend_from_slice(&chunk[..copy_len]);
        if copy_len < chunk.len() {
            *truncated = true;
        }
    } else {
        *truncated = true;
    }
}

pub async fn collect_reader(
    handle: JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
) -> Result<(String, bool), ToolError> {
    let (bytes, truncated) = handle
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("reader task failed: {e}")))?
        .map_err(|e| ToolError::ExecutionFailed(format!("stream read failed: {e}")))?;
    Ok((
        crate::storage::console_decode::decode_console_bytes(&bytes),
        truncated,
    ))
}

/// Inject the bundled runtime bin dir into a child shell's PATH so that
/// shebang scripts like `npm`/`npx`/`uvx` (`#!/usr/bin/env node` /
/// `python3`) can locate the interpreter we ship. Without this every
/// `npm install -g …` emitted by the LLM dies with
/// `env: node: No such file or directory` (observed on real customer
/// machines, see screenshots in the 2026-05-21 review).
///
/// No-op for legacy/test paths whose `ToolExecutionContext` does not carry
/// a runtime resolver.
pub fn inject_bundled_runtime_path(ctx: &ToolExecutionContext, command: &mut Command) {
    let Some(cap) = ctx.capability.as_ref() else {
        return;
    };
    let Some(resolver) = cap.runtime_resolver.as_ref() else {
        return;
    };
    let Ok(deps) = resolver.workspace_dependencies() else {
        return;
    };
    crate::runtime::dependencies::prepend_bundle_bin_to_path_tokio(command, &deps.node);
}

/// Inject the current tracing span's trace/span IDs as environment variables
/// so child processes (Bash, Python, Node) can propagate them further.
///
/// Variables set:
///   TRACE_ID  — the full trace ID (`instance.ms.seq`)
///   SPAN_ID   — the current span's seq (5-digit)
pub fn inject_trace_env(command: &mut Command) {
    if let Some((trace_id, span_id)) = crate::tracing_setup::current_span_context() {
        command.env("TRACE_ID", trace_id).env("SPAN_ID", span_id);
    }
}

/// Classify a shell exit code + stderr into a category we can route on the
/// server side. Mirrors the logic the lotus diagnostics handler uses to
/// elevate signals to Error level for DingTalk alerting.
fn classify_shell_failure(command: &str, exit_code: i32, output: &str) -> Option<&'static str> {
    let is_install_cmd = command.contains("npm install")
        || command.contains("npm i ")
        || command.contains("pnpm install")
        || command.contains("pnpm add")
        || command.contains("yarn add")
        || command.contains("uv pip install")
        || command.contains("pip install")
        || command.contains("uvx ");
    let has_install_failure_marker = output.contains("npm error")
        || output.contains("npm ERR!")
        || output.contains("postinstall")
        || output.contains("ERROR: Could not install");

    if is_install_cmd && (has_install_failure_marker || exit_code != 0) {
        return Some("runtime_install_failure");
    }
    match exit_code {
        127 => Some("command_not_found"),
        126 => Some("permission_denied"),
        124 => Some("command_timeout"),
        0 => None,
        _ => Some("command_failure"),
    }
}

fn stderr_signature(output: &str) -> Option<String> {
    output
        .lines()
        .rev()
        .find(|line| {
            let l = line.to_ascii_lowercase();
            l.contains("not found")
                || l.contains("no such file")
                || l.contains("permission denied")
                || l.contains("error:")
                || l.contains("err!")
        })
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.chars().count() > 240 {
                let truncated: String = trimmed.chars().take(240).collect();
                format!("{truncated}…")
            } else {
                trimmed.to_string()
            }
        })
}

fn tail_chars(output: &str, max_chars: usize) -> String {
    let total = output.chars().count();
    if total <= max_chars {
        return output.to_string();
    }
    let skip = total - max_chars;
    output.chars().skip(skip).collect()
}

/// Record a diagnostic for a shell command that ended with a non-zero exit
/// code or otherwise looked like a runtime install failure. The server
/// classifies on `payload.category` and elevates known severe categories
/// (`runtime_install_failure`, `command_not_found`) into DingTalk alerts.
pub fn emit_shell_failure_diagnostic(
    ctx: &ToolExecutionContext,
    tool: &str,
    command: &str,
    exit_code: i32,
    output: &str,
    is_semantic_error: bool,
) {
    let Some(category) = classify_shell_failure(command, exit_code, output) else {
        return;
    };
    if !is_semantic_error && exit_code == 0 {
        return;
    }
    let level = match category {
        "runtime_install_failure" | "command_not_found" => DiagnosticLevel::Error,
        _ => DiagnosticLevel::Warn,
    };
    let ws = diagnostics_workspace();
    let signature = stderr_signature(output);
    let tail = tail_chars(output, 800);

    record_diagnostic(
        &ws,
        DiagnosticEvent::new(format!("tool.{tool}.failure"), DiagnosticSource::Backend)
            .level(level)
            .conversation_id(ctx.session_id.as_str())
            .run_id(ctx.run_id.as_str())
            .tool_call_id(ctx.tool_call_id.as_str())
            .error(
                signature
                    .clone()
                    .unwrap_or_else(|| format!("exit_code={exit_code}")),
            )
            .payload(serde_json::json!({
                "category": category,
                "tool": tool,
                "exit_code": exit_code,
                "command": command.chars().take(400).collect::<String>(),
                "stderr_signature": signature,
                "output_tail": tail,
            })),
    );
}

#[cfg(test)]
mod classifier_tests {
    use super::{classify_shell_failure, stderr_signature, tail_chars};

    #[test]
    fn npm_install_postinstall_failure_classified() {
        let output = "npm error code 127\nnpm error sh: node: command not found";
        assert_eq!(
            classify_shell_failure("npm install -g dws", 1, output),
            Some("runtime_install_failure")
        );
    }

    #[test]
    fn bare_command_not_found_uses_command_not_found() {
        assert_eq!(
            classify_shell_failure("dws --version", 127, "/bin/sh: dws: command not found"),
            Some("command_not_found")
        );
    }

    #[test]
    fn exit_zero_returns_none() {
        assert_eq!(classify_shell_failure("ls", 0, ""), None);
    }

    #[test]
    fn signature_picks_last_error_line() {
        let out = "v22.15.0\nnpm error sh: node: command not found\nbye";
        assert_eq!(
            stderr_signature(out).as_deref(),
            Some("npm error sh: node: command not found")
        );
    }

    #[test]
    fn tail_chars_handles_short_output() {
        assert_eq!(tail_chars("hello", 100), "hello");
    }

    #[test]
    fn tail_chars_truncates_long_output() {
        let s: String = std::iter::repeat('a').take(2000).collect();
        let tail = tail_chars(&s, 500);
        assert_eq!(tail.chars().count(), 500);
    }
}

pub async fn kill_child_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            let _ = unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
            let _ = child.wait().await;
            return;
        }
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(test)]
mod progress_helpers_tests {
    use super::{read_merged_streams_with_progress, tail_n_lines};
    use std::sync::{Arc, Mutex};

    #[test]
    fn tail_n_lines_returns_full_string_when_under_limit() {
        let s = "a\nb\nc";
        assert_eq!(tail_n_lines(s, 5), s);
    }

    #[test]
    fn tail_n_lines_returns_last_n_lines() {
        let s = "a\nb\nc\nd\ne";
        assert_eq!(tail_n_lines(s, 2), "d\ne");
    }

    #[test]
    fn tail_n_lines_with_zero_returns_empty() {
        let s = "a\nb\nc";
        assert_eq!(tail_n_lines(s, 0), "");
    }

    /// `on_chunk` must be called for every appended chunk, with monotonically
    /// non-decreasing `total_received_bytes` and the current full buffer.
    #[tokio::test]
    async fn on_chunk_called_per_append_with_monotonic_bytes() {
        let stdout = std::io::Cursor::new(b"hello\n".to_vec());
        let stderr = std::io::Cursor::new(b"world\n".to_vec());
        let observed: Arc<Mutex<Vec<(usize, u64)>>> = Arc::new(Mutex::new(vec![]));
        let observed_clone = observed.clone();

        let (bytes, truncated) =
            read_merged_streams_with_progress(stdout, stderr, move |buf, total| {
                observed_clone.lock().unwrap().push((buf.len(), total));
            })
            .await
            .unwrap();

        assert!(!truncated);
        // Captured bytes contain both streams (order indeterminate).
        let captured_str = String::from_utf8_lossy(&bytes);
        assert!(captured_str.contains("hello"));
        assert!(captured_str.contains("world"));

        let observed = observed.lock().unwrap().clone();
        assert!(!observed.is_empty(), "on_chunk should fire at least once");
        let mut prev_total = 0u64;
        for (_buf_len, total) in &observed {
            assert!(*total >= prev_total, "total bytes must be monotonic");
            prev_total = *total;
        }
        // Final total ≥ sum of input lengths (6 + 6 = 12).
        assert!(prev_total >= 12);
    }
}
