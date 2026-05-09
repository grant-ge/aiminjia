//! 跨平台 shell 工具共享辅助函数。
//! BashTool（Unix）和 PowerShellTool（Windows）都使用这些函数，确保两种 shell
//! 在输出截断、cancellation、stderr 合并、grep/find 等命令的语义豁免上行为一致。

use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::task::JoinHandle;

use crate::runtime::cancellation::{CancellationReason, CancellationToken};
use crate::runtime::tools::executor::ToolError;

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

pub async fn wait_for_cancellation(token: CancellationToken) -> Option<CancellationReason> {
    loop {
        if token.is_cancelled() {
            return token.reason();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
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
