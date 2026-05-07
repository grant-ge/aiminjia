//! PowerShellTool — execute PowerShell commands inside the authorized workspace.
//! Windows-only. Prefers pwsh.exe (7+ Core, supports `&&`/`||`) over
//! powershell.exe (5.1 Desktop, no chain operators).

#![cfg(windows)]

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
use crate::storage::process_ext::NoWindowExt;

use super::powershell_detect::{detect, PowerShellLocation};
use super::shell_common::{
    collect_reader, content_from_output, format_cancel_message, format_command_failure,
    interpret_command_result, kill_child_process_tree, read_merged_streams, truncated_to_max_bytes,
    wait_for_cancellation, ExitKind, MAX_OUTPUT_BYTES,
};
use super::workspace::require_workspace_root;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;

/// Case-insensitive substring patterns. The match logic is
/// `command.to_lowercase().contains(pattern_lc)`. Patterns are stored already
/// lower-cased to avoid an allocation per pattern on every check.
static DANGEROUS_PATTERNS: &[(&str, &str)] = &[
    (
        "remove-item c:\\windows",
        "Refusing: removing C:\\Windows would brick the OS",
    ),
    (
        "remove-item -path c:\\windows",
        "Refusing: removing C:\\Windows would brick the OS",
    ),
    (
        "remove-item c:\\program files",
        "Refusing: removing Program Files is not allowed",
    ),
    (
        "remove-item -path c:\\program files",
        "Refusing: removing Program Files is not allowed",
    ),
    ("format-volume", "Refusing: Format-Volume erases data"),
    ("clear-disk", "Refusing: Clear-Disk wipes a disk"),
    ("initialize-disk", "Refusing: Initialize-Disk wipes a disk"),
    (
        "stop-computer",
        "Refusing: Stop-Computer shuts down the machine",
    ),
    (
        "restart-computer",
        "Refusing: Restart-Computer reboots the machine",
    ),
    (
        "| invoke-expression",
        "Refusing: pipe-to-Invoke-Expression is remote code execution",
    ),
    ("| iex", "Refusing: pipe-to-iex is remote code execution"),
    (
        ").downloadstring(",
        "Refusing: WebClient.DownloadString followed by execution is RCE",
    ),
];

pub struct PowerShellTool;

fn default_powershell_timeout_secs() -> u64 {
    TOOL_CATALOG
        .get("powershell")
        .and_then(|def| def.default_timeout_secs)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

fn resolve_timeout_secs(input: &Value) -> u64 {
    input
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or_else(default_powershell_timeout_secs)
        .min(MAX_TIMEOUT_SECS)
}

fn tool_result_powershell(content: String, data: Value) -> ToolResult {
    ToolResult {
        tool_name: "powershell".to_string(),
        content,
        data: Some(data),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

#[async_trait]
impl RuntimeTool for PowerShellTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("powershell")
            .unwrap_or_else(|| ToolDefinition::new("powershell", "Execute PowerShell command"))
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
                tool_name: "powershell".to_string(),
                message: "Missing required field: command (string)".to_string(),
            }),
            Some(value) if !value.is_string() => Some(ToolError::InputValidationError {
                tool_name: "powershell".to_string(),
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
        let lc = command.to_lowercase();
        for (pattern_lc, message) in DANGEROUS_PATTERNS {
            if lc.contains(pattern_lc) {
                return Some(PermissionDecision::Deny {
                    message: (*message).to_string(),
                    reason: PermissionReason::Other("dangerous_pattern".to_string()),
                });
            }
        }

        if let Some(store) = ctx.permission_store.as_ref() {
            match store.get_for_command("powershell", command) {
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

        let location: PowerShellLocation = detect().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "PowerShell not found on this system. Install PowerShell 7 or ensure powershell.exe is on PATH.".into(),
            )
        })?;

        // Best-effort UTF-8 setup before running the user command. Managed
        // Windows hosts may run PowerShell in ConstrainedLanguage mode, where
        // OutputEncoding property setters fail; those setup failures must not
        // pollute output or block the actual command.
        let wrapped_command = format!(
            "chcp 65001 > $null 2>$null; \
             & {{ try {{ [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false) }} catch {{ }} }} 2>$null; \
             & {{ try {{ $OutputEncoding = [System.Text.UTF8Encoding]::new($false) }} catch {{ }} }} 2>$null; \
             {command}"
        );

        let mut child = Command::new(&location.path)
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(&wrapped_command)
            .current_dir(&root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .no_window()
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn PowerShell: {e}")))?;

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
                Ok(tool_result_powershell(
                    content,
                    json!({
                        "command": command,
                        "exit_code": exit_code,
                        "stdout_stderr": combined_output,
                        "truncated": truncated,
                        "semantic_message": semantics.message,
                        "shell_path": location.path.display().to_string(),
                        "edition": format!("{:?}", location.edition),
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

    #[test]
    fn resolve_timeout_secs_prefers_input_override() {
        assert_eq!(resolve_timeout_secs(&json!({ "timeout_secs": 5 })), 5);
    }

    #[test]
    fn resolve_timeout_secs_caps_large_values() {
        assert_eq!(resolve_timeout_secs(&json!({ "timeout_secs": 9999 })), 600);
    }

    #[test]
    fn resolve_timeout_secs_falls_back_to_catalog_default() {
        let expected = TOOL_CATALOG
            .get("powershell")
            .and_then(|def| def.default_timeout_secs)
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        assert_eq!(resolve_timeout_secs(&json!({})), expected);
    }
}
