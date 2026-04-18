use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::runtime::hooks::config::HookConfig;

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
}

impl HookOutcome {
    fn allow() -> Self {
        Self {
            decision: HookDecision::Allow,
            updated_input: None,
            prevent_continuation: false,
            stop_reason: None,
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
}

fn default_behavior() -> String {
    "allow".to_string()
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
        if !config.matches_tool(tool_name) {
            return Ok(HookOutcome::allow());
        }

        let timeout = Duration::from_secs(config.effective_timeout_secs());
        let stdin_payload = serde_json::to_vec(tool_input)?;

        let timed = tokio::time::timeout(timeout, async move {
            let mut child = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&config.command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;

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
                log::warn!("[HookRunner] hook execution error: {}", err);
                return Ok(HookOutcome::allow());
            }
            Err(_) => {
                log::warn!(
                    "[HookRunner] hook timed out after {}s",
                    config.effective_timeout_secs()
                );
                return Ok(HookOutcome::allow());
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            if output.status.code() == Some(2) {
                return Ok(HookOutcome {
                    decision: HookDecision::Deny {
                        message: "Hook denied execution (exit code 2)".to_string(),
                    },
                    ..HookOutcome::allow()
                });
            }
            return Ok(HookOutcome::allow());
        }

        let parsed = match serde_json::from_str::<HookOutput>(&stdout) {
            Ok(parsed) => parsed,
            Err(_) => return Ok(HookOutcome::allow()),
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

        Ok(HookOutcome {
            decision,
            updated_input: parsed.updated_input,
            prevent_continuation: parsed.prevent_continuation,
            stop_reason: parsed.stop_reason,
        })
    }

    pub async fn run_hooks(
        &self,
        hooks: &[&HookConfig],
        tool_name: &str,
        tool_input: &Value,
    ) -> Result<HookOutcome> {
        let mut current_input = tool_input.clone();

        for hook in hooks {
            let outcome = self.run_hook(hook, tool_name, &current_input).await?;
            if let HookDecision::Deny { .. } = outcome.decision {
                return Ok(outcome);
            }
            if let Some(updated_input) = outcome.updated_input.clone() {
                current_input = updated_input;
            }
            if outcome.prevent_continuation {
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
