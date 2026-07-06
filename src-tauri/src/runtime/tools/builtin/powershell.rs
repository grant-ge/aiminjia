//! PowerShellTool — execute PowerShell commands inside the authorized workspace.
//! Windows-only. Prefers pwsh.exe (7+ Core, supports `&&`/`||`) over
//! powershell.exe (5.1 Desktop, no chain operators).

#![cfg(windows)]

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

use crate::runtime::agent::async_task_store::AsyncAgentTaskStore;
use crate::runtime::agent::task_notification::TaskNotificationQueue;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;
use crate::storage::process_ext::NoWindowExt;

use super::powershell_detect::{detect, PowerShellEdition, PowerShellLocation};
use super::shell_common::{
    append_reader_fallback_notice, auto_loaded_skill_install_deny_message, collect_reader_bounded,
    content_from_output, emit_shell_failure_diagnostic, format_cancel_message,
    format_command_failure, inject_managed_runtime_env, inject_trace_env, interpret_command_result,
    kill_child_process_tree, optional_transcript_path,
    read_merged_streams_with_progress_and_optional_transcript, reader_drain_timeout,
    truncated_to_max_bytes, ExitKind, ReaderSnapshot, MAX_OUTPUT_BYTES,
};
use super::workspace::require_workspace_root;
use crate::runtime::cancellation::wait_for_cancellation;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
const AUTO_BACKGROUND_AFTER_SECS: u64 = 10;

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

fn dangerous_command_view(command: &str) -> String {
    command
        .to_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, '\'' | '"' | '`'))
        .collect()
}

#[derive(Clone, Default)]
pub struct PowerShellTool {
    background: Option<super::shell_task::ShellBackgroundDeps>,
}

impl PowerShellTool {
    pub fn new(
        task_store: Arc<AsyncAgentTaskStore>,
        notifications: Arc<TaskNotificationQueue>,
    ) -> Self {
        Self {
            background: Some(super::shell_task::ShellBackgroundDeps::new(
                task_store,
                notifications,
            )),
        }
    }
}

fn default_powershell_timeout_secs() -> u64 {
    TOOL_CATALOG
        .get("PowerShell")
        .and_then(|def| def.default_timeout_secs)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

fn resolve_timeout_secs(input: &Value) -> u64 {
    // Input is `timeout` in milliseconds (aligned with claude-code-best).
    // Convert to seconds for internal consumption.
    let ms = input
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| default_powershell_timeout_secs() * 1000);
    let secs = ms.div_ceil(1000);
    secs.min(MAX_TIMEOUT_SECS)
}

fn edition_guidance(edition: Option<PowerShellEdition>) -> &'static str {
    match edition {
        Some(PowerShellEdition::Desktop) => {
            "PowerShell edition: Windows PowerShell 5.1 (powershell.exe)\
            \n- `&&` 和 `||` 不可用，会触发 parser error；B 只在 A 成功后运行请写 `A; if ($?) { B }`，无条件顺序执行请写 `A; B`。\
            \n- 三元运算符 `?:`、null-coalescing `??`、null-conditional `?.` 不可用；请使用 `if/else` 和显式 `$null -eq` 判断。\
            \n- 避免在原生 exe/cmd 后追加 `2>&1`。PowerShell 5.1 会把 native stderr 包成 ErrorRecord (NativeCommandError)，即使进程 exit code 为 0 也可能让 `$?` 变成 false；本工具已经捕获 stderr。\
            \n- `Out-File` / `Set-Content` 默认写 UTF-16 LE (with BOM)；写给其他工具读取的文件时请显式传 `-Encoding utf8`。\
            \n- `ConvertFrom-Json` 返回 PSCustomObject，不是 hashtable；`-AsHashtable` 不可用。\
            \n- 本工具使用 `-NonInteractive`，不要使用 `Read-Host`、`pause`、`Get-Credential`、`Out-GridView` 或 `$Host.UI.PromptForChoice`。"
        }
        Some(PowerShellEdition::Core) => {
            "PowerShell edition: PowerShell 7+ (pwsh)\
            \n- `&&` 和 `||` 可用；当 B 只应在 A 成功后运行时，优先使用 `A && B`。\
            \n- 三元运算符 `$cond ? $a : $b`、null-coalescing `??`、null-conditional `?.` 可用。\
            \n- 默认文件编码是 UTF-8 without BOM。\
            \n- 本工具使用 `-NonInteractive`，不要使用 `Read-Host`、`pause`、`Get-Credential`、`Out-GridView` 或 `$Host.UI.PromptForChoice`。"
        }
        None => {
            "PowerShell edition: unknown — assume Windows PowerShell 5.1 for compatibility\
            \n- 不要使用 `&&`、`||`、三元运算符 `?:`、null-coalescing `??` 或 null-conditional `?.`。\
            \n- 条件串联请写 `A; if ($?) { B }`，无条件顺序执行请写 `A; B`。\
            \n- 避免在原生 exe/cmd 后追加 `2>&1`；本工具已经捕获 stderr。\
            \n- 写给其他工具读取的文件时请显式传 `-Encoding utf8`。\
            \n- 本工具使用 `-NonInteractive`，不要使用 `Read-Host`、`pause`、`Get-Credential`、`Out-GridView` 或 `$Host.UI.PromptForChoice`。"
        }
    }
}

fn resolve_auto_background_after_secs(input: &Value, timeout_secs: u64) -> Option<u64> {
    let secs = AUTO_BACKGROUND_AFTER_SECS;
    #[cfg(test)]
    let secs = if let Some(ms) = input
        .get("_auto_background_after_ms")
        .and_then(Value::as_u64)
    {
        ms.div_ceil(1000).max(1)
    } else {
        secs
    };
    #[cfg(not(test))]
    {
        let _ = input;
    }
    (secs < timeout_secs).then_some(secs)
}

fn tool_result_powershell(content: String, data: Value) -> ToolResult {
    ToolResult {
        tool_name: "PowerShell".to_string(),
        content,
        data: Some(data),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellPathAccess {
    path: PathBuf,
    op: crate::runtime::path_auth::PathOp,
}

fn shell_separator(token: &str) -> bool {
    matches!(token, ";" | "&&" | "||" | "|")
}

fn command_name(token: &str) -> String {
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(token)
        .to_ascii_lowercase()
}

fn clean_path_token(token: &str) -> Option<String> {
    let token = token
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ';' | ',' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}'
                )
        })
        .trim();
    if token.is_empty()
        || token == "-"
        || token.starts_with('-')
        || token.contains("://")
        || token.starts_with("$(")
    {
        return None;
    }
    Some(token.to_string())
}

fn resolve_shell_path_token(token: &str, root: &Path) -> Option<PathBuf> {
    let token = clean_path_token(token)?;
    let token = token.strip_prefix("file://").unwrap_or(&token);
    let path = if token == "~" {
        dirs::home_dir()?
    } else if let Some(rest) = token
        .strip_prefix("~/")
        .or_else(|| token.strip_prefix("~\\"))
    {
        dirs::home_dir()?.join(rest)
    } else if let Some(rest) = token
        .strip_prefix("$HOME/")
        .or_else(|| token.strip_prefix("$HOME\\"))
        .or_else(|| token.strip_prefix("$env:USERPROFILE/"))
        .or_else(|| token.strip_prefix("$env:USERPROFILE\\"))
    {
        dirs::home_dir()?.join(rest)
    } else {
        let path = PathBuf::from(token);
        if path.is_absolute() || token.starts_with("\\\\") {
            path
        } else if path.starts_with(".")
            || path.starts_with("..")
            || token.contains('\\')
            || token.contains('/')
        {
            root.join(path)
        } else {
            return None;
        }
    };
    Some(path)
}

fn push_access(
    accesses: &mut Vec<ShellPathAccess>,
    token: &str,
    root: &Path,
    op: crate::runtime::path_auth::PathOp,
) {
    if let Some(path) = resolve_shell_path_token(token, root) {
        accesses.push(ShellPathAccess { path, op });
    }
}

fn shell_command_path_accesses(command: &str, root: &Path) -> Vec<ShellPathAccess> {
    use crate::runtime::path_auth::PathOp;

    let tokens = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut accesses = Vec::new();

    for (idx, token) in tokens.iter().enumerate() {
        if matches!(
            token.to_ascii_lowercase().as_str(),
            ">" | ">>" | "2>" | "2>>" | "-filepath" | "-literalpath" | "-path"
        ) {
            if let Some(next) = tokens.get(idx + 1) {
                let op = if token == ">" || token == ">>" || token == "2>" || token == "2>>" {
                    PathOp::Write
                } else {
                    PathOp::Read
                };
                push_access(&mut accesses, next, root, op);
            }
            continue;
        }
        if let Some(path) = token.strip_prefix(">>").or_else(|| token.strip_prefix('>')) {
            push_access(&mut accesses, path, root, PathOp::Write);
        }
    }

    let mut idx = 0;
    while idx < tokens.len() {
        let token = &tokens[idx];
        if shell_separator(token) {
            idx += 1;
            continue;
        }
        let command = command_name(token);
        let op = if matches!(
            command.as_str(),
            "remove-item" | "rm" | "del" | "erase" | "rmdir" | "rd"
        ) {
            Some(PathOp::Delete)
        } else if matches!(
            command.as_str(),
            "set-content"
                | "add-content"
                | "out-file"
                | "new-item"
                | "copy-item"
                | "move-item"
                | "ni"
                | "sc"
                | "ac"
        ) {
            Some(PathOp::Write)
        } else if matches!(
            command.as_str(),
            "get-content"
                | "gc"
                | "type"
                | "select-string"
                | "get-childitem"
                | "get-childitems"
                | "dir"
                | "ls"
                | "test-path"
        ) {
            Some(PathOp::Read)
        } else {
            None
        };

        let Some(default_op) = op else {
            idx += 1;
            continue;
        };
        let mut segment_paths = Vec::new();
        let mut j = idx + 1;
        while j < tokens.len() && !shell_separator(&tokens[j]) {
            if let Some(path) = resolve_shell_path_token(&tokens[j], root) {
                segment_paths.push(path);
            }
            j += 1;
        }
        if command == "copy-item" || command == "move-item" {
            if let Some((last, sources)) = segment_paths.split_last() {
                for source in sources {
                    accesses.push(ShellPathAccess {
                        path: source.clone(),
                        op: PathOp::Read,
                    });
                }
                accesses.push(ShellPathAccess {
                    path: last.clone(),
                    op: PathOp::Write,
                });
            }
        } else {
            for path in segment_paths {
                accesses.push(ShellPathAccess {
                    path,
                    op: default_op,
                });
            }
        }
        idx = j;
    }

    accesses
}

fn path_auth_scope(canonical: &Path, op: crate::runtime::path_auth::PathOp) -> String {
    let scope_path = if canonical.is_dir() {
        canonical.to_path_buf()
    } else {
        canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| canonical.to_path_buf())
    };
    match op {
        crate::runtime::path_auth::PathOp::Read => format!("path:{}", scope_path.display()),
        crate::runtime::path_auth::PathOp::Write => format!("pathwrite:{}", scope_path.display()),
        crate::runtime::path_auth::PathOp::Delete => {
            format!("pathdelete:{}", scope_path.display())
        }
    }
}

fn command_path_permission_decision(
    command: &str,
    ctx: &ToolExecutionContext,
) -> Option<PermissionDecision> {
    let root = require_workspace_root(ctx).ok()?;
    let storage = ctx.capability.as_ref()?.storage.as_ref()?;
    let perm_ctx = storage.permission_ctx.as_ref();
    let effective_ctx;
    let ctx_ref = if perm_ctx.primary_root.is_none() {
        effective_ctx = crate::runtime::path_auth::ToolPermissionContext {
            primary_root: Some(root.clone()),
            ..(*perm_ctx).clone()
        };
        &effective_ctx
    } else {
        perm_ctx
    };

    for access in shell_command_path_accesses(command, &root) {
        let canonical = crate::runtime::path_auth::decide::canonicalize_or_ancestor(&access.path)
            .unwrap_or(access.path);
        match crate::runtime::path_auth::decide::is_path_allowed(&canonical, access.op, ctx_ref) {
            crate::runtime::path_auth::Decision::Allow => {}
            crate::runtime::path_auth::Decision::Deny(message) => {
                return Some(PermissionDecision::Deny {
                    message,
                    reason: PermissionReason::Capability,
                });
            }
            crate::runtime::path_auth::Decision::Ask { reason } => {
                return Some(PermissionDecision::Ask {
                    message: reason,
                    suggestions: vec!["仅本次允许".into(), "永久允许".into(), "拒绝".into()],
                    remember_options: vec![
                        crate::runtime::tools::permission::PermissionDestination::Session,
                        crate::runtime::tools::permission::PermissionDestination::User,
                    ],
                    default_destination: Some(
                        crate::runtime::tools::permission::PermissionDestination::Session,
                    ),
                    reason: PermissionReason::Capability,
                    path_auth_scope: Some(path_auth_scope(&canonical, access.op)),
                });
            }
        }
    }
    None
}

#[async_trait]
impl RuntimeTool for PowerShellTool {
    fn id(&self) -> &str {
        "PowerShell"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        let mut definition = TOOL_CATALOG
            .get("PowerShell")
            .unwrap_or_else(|| ToolDefinition::new("PowerShell", "Execute PowerShell command"));
        let edition = detect().map(|location| location.edition);
        definition.description = format!(
            "{}\n\n{}",
            definition.description,
            edition_guidance(edition)
        );
        definition
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
                tool_name: "PowerShell".to_string(),
                message: "Missing required field: command (string)".to_string(),
            }),
            Some(value) if !value.is_string() => Some(ToolError::InputValidationError {
                tool_name: "PowerShell".to_string(),
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
        let lc = dangerous_command_view(command);
        for (pattern_lc, message) in DANGEROUS_PATTERNS {
            if lc.contains(pattern_lc) {
                return Some(PermissionDecision::Deny {
                    message: (*message).to_string(),
                    reason: PermissionReason::Other("dangerous_pattern".to_string()),
                });
            }
        }
        if let Some(message) = auto_loaded_skill_install_deny_message(command) {
            return Some(PermissionDecision::Deny {
                message,
                reason: PermissionReason::Other("auto_loaded_skill_directory".to_string()),
            });
        }

        if let Some(decision) = command_path_permission_decision(command, ctx) {
            return Some(decision);
        }

        if let Some(store) = ctx.permission_store.as_ref() {
            match store.get_for_command("PowerShell", command) {
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
        let run_in_background = input
            .get("run_in_background")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let description = input
            .get("description")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&command)
            .to_string();

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

        let mut shell = Command::new(&location.path);
        shell
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(&wrapped_command)
            .current_dir(&root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .no_window();
        inject_managed_runtime_env(&ctx, &mut shell);
        inject_trace_env(&mut shell);
        let mut child = shell
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

        if run_in_background {
            let background = self.background.clone().ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "PowerShell background execution is unavailable in this context".into(),
                )
            })?;
            return super::shell_task::launch_background_shell_task(
                "PowerShell",
                "powershell",
                command,
                description,
                timeout_secs,
                ctx,
                background,
                child,
                stdout,
                stderr,
            );
        }
        let transcript_switch = optional_transcript_path();
        let transcript_switch_for_reader = transcript_switch.clone();
        let captured_snapshot = ReaderSnapshot::new();
        let captured_snapshot_for_reader = captured_snapshot.clone();
        let merged_handle =
            tokio::spawn(read_merged_streams_with_progress_and_optional_transcript(
                stdout,
                stderr,
                move |captured, _| {
                    captured_snapshot_for_reader.set(captured);
                },
                Some(transcript_switch_for_reader),
            ));

        enum ForegroundControl {
            Exit(ExitKind),
            AutoBackground,
        }

        let auto_background_after_secs = resolve_auto_background_after_secs(&input, timeout_secs);
        let auto_background_delay_secs = auto_background_after_secs.unwrap_or(timeout_secs);
        let auto_background_enabled = auto_background_after_secs.is_some()
            && self.background.is_some()
            && ctx.conv_dir.is_some();

        let foreground_control = tokio::select! {
            status = child.wait() => {
                ForegroundControl::Exit(ExitKind::Completed(
                    status.map_err(|e| ToolError::ExecutionFailed(format!("Failed waiting for process: {e}")))?
                ))
            }
            _ = tokio::time::sleep(Duration::from_secs(auto_background_delay_secs)), if auto_background_enabled => {
                ForegroundControl::AutoBackground
            }
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
                kill_child_process_tree(&mut child).await;
                ForegroundControl::Exit(ExitKind::TimedOut)
            }
            reason = wait_for_cancellation(ctx.cancellation.clone()) => {
                kill_child_process_tree(&mut child).await;
                ForegroundControl::Exit(ExitKind::Cancelled(reason))
            }
        };

        let exit_kind = match foreground_control {
            ForegroundControl::AutoBackground => {
                let background = self.background.clone().ok_or_else(|| {
                    ToolError::ExecutionFailed(
                        "PowerShell background execution is unavailable in this context".into(),
                    )
                })?;
                let remaining_timeout_secs = timeout_secs
                    .saturating_sub(auto_background_after_secs.unwrap_or(timeout_secs))
                    .max(1);
                let pre_background_output = captured_snapshot.get();
                return super::shell_task::launch_auto_backgrounded_shell_task(
                    "PowerShell",
                    "powershell",
                    command,
                    description,
                    remaining_timeout_secs,
                    ctx,
                    background,
                    child,
                    merged_handle,
                    captured_snapshot,
                    transcript_switch,
                    pre_background_output,
                );
            }
            ForegroundControl::Exit(exit_kind) => exit_kind,
        };

        let reader = collect_reader_bounded(
            merged_handle,
            captured_snapshot,
            reader_drain_timeout(&exit_kind),
        )
        .await?;
        let (combined_output, combined_truncated) =
            truncated_to_max_bytes(&reader.output, MAX_OUTPUT_BYTES);
        let truncated = reader.truncated || combined_truncated;

        match exit_kind {
            ExitKind::Completed(status) => {
                let exit_code = status.code().unwrap_or(-1);
                let semantics = interpret_command_result(&command, exit_code);
                emit_shell_failure_diagnostic(
                    &ctx,
                    "powershell",
                    &command,
                    exit_code,
                    &combined_output,
                    semantics.is_error,
                );
                if semantics.is_error {
                    return Err(ToolError::ExecutionFailed(format_command_failure(
                        &command,
                        exit_code,
                        &combined_output,
                        semantics.message,
                    )));
                }

                let content = append_reader_fallback_notice(
                    content_from_output(&combined_output, semantics.message),
                    &reader,
                );
                Ok(tool_result_powershell(
                    content,
                    json!({
                        "command": command,
                        "exit_code": exit_code,
                        "stdout_stderr": combined_output,
                        "truncated": truncated,
                        "stream_timed_out": reader.stream_timed_out,
                        "reader_aborted": reader.reader_aborted,
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
        // 5000ms = 5s
        assert_eq!(resolve_timeout_secs(&json!({ "timeout": 5000 })), 5);
    }

    #[test]
    fn resolve_timeout_secs_caps_large_values() {
        // 9_999_000ms would be 9999s, capped to MAX (600s)
        assert_eq!(resolve_timeout_secs(&json!({ "timeout": 9_999_000 })), 600);
    }

    #[test]
    fn resolve_auto_background_after_secs_uses_test_override() {
        assert_eq!(
            resolve_auto_background_after_secs(
                &json!({ "_auto_background_after_ms": 100, "timeout": 5000 }),
                5
            ),
            Some(1)
        );
    }

    #[test]
    fn resolve_timeout_secs_falls_back_to_catalog_default() {
        let expected = TOOL_CATALOG
            .get("PowerShell")
            .and_then(|def| def.default_timeout_secs)
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        assert_eq!(resolve_timeout_secs(&json!({})), expected);
    }

    #[test]
    fn edition_guidance_matches_detected_powershell_edition() {
        let desktop = edition_guidance(Some(PowerShellEdition::Desktop));
        assert!(desktop.contains("Windows PowerShell 5.1"));
        assert!(desktop.contains("`&&` 和 `||` 不可用"));
        assert!(desktop.contains("避免在原生 exe/cmd 后追加 `2>&1`"));

        let core = edition_guidance(Some(PowerShellEdition::Core));
        assert!(core.contains("PowerShell 7+"));
        assert!(core.contains("`&&` 和 `||` 可用"));
        assert!(core.contains("UTF-8 without BOM"));

        let unknown = edition_guidance(None);
        assert!(unknown.contains("assume Windows PowerShell 5.1"));
        assert!(unknown.contains("避免在原生 exe/cmd 后追加 `2>&1`"));
    }
}
