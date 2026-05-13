//! Empty-response recovery state machine.
//!
//! Decides whether to retry (inject a hint user message) or surface a
//! fallback output string when an LLM turn ends with no content and no
//! tool calls. Pure state machine — no dependency on gateway / messages /
//! runtime context, so any LLM loop (sub-agent worker, main chat driver)
//! can plug it in.
//!
//! Plan: docs/superpowers/plans/2026-05-13-subagent-empty-response-handling.md

use crate::llm::streaming::StopReason;

const DEFAULT_MAX_ATTEMPTS: u32 = 2;

/// Hint injected as a user message before the next LLM call. Tells the
/// model to skip planning/reasoning narration and go straight to writing
/// content or calling tools — works around the "reasoning tokens consume
/// the entire output budget" upstream failure mode.
pub const RECOVERY_HINT: &str =
    "上一轮被 max_tokens 截断且没有产生任何可见输出——很可能被推理过程消耗光了。\
     请直接开始写实际内容,跳过计划性的叙述。\
     如果任务输出较长,请拆成小块,优先调用工具(如 write_file)分次写入,\
     不要试图在一次回复里返回一段超长文本。";

#[derive(Debug, Clone)]
pub struct EmptyResponseRecoveryConfig {
    pub max_attempts: u32,
}

impl Default for EmptyResponseRecoveryConfig {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryDecision {
    /// Quota available — caller should push `hint_message` as a user turn
    /// and continue the outer loop to invoke the LLM again.
    Retry { hint_message: &'static str },
    /// Quota exhausted or stop_reason is not recoverable — caller should
    /// set `output = fallback_output` and break out of the loop.
    Surface { fallback_output: String },
    /// LLM produced content or tool_calls — no recovery needed, caller
    /// continues with its normal handling path.
    NoRecovery,
}

pub struct EmptyResponseRecoveryState {
    attempts_used: u32,
    config: EmptyResponseRecoveryConfig,
}

impl EmptyResponseRecoveryState {
    pub fn new(config: EmptyResponseRecoveryConfig) -> Self {
        Self {
            attempts_used: 0,
            config,
        }
    }

    pub fn attempts_used(&self) -> u32 {
        self.attempts_used
    }

    /// Single decision entry point.
    ///
    /// `had_content` / `had_tool_calls` reflect what the LLM produced
    /// this turn. `max_tokens` / `iterations_used` are surfaced into the
    /// fallback text so the parent agent can reason about retry strategy.
    pub fn decide(
        &mut self,
        stop_reason: StopReason,
        had_content: bool,
        had_tool_calls: bool,
        max_tokens: u32,
        iterations_used: u32,
    ) -> RecoveryDecision {
        if had_content || had_tool_calls {
            return RecoveryDecision::NoRecovery;
        }

        match stop_reason {
            StopReason::MaxTokens => {
                if self.attempts_used < self.config.max_attempts {
                    self.attempts_used += 1;
                    RecoveryDecision::Retry {
                        hint_message: RECOVERY_HINT,
                    }
                } else {
                    RecoveryDecision::Surface {
                        fallback_output: format!(
                            "子代理在 {} 次内部重试后,仍以 stop_reason=max_tokens 结束且没有任何文本/工具调用 \
                             (iterations={}, max_tokens={})。上游持续把输出预算消耗在不可见内容上 \
                             (通常是推理 token),建议把任务拆成更小的子任务,或用更紧凑的 prompt 重新派发。",
                            self.attempts_used, iterations_used, max_tokens,
                        ),
                    }
                }
            }
            other => RecoveryDecision::Surface {
                fallback_output: format!(
                    "子代理结束但没有产生任何输出 (iterations={}, stop_reason={:?})。\
                     LLM 在没有写文本也没有调用工具的情况下结束了本轮。",
                    iterations_used, other,
                ),
            },
        }
    }
}
