//! BashTool — execute shell commands inside the authorized workspace.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
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
        Some(CancellationReason::BackgroundStop) => "Command cancelled because the background run stopped",
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

async fn read_stream_to_limit<R>(mut reader: R) -> std::io::Result<(Vec<u8>, bool)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut buf = [0u8; 8192];
    let mut truncated = false;

    loop {
        let read = reader.read(&mut buf).await?;
        if read == 0 {
            break;
        }

        if captured.len() < MAX_OUTPUT_BYTES {
            let remaining = MAX_OUTPUT_BYTES - captured.len();
            let copy_len = remaining.min(read);
            captured.extend_from_slice(&buf[..copy_len]);
            if copy_len < read {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }

    Ok((captured, truncated))
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

#[async_trait]
impl RuntimeTool for BashTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("bash")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("bash", "Execute shell command"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
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

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(&command)
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
        let stdout_handle = tokio::spawn(read_stream_to_limit(stdout));
        let stderr_handle = tokio::spawn(read_stream_to_limit(stderr));

        let exit_kind = tokio::select! {
            status = child.wait() => {
                ExitKind::Completed(
                    status.map_err(|e| ToolError::ExecutionFailed(format!("Failed waiting for process: {e}")))?
                )
            }
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                ExitKind::TimedOut
            }
            reason = wait_for_cancellation(ctx.cancellation.clone()) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                ExitKind::Cancelled(reason)
            }
        };

        let (stdout_str, stdout_truncated) = collect_reader(stdout_handle).await?;
        let (stderr_str, stderr_truncated) = collect_reader(stderr_handle).await?;
        let combined_output = format!("{stdout_str}{stderr_str}");
        let (combined_output, combined_truncated) =
            truncated_to_max_bytes(&combined_output, MAX_OUTPUT_BYTES);
        let truncated = stdout_truncated || stderr_truncated || combined_truncated;

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
