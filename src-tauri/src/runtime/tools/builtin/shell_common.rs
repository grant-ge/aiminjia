//! 跨平台 shell 工具共享辅助函数。
//! BashTool（Unix）和 PowerShellTool（Windows）都使用这些函数，确保两种 shell
//! 在输出截断、cancellation、stderr 合并、grep/find 等命令的语义豁免上行为一致。

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::runtime::cancellation::CancellationReason;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::executor::ToolError;
use crate::telemetry::{
    diagnostics_workspace, record_diagnostic, DiagnosticEvent, DiagnosticLevel, DiagnosticSource,
};

pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;

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

pub async fn read_merged_streams<R1, R2>(
    mut stdout: R1,
    mut stderr: R2,
) -> std::io::Result<(Vec<u8>, bool)>
where
    R1: tokio::io::AsyncRead + Unpin,
    R2: tokio::io::AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut stdout_buf = [0u8; 8192];
    let mut stderr_buf = [0u8; 8192];
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut truncated = false;

    while stdout_open || stderr_open {
        tokio::select! {
            read = stdout.read(&mut stdout_buf), if stdout_open => {
                let read = read?;
                if read == 0 {
                    stdout_open = false;
                } else {
                    append_capped_bytes(&mut captured, &stdout_buf[..read], &mut truncated);
                }
            }
            read = stderr.read(&mut stderr_buf), if stderr_open => {
                let read = read?;
                if read == 0 {
                    stderr_open = false;
                } else {
                    append_capped_bytes(&mut captured, &stderr_buf[..read], &mut truncated);
                }
            }
        }
    }

    Ok((captured, truncated))
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
