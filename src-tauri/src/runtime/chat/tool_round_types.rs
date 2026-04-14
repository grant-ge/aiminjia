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
/// Returned by `QueryEngine::run_tool_call_with_bus` after the tool completes
/// (or fails).  The caller uses `content` to construct the tool-result message
/// sent back to the LLM.
#[derive(Debug, Clone)]
pub struct RuntimeToolCallOutcome {
    /// Echoes `RuntimeToolCallRequest::tool_call_id` for correlation.
    pub tool_call_id: String,
    /// Name of the tool that was executed.
    pub tool_name: String,
    /// Serialized content to return to the LLM as the tool result.
    pub content: String,
    /// Whether the tool reported an error (content still contains the
    /// error description that the LLM should reason over).
    pub is_error: bool,
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
