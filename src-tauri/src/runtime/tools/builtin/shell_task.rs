//! Background shell task lifecycle shared by BashTool and PowerShellTool.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout};
use tokio::task::JoinHandle;

use crate::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState, AsyncTaskType,
};
use crate::runtime::agent::output_writer::{self, TranscriptLine};
use crate::runtime::agent::task_notification::{
    build_task_notification_xml, TaskNotificationQueue,
};
use crate::runtime::cancellation::{wait_for_cancellation, CancellationReason, CancellationToken};
use crate::runtime::ids::AgentId;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::executor::{ToolError, ToolResult};

use super::shell_common::{
    append_transcript_bytes, collect_reader, content_from_output, emit_shell_failure_diagnostic,
    enable_optional_transcript_path, format_cancel_message, format_command_failure,
    interpret_command_result, kill_child_process_tree, truncated_to_max_bytes, ExitKind,
    OptionalTranscriptPath, MAX_OUTPUT_BYTES,
};

pub const LOCAL_SHELL_TASK_TYPE: &str = "local_bash";

#[derive(Clone)]
pub struct ShellBackgroundDeps {
    pub store: Arc<AsyncAgentTaskStore>,
    pub notifications: Arc<TaskNotificationQueue>,
}

impl ShellBackgroundDeps {
    pub fn new(store: Arc<AsyncAgentTaskStore>, notifications: Arc<TaskNotificationQueue>) -> Self {
        Self {
            store,
            notifications,
        }
    }
}

pub fn shell_transcript_path(conv_dir: &Path, task_id: &str) -> PathBuf {
    conv_dir.join("tasks").join(format!("{task_id}.jsonl"))
}

fn generate_shell_task_id() -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    format!("b{}", &raw[..8])
}

pub fn launch_background_shell_task(
    tool_name: &'static str,
    diagnostic_tool_name: &'static str,
    command: String,
    description: String,
    timeout_secs: u64,
    ctx: ToolExecutionContext,
    deps: ShellBackgroundDeps,
    mut child: Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
) -> Result<ToolResult, ToolError> {
    let conv_dir = ctx.conv_dir.as_ref().ok_or_else(|| {
        ToolError::ExecutionFailed(
            "background shell tasks require a conversation directory".to_string(),
        )
    })?;

    let task_id = generate_shell_task_id();
    let task_agent_id = AgentId::new(task_id.clone());
    let transcript_path = shell_transcript_path(conv_dir, &task_id);
    let cancel_token = CancellationToken::new();

    let _ = output_writer::append_line(
        &transcript_path,
        &TranscriptLine::tool_result(
            ctx.tool_call_id.as_str(),
            tool_name,
            format!("Background command started: {command}"),
        ),
    )
    .map_err(|e| log::warn!("[{tool_name} background] initial transcript append failed: {e}"));

    deps.store.register_anonymous_with_type(
        AsyncTaskHandle {
            agent_id: task_agent_id.clone(),
            state: AsyncTaskState::Running,
            output_file: transcript_path.clone(),
            description: description.clone(),
            cancel_token: cancel_token.clone(),
        },
        AsyncTaskType::LocalBash,
    );

    let store = deps.store.clone();
    let notifications = deps.notifications.clone();
    let parent_session_id = ctx.session_id.clone();
    let parent_run_id = Some(ctx.run_id.clone());
    let parent_tool_use_id = ctx.tool_call_id.as_str().to_string();
    let command_for_task = command.clone();
    let description_for_task = description.clone();
    let transcript_for_task = transcript_path.clone();
    let ctx_for_task = ctx.clone();
    let task_id_for_task = task_agent_id.clone();

    tokio::spawn(async move {
        let reader = tokio::spawn(read_merged_streams_to_transcript(
            stdout,
            stderr,
            transcript_for_task.clone(),
        ));

        let exit_kind = tokio::select! {
            status = child.wait() => {
                match status {
                    Ok(status) => ExitKind::Completed(status),
                    Err(e) => {
                        let message = format!("Failed waiting for process: {e}");
                        finish_shell_task(
                            &store,
                            &notifications,
                            &task_id_for_task,
                            &transcript_for_task,
                            &parent_session_id,
                            parent_run_id.clone(),
                            &parent_tool_use_id,
                            "failed",
                            &format!("Background command \"{description_for_task}\" failed"),
                            Some(&message),
                            true,
                        );
                        return;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
                kill_child_process_tree(&mut child).await;
                ExitKind::TimedOut
            }
            reason = wait_for_cancellation(cancel_token.clone()) => {
                kill_child_process_tree(&mut child).await;
                ExitKind::Cancelled(reason)
            }
        };

        let (combined_output, stream_truncated) = match collect_reader(reader).await {
            Ok(value) => value,
            Err(e) => {
                let message = e.to_string();
                finish_shell_task(
                    &store,
                    &notifications,
                    &task_id_for_task,
                    &transcript_for_task,
                    &parent_session_id,
                    parent_run_id.clone(),
                    &parent_tool_use_id,
                    "failed",
                    &format!("Background command \"{description_for_task}\" failed"),
                    Some(&message),
                    true,
                );
                return;
            }
        };
        let (combined_output, combined_truncated) =
            truncated_to_max_bytes(&combined_output, MAX_OUTPUT_BYTES);
        let truncated = stream_truncated || combined_truncated;

        match exit_kind {
            ExitKind::Completed(status) => {
                let exit_code = status.code().unwrap_or(-1);
                let semantics = interpret_command_result(&command_for_task, exit_code);
                emit_shell_failure_diagnostic(
                    &ctx_for_task,
                    diagnostic_tool_name,
                    &command_for_task,
                    exit_code,
                    &combined_output,
                    semantics.is_error,
                );

                if semantics.is_error {
                    let message = format_command_failure(
                        &command_for_task,
                        exit_code,
                        &combined_output,
                        semantics.message,
                    );
                    finish_shell_task(
                        &store,
                        &notifications,
                        &task_id_for_task,
                        &transcript_for_task,
                        &parent_session_id,
                        parent_run_id.clone(),
                        &parent_tool_use_id,
                        "failed",
                        &format!(
                            "Background command \"{description_for_task}\" failed with exit code {exit_code}"
                        ),
                        Some(&message),
                        true,
                    );
                    return;
                }

                let content = content_from_output(&combined_output, semantics.message);
                let final_message = if truncated {
                    format!("{content}\n[output truncated]")
                } else {
                    content
                };
                finish_shell_task(
                    &store,
                    &notifications,
                    &task_id_for_task,
                    &transcript_for_task,
                    &parent_session_id,
                    parent_run_id.clone(),
                    &parent_tool_use_id,
                    "completed",
                    &format!(
                        "Background command \"{description_for_task}\" completed (exit code {exit_code})"
                    ),
                    Some(&final_message),
                    true,
                );
            }
            ExitKind::TimedOut => {
                let message = format_command_failure(
                    &command_for_task,
                    124,
                    &combined_output,
                    Some(&format!("Command timed out after {timeout_secs}s")),
                );
                finish_shell_task(
                    &store,
                    &notifications,
                    &task_id_for_task,
                    &transcript_for_task,
                    &parent_session_id,
                    parent_run_id.clone(),
                    &parent_tool_use_id,
                    "failed",
                    &format!("Background command \"{description_for_task}\" timed out"),
                    Some(&message),
                    true,
                );
            }
            ExitKind::Cancelled(reason) => {
                let message = format_cancel_message(reason, &combined_output);
                let notify = reason != Some(CancellationReason::BackgroundStop);
                finish_shell_task(
                    &store,
                    &notifications,
                    &task_id_for_task,
                    &transcript_for_task,
                    &parent_session_id,
                    parent_run_id.clone(),
                    &parent_tool_use_id,
                    "killed",
                    &format!("Background command \"{description_for_task}\" was stopped"),
                    Some(&message),
                    notify,
                );
            }
        }
    });

    let body = json!({
        "status": "backgrounded",
        "task_id": task_id,
        "task_type": LOCAL_SHELL_TASK_TYPE,
        "command": command,
        "description": description,
        "output_file": transcript_path.to_string_lossy().to_string(),
    });
    Ok(ToolResult::new(tool_name, body.to_string(), Some(body)))
}

pub fn launch_auto_backgrounded_shell_task(
    tool_name: &'static str,
    diagnostic_tool_name: &'static str,
    command: String,
    description: String,
    timeout_secs: u64,
    ctx: ToolExecutionContext,
    deps: ShellBackgroundDeps,
    mut child: Child,
    reader: JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
    transcript_switch: OptionalTranscriptPath,
    pre_background_output: Vec<u8>,
) -> Result<ToolResult, ToolError> {
    let conv_dir = ctx.conv_dir.as_ref().ok_or_else(|| {
        ToolError::ExecutionFailed(
            "background shell tasks require a conversation directory".to_string(),
        )
    })?;

    let task_id = generate_shell_task_id();
    let task_agent_id = AgentId::new(task_id.clone());
    let transcript_path = shell_transcript_path(conv_dir, &task_id);
    let cancel_token = CancellationToken::new();

    let _ = output_writer::append_line(
        &transcript_path,
        &TranscriptLine::tool_result(
            ctx.tool_call_id.as_str(),
            tool_name,
            format!("Foreground command moved to background: {command}"),
        ),
    )
    .map_err(|e| log::warn!("[{tool_name} auto-background] initial transcript append failed: {e}"));

    let pre_background_flushed_bytes = if append_transcript_bytes(
        &transcript_path,
        &pre_background_output,
        "shell auto-background preflush",
    ) {
        pre_background_output.len()
    } else {
        0
    };
    enable_optional_transcript_path(
        &transcript_switch,
        transcript_path.clone(),
        pre_background_flushed_bytes,
    );

    deps.store.register_anonymous_with_type(
        AsyncTaskHandle {
            agent_id: task_agent_id.clone(),
            state: AsyncTaskState::Running,
            output_file: transcript_path.clone(),
            description: description.clone(),
            cancel_token: cancel_token.clone(),
        },
        AsyncTaskType::LocalBash,
    );

    let store = deps.store.clone();
    let notifications = deps.notifications.clone();
    let parent_session_id = ctx.session_id.clone();
    let parent_run_id = Some(ctx.run_id.clone());
    let parent_tool_use_id = ctx.tool_call_id.as_str().to_string();
    let command_for_task = command.clone();
    let description_for_task = description.clone();
    let transcript_for_task = transcript_path.clone();
    let ctx_for_task = ctx.clone();
    let task_id_for_task = task_agent_id.clone();
    let timeout_secs = timeout_secs.max(1);

    tokio::spawn(async move {
        let exit_kind = tokio::select! {
            status = child.wait() => {
                match status {
                    Ok(status) => ExitKind::Completed(status),
                    Err(e) => {
                        let message = format!("Failed waiting for process: {e}");
                        finish_shell_task(
                            &store,
                            &notifications,
                            &task_id_for_task,
                            &transcript_for_task,
                            &parent_session_id,
                            parent_run_id.clone(),
                            &parent_tool_use_id,
                            "failed",
                            &format!("Background command \"{description_for_task}\" failed"),
                            Some(&message),
                            true,
                        );
                        return;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
                kill_child_process_tree(&mut child).await;
                ExitKind::TimedOut
            }
            reason = wait_for_cancellation(cancel_token.clone()) => {
                kill_child_process_tree(&mut child).await;
                ExitKind::Cancelled(reason)
            }
        };

        let (combined_output, stream_truncated) = match collect_reader(reader).await {
            Ok(value) => value,
            Err(e) => {
                let message = e.to_string();
                finish_shell_task(
                    &store,
                    &notifications,
                    &task_id_for_task,
                    &transcript_for_task,
                    &parent_session_id,
                    parent_run_id.clone(),
                    &parent_tool_use_id,
                    "failed",
                    &format!("Background command \"{description_for_task}\" failed"),
                    Some(&message),
                    true,
                );
                return;
            }
        };
        let (combined_output, combined_truncated) =
            truncated_to_max_bytes(&combined_output, MAX_OUTPUT_BYTES);
        let truncated = stream_truncated || combined_truncated;

        match exit_kind {
            ExitKind::Completed(status) => {
                let exit_code = status.code().unwrap_or(-1);
                let semantics = interpret_command_result(&command_for_task, exit_code);
                emit_shell_failure_diagnostic(
                    &ctx_for_task,
                    diagnostic_tool_name,
                    &command_for_task,
                    exit_code,
                    &combined_output,
                    semantics.is_error,
                );

                if semantics.is_error {
                    let message = format_command_failure(
                        &command_for_task,
                        exit_code,
                        &combined_output,
                        semantics.message,
                    );
                    finish_shell_task(
                        &store,
                        &notifications,
                        &task_id_for_task,
                        &transcript_for_task,
                        &parent_session_id,
                        parent_run_id.clone(),
                        &parent_tool_use_id,
                        "failed",
                        &format!(
                            "Background command \"{description_for_task}\" failed with exit code {exit_code}"
                        ),
                        Some(&message),
                        true,
                    );
                    return;
                }

                let content = content_from_output(&combined_output, semantics.message);
                let final_message = if truncated {
                    format!("{content}\n[output truncated]")
                } else {
                    content
                };
                finish_shell_task(
                    &store,
                    &notifications,
                    &task_id_for_task,
                    &transcript_for_task,
                    &parent_session_id,
                    parent_run_id.clone(),
                    &parent_tool_use_id,
                    "completed",
                    &format!(
                        "Background command \"{description_for_task}\" completed (exit code {exit_code})"
                    ),
                    Some(&final_message),
                    true,
                );
            }
            ExitKind::TimedOut => {
                let message = format_command_failure(
                    &command_for_task,
                    124,
                    &combined_output,
                    Some(&format!("Command timed out after {timeout_secs}s")),
                );
                finish_shell_task(
                    &store,
                    &notifications,
                    &task_id_for_task,
                    &transcript_for_task,
                    &parent_session_id,
                    parent_run_id.clone(),
                    &parent_tool_use_id,
                    "failed",
                    &format!("Background command \"{description_for_task}\" timed out"),
                    Some(&message),
                    true,
                );
            }
            ExitKind::Cancelled(reason) => {
                let message = format_cancel_message(reason, &combined_output);
                let notify = reason != Some(CancellationReason::BackgroundStop);
                finish_shell_task(
                    &store,
                    &notifications,
                    &task_id_for_task,
                    &transcript_for_task,
                    &parent_session_id,
                    parent_run_id.clone(),
                    &parent_tool_use_id,
                    "killed",
                    &format!("Background command \"{description_for_task}\" was stopped"),
                    Some(&message),
                    notify,
                );
            }
        }
    });

    let body = json!({
        "status": "backgrounded",
        "task_id": task_id,
        "task_type": LOCAL_SHELL_TASK_TYPE,
        "assistant_auto_backgrounded": true,
        "command": command,
        "description": description,
        "output_file": transcript_path.to_string_lossy().to_string(),
    });
    Ok(ToolResult::new(tool_name, body.to_string(), Some(body)))
}

fn finish_shell_task(
    store: &AsyncAgentTaskStore,
    notifications: &TaskNotificationQueue,
    task_id: &AgentId,
    transcript_path: &Path,
    parent_session_id: &crate::runtime::ids::SessionId,
    parent_run_id: Option<crate::runtime::ids::RunId>,
    parent_tool_use_id: &str,
    status: &str,
    summary: &str,
    result: Option<&str>,
    notify: bool,
) {
    let line_result = match status {
        "completed" => TranscriptLine::assistant(summary),
        "killed" => TranscriptLine::failed(summary),
        _ => TranscriptLine::failed(summary),
    };
    if let Err(e) = output_writer::append_line(transcript_path, &line_result) {
        log::warn!(
            "[shell background {}] final transcript append failed: {}",
            task_id.as_str(),
            e
        );
    }

    if notify {
        let path = transcript_path.to_string_lossy();
        let xml = build_task_notification_xml(
            task_id.as_str(),
            Some(parent_tool_use_id),
            &path,
            status,
            summary,
            result,
            None,
        );
        notifications.enqueue(task_id.as_str(), xml, parent_session_id.clone(), parent_run_id);
    }

    let state = match status {
        "completed" => AsyncTaskState::Completed,
        "killed" => AsyncTaskState::Killed,
        _ => AsyncTaskState::Failed,
    };
    store.update_state(task_id, state);
}

async fn read_merged_streams_to_transcript<R1, R2>(
    mut stdout: R1,
    mut stderr: R2,
    transcript_path: PathBuf,
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
                    append_chunk(&mut captured, &stdout_buf[..read], &mut truncated, &transcript_path);
                }
            }
            read = stderr.read(&mut stderr_buf), if stderr_open => {
                let read = read?;
                if read == 0 {
                    stderr_open = false;
                } else {
                    append_chunk(&mut captured, &stderr_buf[..read], &mut truncated, &transcript_path);
                }
            }
        }
    }

    Ok((captured, truncated))
}

fn append_chunk(captured: &mut Vec<u8>, chunk: &[u8], truncated: &mut bool, transcript_path: &Path) {
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

    let decoded = crate::storage::console_decode::decode_console_bytes(chunk);
    if decoded.is_empty() {
        return;
    }
    if let Err(e) = output_writer::append_line(transcript_path, &TranscriptLine::tool(decoded)) {
        log::warn!("[shell background] transcript chunk append failed: {e}");
    }
}
