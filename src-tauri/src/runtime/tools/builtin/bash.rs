//! BashTool — execute shell commands inside the authorized workspace.
//! Unix-only; Windows uses PowerShellTool instead.

#![cfg(not(windows))]

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;

use crate::runtime::cancellation::wait_for_cancellation;
use super::shell_common::{
    collect_reader, content_from_output, format_cancel_message, format_command_failure,
    interpret_command_result, kill_child_process_tree, read_merged_streams,
    truncated_to_max_bytes, ExitKind, MAX_OUTPUT_BYTES,
};
use super::workspace::require_workspace_root;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;

static DANGEROUS_PATTERNS: &[(&str, &str)] = &[
    (
        "rm -rf /",
        "Refusing: rm -rf / would destroy the entire filesystem",
    ),
    (
        "rm -rf /*",
        "Refusing: rm -rf /* would destroy the entire filesystem",
    ),
    ("sudo ", "Refusing: sudo escalation is not allowed"),
    ("| sh", "Refusing: pipe-to-shell execution is not allowed"),
    ("| bash", "Refusing: pipe-to-shell execution is not allowed"),
    (
        "<(curl",
        "Refusing: process substitution remote execution is not allowed",
    ),
    (
        "<(wget",
        "Refusing: process substitution remote execution is not allowed",
    ),
    ("> /etc/", "Refusing: writing to /etc/ is not allowed"),
    (">> /etc/", "Refusing: writing to /etc/ is not allowed"),
    ("> /bin/", "Refusing: writing to /bin/ is not allowed"),
    (
        "> /usr/bin/",
        "Refusing: writing to /usr/bin/ is not allowed",
    ),
    (
        "of=/dev/sd",
        "Refusing: writing raw block devices is not allowed",
    ),
    (
        "> /dev/sd",
        "Refusing: writing raw block devices is not allowed",
    ),
    (
        "dd of=/dev/",
        "Refusing: writing raw block devices is not allowed",
    ),
    ("mkfs", "Refusing: mkfs formats filesystems"),
    (
        "dd if=",
        "Refusing: dd with if= can be dangerous; use with caution",
    ),
];

pub struct BashTool;

fn default_bash_timeout_secs() -> u64 {
    TOOL_CATALOG
        .get("Bash")
        .and_then(|def| def.default_timeout_secs)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

fn resolve_timeout_secs(input: &Value) -> u64 {
    // Input is `timeout` in milliseconds (aligned with claude-code-best).
    // Convert to seconds for internal consumption. Default in seconds.
    let ms = input
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| default_bash_timeout_secs() * 1000);
    let secs = ms.div_ceil(1000);
    secs.min(MAX_TIMEOUT_SECS)
}

fn tool_result_bash(content: String, data: Value) -> ToolResult {
    ToolResult {
        tool_name: "Bash".to_string(),
        content,
        data: Some(data),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

fn command_with_merged_stderr(command: &str) -> String {
    format!("{{\n{command}\n}} 2>&1")
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

#[async_trait]
impl RuntimeTool for BashTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("Bash")
            .unwrap_or_else(|| ToolDefinition::new("Bash", "Execute shell command"))
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
                tool_name: "Bash".to_string(),
                message: "Missing required field: command (string)".to_string(),
            }),
            Some(value) if !value.is_string() => Some(ToolError::InputValidationError {
                tool_name: "Bash".to_string(),
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
        ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        use crate::runtime::store::permission_store::PolicyDecision;

        let command = input.get("command").and_then(Value::as_str).unwrap_or("");
        for (pattern, message) in DANGEROUS_PATTERNS {
            if command.contains(pattern) {
                return Some(PermissionDecision::Deny {
                    message: (*message).to_string(),
                    reason: PermissionReason::Other("dangerous_pattern".to_string()),
                });
            }
        }

        if let Some(store) = ctx.permission_store.as_ref() {
            match store.get_for_command("Bash", command) {
                Some(PolicyDecision::AlwaysDeny) | Some(PolicyDecision::Deny) => {
                    return Some(PermissionDecision::Deny {
                        message: format!(
                            "Command blocked by stored CommandPattern policy: {}",
                            command.chars().take(80).collect::<String>()
                        ),
                        reason: PermissionReason::StoredPolicy,
                    });
                }
                Some(PolicyDecision::AlwaysAllow) | Some(PolicyDecision::Allow) => {
                    return Some(PermissionDecision::Allow {
                        updated_input: None,
                        reason: PermissionReason::StoredPolicy,
                    });
                }
                None => {}
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
        let timeout_secs = resolve_timeout_secs(&input);
        let shell_command = command_with_merged_stderr(&command);

        let mut shell = Command::new("/bin/sh");
        configure_child_process_group(&mut shell);
        let mut child = shell
            .arg("-c")
            .arg(&shell_command)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_timeout_secs_prefers_input_override() {
        // 5000ms = 5s
        assert_eq!(resolve_timeout_secs(&json!({ "timeout": 5000 })), 5);
    }

    #[test]
    fn resolve_timeout_secs_falls_back_to_catalog_default() {
        let expected = TOOL_CATALOG
            .get("Bash")
            .and_then(|def| def.default_timeout_secs)
            .expect("bash should declare default timeout");
        assert_eq!(default_bash_timeout_secs(), expected);
        assert_eq!(resolve_timeout_secs(&json!({})), expected);
    }

    #[test]
    fn resolve_timeout_secs_caps_large_values() {
        // 9_999_000ms would be 9999s, capped to MAX (600s)
        assert_eq!(resolve_timeout_secs(&json!({ "timeout": 9_999_000 })), 600);
    }

    #[test]
    fn resolve_timeout_secs_rounds_up_subsecond_ms() {
        // 1500ms should round up to 2s (don't truncate to 1s)
        assert_eq!(resolve_timeout_secs(&json!({ "timeout": 1500 })), 2);
    }
}
