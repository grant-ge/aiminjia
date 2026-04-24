use crate::plugin::tool_trait::FileMeta;

#[derive(Debug, Clone, PartialEq)]
pub struct SkillRuntimePatch {
    pub skill_id: String,
    pub system_prompt: String,
    pub allowed_tools: Option<Vec<String>>,
    pub tool_defs: Vec<serde_json::Value>,
    pub max_iterations: usize,
    pub token_budget: usize,
}


/// A single tool call request coming from the LLM.
///
/// Carries all information needed to route and execute one tool invocation,
/// including the stable `tool_call_id` emitted in events, the tool name used
/// for dispatcher lookup, and the raw argument payload.
#[derive(Debug, Clone)]
pub struct RuntimeToolCallRequest {
    /// Stable identifier assigned by the LLM for this tool call.
    /// Used as the primary key in `ToolCallExecuting` / `ToolCallCompleted` events.
    pub tool_call_id: String,
    /// Name of the tool to dispatch (matches `ToolDefinition::id`).
    pub tool_name: String,
    /// Raw JSON arguments as provided by the LLM.
    pub args: serde_json::Value,
    /// Optional human-readable annotation for audit / UI purposes.
    pub purpose: Option<String>,
}

/// Outcome of executing a single runtime tool call.
///
/// Returned by `QueryEngine::run_tool_call_with_bus`.  Upper layers
/// (`ToolRoundDriver`, `TurnDriver`, transport) can structurally distinguish
/// between:
/// - `Completed` — tool finished (successfully or with a tool-level error)
/// - `AskRequired` — the permission pipeline requires explicit user confirmation
///   before the tool may run.  S6 will route this to the UI; until then,
///   callers produce a "permission ask required" tool-result message for the LLM.
#[derive(Debug, Clone)]
pub enum RuntimeToolCallOutcome {
    /// Tool executed and produced a result (success or tool-level error).
    Completed {
        /// Echoes `RuntimeToolCallRequest::tool_call_id` for correlation.
        tool_call_id: String,
        /// Name of the tool that was executed.
        tool_name: String,
        /// Serialized content to return to the LLM as the tool result.
        content: String,
        /// Whether the tool reported an error (content still contains the
        /// error description that the LLM should reason over).
        is_error: bool,
        /// Stable message id shared by the tool:completed event and DB record.
        msg_id: String,
        /// Metadata for the generated file, if the tool produced one.
        file_meta: Option<FileMeta>,
        /// Whether the tool result was degraded (e.g. PDF→HTML fallback).
        is_degraded: bool,
        /// Human-readable notice when degradation occurred.
        degradation_notice: Option<String>,
        /// Declared max result size for this tool.
        max_result_size_chars: usize,
        /// Optional extra context message to append before the next LLM step.
        context_modifier_message: Option<serde_json::Value>,
        /// Optional runtime skill patch produced by tools such as `switch_skill`.
        skill_runtime_patch: Option<SkillRuntimePatch>,
    },

    /// The permission pipeline returned `Ask` — user confirmation is required
    /// before the tool can execute.
    ///
    /// The structured `decision` is preserved here so S6 can route it to the UI.
    /// Until S6, callers should synthesize a "permission ask required" tool-result
    /// message for the LLM and add a `FIXME(S6)` comment.
    AskRequired {
        /// Echoes `RuntimeToolCallRequest::tool_call_id` for correlation.
        tool_call_id: String,
        /// Name of the tool that needs confirmation.
        tool_name: String,
        /// Capability scopes declared by the tool definition when Ask occurred.
        capability_scopes: Vec<String>,
        /// Original tool call request so the driver can replay it after user approval.
        original_request: RuntimeToolCallRequest,
        /// The structured permission decision from the pipeline.
        decision: crate::runtime::tools::permission::PermissionDecision,
    },
}

impl RuntimeToolCallOutcome {
    /// Returns the `tool_call_id` for this outcome regardless of variant.
    pub fn tool_call_id(&self) -> &str {
        match self {
            Self::Completed { tool_call_id, .. } => tool_call_id,
            Self::AskRequired { tool_call_id, .. } => tool_call_id,
        }
    }

    /// Returns the `tool_name` for this outcome regardless of variant.
    pub fn tool_name(&self) -> &str {
        match self {
            Self::Completed { tool_name, .. } => tool_name,
            Self::AskRequired { tool_name, .. } => tool_name,
        }
    }

    /// Returns `true` if the tool call encountered an error or requires Ask.
    ///
    /// `AskRequired` is treated as an error-equivalent so callers that only
    /// inspect `is_error()` continue to gate downstream logic correctly.
    pub fn is_error(&self) -> bool {
        match self {
            Self::Completed { is_error, .. } => *is_error,
            Self::AskRequired { .. } => true,
        }
    }

    /// Returns the declared max result size for this outcome.
    pub fn max_result_size_chars(&self) -> usize {
        match self {
            Self::Completed {
                max_result_size_chars,
                ..
            } => *max_result_size_chars,
            Self::AskRequired { .. } => 0,
        }
    }

    /// Returns the content string for this outcome.
    ///
    /// `AskRequired` no longer synthesizes a fallback tool_result string; the
    /// driver must route it through the pending permission control plane.
    pub fn content(&self) -> &str {
        match self {
            Self::Completed { content, .. } => content,
            Self::AskRequired { .. } => "",
        }
    }

    /// Returns the context modifier message if present (only `Completed` variant).
    pub fn context_modifier_message(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Completed {
                context_modifier_message,
                ..
            } => context_modifier_message.as_ref(),
            Self::AskRequired { .. } => None,
        }
    }

    /// Returns the file metadata if present (only `Completed` variant).
    pub fn file_meta(&self) -> Option<&FileMeta> {
        match self {
            Self::Completed { file_meta, .. } => file_meta.as_ref(),
            Self::AskRequired { .. } => None,
        }
    }

    pub fn skill_runtime_patch(&self) -> Option<&SkillRuntimePatch> {
        match self {
            Self::Completed {
                skill_runtime_patch,
                ..
            } => skill_runtime_patch.as_ref(),
            Self::AskRequired { .. } => None,
        }
    }
}

/// A tool call that was blocked by the allowed-tools filter before execution.
///
/// The caller should synthesise a synthetic tool-result message from this
/// so the LLM receives a clear "not permitted" response.
#[derive(Debug, Clone)]
pub struct BlockedToolOutcome {
    /// Echoes the tool call id for correlation.
    pub tool_call_id: String,
    /// Name of the tool that was blocked.
    pub tool_name: String,
    /// Human-readable reason the call was blocked.
    pub reason: String,
}
