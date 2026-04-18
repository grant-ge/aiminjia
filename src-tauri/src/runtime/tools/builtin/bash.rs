//! BashTool — execute shell commands inside the authorized workspace.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::runtime::cancellation::{CancellationReason, CancellationToken};
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;

use super::workspace::require_workspace_root;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
const MAX_OUTPUT_BYTES: usize = 512 * 1024;

static DANGEROUS_PATTERNS: &[(&str, &str)] = &[
    ("rm -rf /", "Refusing: rm -rf / would destroy the entire filesystem"),
    ("rm -rf /*", "Refusing: rm -rf /* would destroy the entire filesystem"),
    ("> /etc/", "Refusing: writing to /etc/ is not allowed"),
    (">> /etc/", "Refusing: writing to /etc/ is not allowed"),
    ("> /bin/", "Refusing: writing to /bin/ is not allowed"),
    ("> /usr/bin/", "Refusing: writing to /usr/bin/ is not allowed"),
    ("mkfs", "Refusing: mkfs formats filesystems"),
    ("dd if=", "Refusing: dd with if= can be dangerous; use with caution"),
];

struct CommandSemantics {
    is_error: bool,
    message: Option<&'static str>,
}

enum ExitKind {
    Completed(std::process::ExitStatus),
    TimedOut,
    Cancelled(Option<CancellationReason>),
}

pub struct BashTool;

fn tool_result_bash(content: String, data: Value) -> ToolResult {
    ToolResult {
        tool_name: "bash".to_string(),
        content,
        data: Some(data),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

fn base_command(command: &str) -> &str {
    command
        .split('|')
        .next_back()
        .unwrap_or(command)
        .split_whitespace()
        .next()
        .unwrap_or("")
}

fn interpret_command_result(command: &str, exit_code: i32) -> CommandSemantics {
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

fn format_command_failure(
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

fn format_cancel_message(reason: Option<CancellationReason>, output: &str) -> String {
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

fn truncated_to_max_bytes(content: &str, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[..end].to_string(), true)
}

fn content_from_output(output: &str, semantic_message: Option<&str>) -> String {
    if output.trim().is_empty() {
        semantic_message.unwrap_or("").to_string()
    } else {
        output.to_string()
    }
}

async fn read_merged_streams<R1, R2>(
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

async fn collect_reader(
    handle: JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
) -> Result<(String, bool), ToolError> {
    let (bytes, truncated) = handle
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("reader task failed: {e}")))?
        .map_err(|e| ToolError::ExecutionFailed(format!("stream read failed: {e}")))?;
    Ok((String::from_utf8_lossy(&bytes).to_string(), truncated))
}

async fn wait_for_cancellation(token: CancellationToken) -> Option<CancellationReason> {
    loop {
        if token.is_cancelled() {
            return token.reason();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(unix)]
fn configure_child_process_group(command: &mut Command) {
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

async fn kill_child_process_tree(child: &mut Child) {
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

#[async_trait]
impl RuntimeTool for BashTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("bash")
            .unwrap_or_else(|| ToolDefinition::new("bash", "Execute shell command"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    fn validate_input(&self, input: &Value) -> Option<ToolError> {
        match input.get("command") {
            None => Some(ToolError::InputValidationError {
                tool_name: "bash".to_string(),
                message: "Missing required field: command (string)".to_string(),
            }),
            Some(value) if !value.is_string() => Some(ToolError::InputValidationError {
                tool_name: "bash".to_string(),
                message: format!(
                    "Field 'command' must be a string, got: {}",
                    value.to_string().chars().take(40).collect::<String>()
                ),
            }),
            _ => None,
        }
    }

    async fn check_permissions(
        &self,
        input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        let command = input.get("command").and_then(Value::as_str).unwrap_or("");
        for (pattern, message) in DANGEROUS_PATTERNS {
            if command.contains(pattern) {
                return Some(PermissionDecision::Deny {
                    message: (*message).to_string(),
                    reason: PermissionReason::Other("dangerous_pattern".to_string()),
                });
            }
        }
        None
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let root = require_workspace_root(&ctx)?;
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: command".into()))?
            .to_string();
        let timeout_secs = input
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);
        let wrapped_command = format!("exec 2>&1; {command}");

        let mut shell = Command::new("/bin/sh");
        configure_child_process_group(&mut shell);
        let mut child = shell
            .arg("-c")
            .arg(&wrapped_command)
            .current_dir(&root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn process: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::ExecutionFailed("stdout pipe missing".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ToolError::ExecutionFailed("stderr pipe missing".into()))?;
        let merged_handle = tokio::spawn(read_merged_streams(stdout, stderr));

        let exit_kind = tokio::select! {
            status = child.wait() => {
                ExitKind::Completed(
                    status.map_err(|e| ToolError::ExecutionFailed(format!("Failed waiting for process: {e}")))?
                )
            }
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
                kill_child_process_tree(&mut child).await;
                ExitKind::TimedOut
            }
            reason = wait_for_cancellation(ctx.cancellation.clone()) => {
                kill_child_process_tree(&mut child).await;
                ExitKind::Cancelled(reason)
            }
        };

        let (combined_output, stream_truncated) = collect_reader(merged_handle).await?;
        let (combined_output, combined_truncated) =
            truncated_to_max_bytes(&combined_output, MAX_OUTPUT_BYTES);
        let truncated = stream_truncated || combined_truncated;

        match exit_kind {
            ExitKind::Completed(status) => {
                let exit_code = status.code().unwrap_or(-1);
                let semantics = interpret_command_result(&command, exit_code);
                if semantics.is_error {
                    return Err(ToolError::ExecutionFailed(format_command_failure(
                        &command,
                        exit_code,
                        &combined_output,
                        semantics.message,
                    )));
                }

                let content = content_from_output(&combined_output, semantics.message);
                Ok(tool_result_bash(
                    content,
                    json!({
                        "command": command,
                        "exit_code": exit_code,
                        "stdout_stderr": combined_output,
                        "truncated": truncated,
                        "semantic_message": semantics.message,
                    }),
                ))
            }
            ExitKind::TimedOut => Err(ToolError::ExecutionFailed(format_command_failure(
                &command,
                124,
                &combined_output,
                Some(&format!("Command timed out after {timeout_secs}s")),
            ))),
            ExitKind::Cancelled(reason) => Err(ToolError::ExecutionFailed(format_cancel_message(
                reason,
                &combined_output,
            ))),
        }
    }
}
