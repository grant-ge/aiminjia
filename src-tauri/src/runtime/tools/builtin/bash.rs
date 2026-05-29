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

use super::shell_common::{
    collect_reader, content_from_output, emit_shell_failure_diagnostic, format_cancel_message,
    format_command_failure, inject_bundled_runtime_path, interpret_command_result,
    kill_child_process_tree, read_merged_streams_with_progress, tail_n_lines,
    truncated_to_max_bytes, ExitKind, MAX_OUTPUT_BYTES,
};
use super::workspace::require_workspace_root;
use crate::runtime::cancellation::wait_for_cancellation;

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
    fn id(&self) -> &str {
        "Bash"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
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
        inject_bundled_runtime_path(&ctx, &mut shell);

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

        // Live progress wiring. The IO loop writes the latest (captured,
        // total_bytes) tuple into a tokio::sync::watch channel; a separate
        // throttling task pulls from it every PROGRESS_TICK_MS, decodes the
        // last N lines of stdout/stderr, and pushes that snapshot to the
        // tool_progress_sink (if present). Watch semantics give us natural
        // coalescing: when the bash command vomits 100 lines in 10ms, the
        // throttler only sends *one* progress event with the latest tail —
        // no per-line spam on the bus.
        const PROGRESS_TICK_MS: u64 = 500;
        const PROGRESS_TAIL_LINES: usize = 20;
        // Hard cap on the tail string to protect against pathological lines
        // (a 100 MB single-line minified payload). 8 KB == ~200 80-col lines.
        const PROGRESS_TAIL_MAX_BYTES: usize = 8 * 1024;

        let progress_sink = ctx
            .capability
            .as_ref()
            .and_then(|cap| cap.tool_progress_sink.clone());
        let tool_call_id_for_progress = ctx.tool_call_id.as_str().to_string();

        let (progress_tx, mut progress_rx) =
            tokio::sync::watch::channel::<(Vec<u8>, u64)>((Vec::new(), 0));

        let progress_task = if let Some(sink) = progress_sink.clone() {
            let tool_call_id = tool_call_id_for_progress.clone();
            Some(tokio::spawn(async move {
                let mut last_sent_bytes: u64 = 0;
                let mut ticker = tokio::time::interval(Duration::from_millis(PROGRESS_TICK_MS));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        // watch sender drop → command finished, exit the loop.
                        changed = progress_rx.changed() => {
                            if changed.is_err() {
                                break;
                            }
                        }
                        _ = ticker.tick() => {}
                    }
                    let (captured, total_bytes) = progress_rx.borrow().clone();
                    if total_bytes == last_sent_bytes {
                        continue;
                    }
                    last_sent_bytes = total_bytes;
                    // Decode for tail extraction only; the original bytes are
                    // already kept in `captured` for the final tool result.
                    let decoded = crate::storage::console_decode::decode_console_bytes(&captured);
                    let mut tail = tail_n_lines(&decoded, PROGRESS_TAIL_LINES);
                    if tail.len() > PROGRESS_TAIL_MAX_BYTES {
                        // UTF-8 safe truncation: walk back to a char boundary.
                        let mut end = PROGRESS_TAIL_MAX_BYTES;
                        while end > 0 && !tail.is_char_boundary(end) {
                            end -= 1;
                        }
                        // Trim from the start so we keep the *most recent* bytes.
                        let drop = tail.len() - end;
                        tail = tail[drop..].to_string();
                    }
                    sink.on_progress(&tool_call_id, &tail, total_bytes);
                }
            }))
        } else {
            None
        };

        // The IO task writes into the watch channel on every appended chunk.
        // We move `progress_tx` into it so dropping the task naturally signals
        // the throttler to stop (Err on `changed()`).
        let merged_handle = tokio::spawn(async move {
            read_merged_streams_with_progress(stdout, stderr, |captured, total| {
                // try_send-equivalent for watch: send() never blocks; receivers
                // see the latest value on their next poll. We clone the buffer
                // because the read loop owns it — keeping a borrow alive across
                // an await point is not possible inside the callback.
                let _ = progress_tx.send((captured.to_vec(), total));
            })
            .await
        });

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

        // The merged_handle has finished — the watch sender it owned is now
        // dropped, so the throttler task will exit on its next `changed()`
        // poll. Wait briefly for that to happen so we don't leak the task or
        // race the final tool:completed event with one last stale tool:progress.
        if let Some(handle) = progress_task {
            let _ = tokio::time::timeout(Duration::from_millis(750), handle).await;
        }

        match exit_kind {
            ExitKind::Completed(status) => {
                let exit_code = status.code().unwrap_or(-1);
                let semantics = interpret_command_result(&command, exit_code);
                emit_shell_failure_diagnostic(
                    &ctx,
                    "bash",
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

#[cfg(test)]
mod progress_tests {
    use crate::runtime::tools::capability::ToolProgressSink;
    use crate::runtime::tools::{
        CapabilityContext, RuntimeTool, StorageCapability, ToolExecutionContext,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Debug, Default)]
    struct RecordingSink {
        events: Mutex<Vec<(String, String, u64)>>,
    }

    impl ToolProgressSink for RecordingSink {
        fn on_progress(&self, tool_call_id: &str, stdout_tail: &str, total_bytes: u64) {
            self.events.lock().unwrap().push((
                tool_call_id.to_string(),
                stdout_tail.to_string(),
                total_bytes,
            ));
        }
    }

    fn cap_with_sink(workspace: PathBuf, sink: Arc<RecordingSink>) -> Arc<CapabilityContext> {
        Arc::new(CapabilityContext {
            storage: Some(StorageCapability {
                workspace_path: workspace,
                authorized_workspace: None,
                permission_ctx: Arc::new(crate::runtime::path_auth::ToolPermissionContext::empty()),
            }),
            workspace_id: Some("ws-test".to_string()),
            runtime_resolver: None,
            file_ops: None,
            read_file_state: None,
            file_reading_limits: None,
            notification_sink: None,
            tool_progress_sink: Some(sink as Arc<dyn ToolProgressSink>),
            is_subagent: false,
        })
    }

    /// Long-running command (sleep 1.5s + N print bursts) should emit at least
    /// one progress event well before completion. The throttler ticks every
    /// 500ms, so over ~1.5s we expect 2-3 emits and a final one near 1500ms.
    #[tokio::test]
    #[ignore = "shells out and sleeps ~1.5s — keep out of fast unit run"]
    async fn long_running_command_emits_progress_before_completion() {
        let tmp = TempDir::new().unwrap();
        let sink = Arc::new(RecordingSink::default());
        let ctx = ToolExecutionContext::for_test("conv-bp", "run-bp", "tc-bp")
            .with_capability(cap_with_sink(tmp.path().to_path_buf(), sink.clone()));
        let tool = super::BashTool;
        let cmd = "for i in 1 2 3; do echo line-$i; sleep 0.6; done";
        RuntimeTool::execute(&tool, json!({ "command": cmd }), ctx)
            .await
            .expect("bash command should succeed");

        let events = sink.events.lock().unwrap().clone();
        assert!(
            !events.is_empty(),
            "expected at least one tool:progress event before completion"
        );
        // Each event must carry the correct tool_call_id
        for (tool_call_id, _tail, _bytes) in &events {
            assert_eq!(tool_call_id, "tc-bp");
        }
        // Total bytes must be monotonic.
        let mut prev_bytes = 0u64;
        for (_, _, bytes) in &events {
            assert!(*bytes >= prev_bytes, "total_bytes must not decrease");
            prev_bytes = *bytes;
        }
        // Last event should mention at least one of the printed lines.
        let last = &events.last().unwrap().1;
        assert!(
            last.contains("line-1") || last.contains("line-2") || last.contains("line-3"),
            "last tail should contain one of the printed lines, got: {last}",
        );
    }

    /// Fast command (`echo hi`) finishes in < 500ms. The throttler may emit a
    /// final tick or skip emit entirely — either is acceptable as long as the
    /// command result is correct and no panic happens.
    #[tokio::test]
    async fn fast_command_does_not_panic_with_progress_sink() {
        let tmp = TempDir::new().unwrap();
        let sink = Arc::new(RecordingSink::default());
        let ctx = ToolExecutionContext::for_test("conv-fast", "run-fast", "tc-fast")
            .with_capability(cap_with_sink(tmp.path().to_path_buf(), sink.clone()));
        let tool = super::BashTool;
        let result = RuntimeTool::execute(&tool, json!({ "command": "echo hi" }), ctx)
            .await
            .expect("fast bash should succeed");
        assert!(result.content.contains("hi"));
        // Don't assert on event count — fast commands may finish before the
        // first 500ms tick. The contract is "won't panic" + "tail correct if
        // emitted".
        for (tcid, _, _) in sink.events.lock().unwrap().iter() {
            assert_eq!(tcid, "tc-fast");
        }
    }
}
