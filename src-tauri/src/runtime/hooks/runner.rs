use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::runtime::hooks::config::{HookConfig, HookEvent};
use crate::storage::process_ext::NoWindowExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    Allow,
    Deny { message: String },
}

#[derive(Debug, Clone)]
pub struct HookOutcome {
    pub decision: HookDecision,
    pub updated_input: Option<Value>,
    pub prevent_continuation: bool,
    pub stop_reason: Option<String>,
    pub blocking_errors: Vec<String>,
}

impl HookOutcome {
    fn allow() -> Self {
        Self {
            decision: HookDecision::Allow,
            updated_input: None,
            prevent_continuation: false,
            stop_reason: None,
            blocking_errors: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct HookOutput {
    #[serde(default = "default_behavior")]
    behavior: String,
    message: Option<String>,
    #[serde(rename = "updatedInput")]
    updated_input: Option<Value>,
    #[serde(rename = "preventContinuation", default)]
    prevent_continuation: bool,
    #[serde(rename = "stopReason")]
    stop_reason: Option<String>,
    #[serde(rename = "blockingErrors", default)]
    blocking_errors: Vec<String>,
}

fn default_behavior() -> String {
    "allow".to_string()
}

fn validate_updated_input(original: &Value, updated: &Value) -> Result<(), String> {
    let orig_obj = original
        .as_object()
        .ok_or_else(|| "original tool input must be a JSON object".to_string())?;
    let upd_obj = updated
        .as_object()
        .ok_or_else(|| "updatedInput must be a JSON object".to_string())?;

    for key in upd_obj.keys() {
        if !orig_obj.contains_key(key) {
            return Err(format!(
                "updatedInput contains unknown field '{}' not present in original tool input",
                key
            ));
        }
    }

    Ok(())
}

pub struct HookRunner;

impl HookRunner {
    pub fn new() -> Self {
        Self
    }

    pub async fn run_hook(
        &self,
        config: &HookConfig,
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<HookOutcome> {
        self.run_hook_in_workspace(config, tool_name, tool_input, None)
            .await
    }

    pub async fn run_hook_in_workspace(
        &self,
        config: &HookConfig,
        tool_name: &str,
        tool_input: &Value,
        workspace_root: Option<&Path>,
    ) -> Result<HookOutcome> {
        if !config.matches_tool(tool_name) {
            return Ok(HookOutcome::allow());
        }

        let timeout = Duration::from_secs(config.effective_timeout_secs());
        let stdin_payload = serde_json::to_vec(tool_input)?;
        let cwd = resolve_hook_cwd(workspace_root);

        let timed = tokio::time::timeout(timeout, async move {
            // Windows: route through PowerShell since `sh` is not present unless
            // Git Bash is on PATH. -NoProfile avoids loading user $PROFILE which
            // can take seconds and pollute hook stdout. The prologue tries to
            // force UTF-8 stdio, but must not fail under ConstrainedLanguage.
            #[cfg(target_os = "windows")]
            let mut command = {
                let wrapped = format!(
                    "chcp 65001 > $null 2>$null; \
                     & {{ try {{ [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false) }} catch {{ }} }} 2>$null; \
                     & {{ try {{ $OutputEncoding = [System.Text.UTF8Encoding]::new($false) }} catch {{ }} }} 2>$null; \
                     {}",
                    config.command,
                );
                let mut c = tokio::process::Command::new("powershell.exe");
                c.arg("-NoProfile").arg("-Command").arg(wrapped);
                c
            };
            #[cfg(not(target_os = "windows"))]
            let mut command = {
                let mut c = tokio::process::Command::new("sh");
                c.arg("-c").arg(&config.command);
                c
            };
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .no_window();

            if let Some(cwd) = cwd.as_ref() {
                command.current_dir(cwd);
            }

            let mut child = command.spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&stdin_payload).await?;
                stdin.shutdown().await?;
            }

            let output = child.wait_with_output().await?;
            Ok::<_, anyhow::Error>(output)
        })
        .await;

        let output = match timed {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                log::warn!(
                    "[HookRunner] hook execution error for '{}': {} — ignoring hook output",
                    tool_name,
                    err
                );
                return Ok(HookOutcome::allow());
            }
            Err(_) => {
                log::warn!(
                    "[HookRunner] hook timed out after {}s for '{}' — ignoring hook output",
                    config.effective_timeout_secs(),
                    tool_name
                );
                return Ok(HookOutcome::allow());
            }
        };

        let stdout = crate::storage::console_decode::decode_console_bytes(&output.stdout)
            .trim()
            .to_string();
        let stderr = crate::storage::console_decode::decode_console_bytes(&output.stderr)
            .trim()
            .to_string();
        let exit_code = output.status.code();

        if exit_code == Some(2) {
            let message = first_non_empty(&[stderr.as_str(), stdout.as_str()])
                .unwrap_or("Hook denied execution (exit code 2)")
                .to_string();
            return Ok(HookOutcome {
                decision: HookDecision::Deny { message },
                ..HookOutcome::allow()
            });
        }

        if !output.status.success() {
            log::warn!(
                "[HookRunner] hook exited with non-blocking status {:?} for '{}': {}",
                exit_code,
                tool_name,
                first_non_empty(&[stderr.as_str(), stdout.as_str()]).unwrap_or("<no output>")
            );
            return Ok(HookOutcome::allow());
        }

        if stdout.is_empty() {
            return Ok(HookOutcome::allow());
        }

        let parsed = match serde_json::from_str::<HookOutput>(&stdout) {
            Ok(parsed) => parsed,
            Err(err) => {
                log::warn!(
                    "[HookRunner] invalid hook JSON for '{}': {} — ignoring hook output",
                    tool_name,
                    err
                );
                return Ok(HookOutcome::allow());
            }
        };

        let decision = if parsed.behavior == "deny" {
            HookDecision::Deny {
                message: parsed
                    .message
                    .unwrap_or_else(|| "Hook denied execution".to_string()),
            }
        } else {
            HookDecision::Allow
        };

        let updated_input = if matches!(config.event, HookEvent::PreToolUse) {
            match parsed.updated_input {
                Some(updated_input) => match validate_updated_input(tool_input, &updated_input) {
                    Ok(()) => Some(updated_input),
                    Err(err) => {
                        log::warn!(
                            "[HookRunner] invalid updatedInput for '{}': {} — ignoring replacement",
                            tool_name,
                            err
                        );
                        None
                    }
                },
                None => None,
            }
        } else {
            if parsed.updated_input.is_some() {
                log::warn!(
                    "[HookRunner] ignoring updatedInput for non-pre-tool hook '{}'",
                    tool_name
                );
            }
            None
        };

        Ok(HookOutcome {
            decision,
            updated_input,
            prevent_continuation: parsed.prevent_continuation,
            stop_reason: parsed.stop_reason,
            blocking_errors: parsed.blocking_errors,
        })
    }

    pub async fn run_hooks(
        &self,
        hooks: &[&HookConfig],
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<HookOutcome> {
        self.run_hooks_in_workspace(hooks, tool_name, tool_input, None)
            .await
    }

    pub async fn run_hooks_in_workspace(
        &self,
        hooks: &[&HookConfig],
        tool_name: &str,
        tool_input: &Value,
        workspace_root: Option<&Path>,
    ) -> Result<HookOutcome> {
        let mut current_input = tool_input.clone();

        for hook in hooks {
            let outcome = self
                .run_hook_in_workspace(hook, tool_name, &current_input, workspace_root)
                .await?;
            if let HookDecision::Deny { .. } = outcome.decision {
                return Ok(outcome);
            }
            if let Some(updated_input) = outcome.updated_input.clone() {
                current_input = updated_input;
            }
            if outcome.prevent_continuation || !outcome.blocking_errors.is_empty() {
                return Ok(HookOutcome {
                    updated_input: Some(current_input),
                    ..outcome
                });
            }
        }

        Ok(HookOutcome {
            updated_input: if &current_input != tool_input {
                Some(current_input)
            } else {
                None
            },
            ..HookOutcome::allow()
        })
    }
}

impl Default for HookRunner {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_hook_cwd(workspace_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = workspace_root {
        if root.is_dir() {
            return Some(root.to_path_buf());
        }
        log::warn!(
            "[HookRunner] workspace root '{}' is not a valid directory; falling back to current cwd",
            root.display()
        );
    }

    std::env::current_dir().ok()
}

fn first_non_empty<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    candidates.iter().copied().find(|value| !value.is_empty())
}
