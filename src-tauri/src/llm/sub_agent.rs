//! Sub-agent executor — runs a mini agent loop with its own system prompt,
//! tool set, and iteration budget. Used by delegation tools like `browse_data`
//! to isolate complex multi-step tasks from the main conversation context.

use anyhow::Result;
use futures::StreamExt;
use log::{info, warn};

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent, ToolDefinition};
use crate::models::settings::AppSettings;
use crate::plugin::context::PluginContext;
use crate::plugin::registry::ToolRegistry;

/// Truncate a string at a safe UTF-8 boundary.
fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Configuration for a sub-agent run.
pub struct SubAgentConfig {
    /// The task description (becomes the initial user message).
    pub task: String,
    /// System prompt for the sub-agent.
    pub system_prompt: String,
    /// Which tools the sub-agent can use (names must match registry).
    pub allowed_tools: Vec<String>,
    /// Max iterations before forced stop.
    pub max_iterations: usize,
    /// Dynamic context injected alongside the system prompt.
    pub dynamic_context: String,
}

/// Result from a sub-agent run.
pub struct SubAgentResult {
    /// Final text output from the sub-agent.
    pub output: String,
    /// File paths produced during execution.
    pub files: Vec<String>,
    /// How many iterations were used.
    pub iterations_used: usize,
}

/// Run a sub-agent loop: LLM + tool execution with isolated context.
///
/// The sub-agent has its own system prompt, tool set, and message history.
/// It does not emit streaming events to the frontend (silent execution).
pub async fn run_sub_agent(
    gateway: &LlmGateway,
    tool_registry: &ToolRegistry,
    plugin_ctx: &PluginContext,
    config: SubAgentConfig,
    settings: &AppSettings,
) -> Result<SubAgentResult> {
    info!(
        "[SubAgent] Starting: task_len={}, tools={:?}, max_iter={}",
        config.task.len(),
        config.allowed_tools,
        config.max_iterations
    );

    // Guard against recursive sub-agent calls
    if config.allowed_tools.contains(&"browse_data".to_string()) {
        return Err(anyhow::anyhow!("Sub-agent must not include 'browse_data' in allowed_tools (recursion guard)"));
    }

    // Build filtered tool schemas
    let all_schemas = tool_registry.get_all_schemas().await;
    let tool_defs: Vec<ToolDefinition> = all_schemas
        .into_iter()
        .filter(|s| config.allowed_tools.contains(&s.name))
        .collect();

    info!("[SubAgent] Tool schemas loaded: {}", tool_defs.len());

    // Initialize message history with the task
    let mut messages = vec![ChatMessage::text("user", &config.task)];

    let mut output = String::new();
    let mut files: Vec<String> = vec![];
    let mut iterations_used = 0;

    // Use a unique sub-agent conversation ID so gateway active_tasks doesn't conflict
    let sub_conv_id = format!("sub_{}", uuid::Uuid::new_v4());

    let dynamic_ctx = if config.dynamic_context.is_empty() {
        None
    } else {
        Some(config.dynamic_context.as_str())
    };

    for iteration in 0..config.max_iterations {
        iterations_used = iteration + 1;

        info!("[SubAgent] iter={}/{}, messages={}", iteration, config.max_iterations, messages.len());

        // Call LLM
        let stream_result = gateway
            .stream_message(
                settings,
                messages.clone(),
                MaskingLevel::Relaxed,
                Some(&config.system_prompt),
                dynamic_ctx,
                Some(tool_defs.clone()),
                4096,
                Some(&sub_conv_id),
            )
            .await;

        let (_task_id, mut stream, _mask_ctx, _cancel_rx) = match stream_result {
            Ok(r) => r,
            Err(e) => {
                warn!("[SubAgent] LLM call failed at iter {}: {}", iteration, e);
                output = format!("Sub-agent LLM error: {}", e);
                break;
            }
        };

        // Collect stream events
        let mut iter_content = String::new();
        let mut tool_calls = vec![];
        let mut stop_reason = StopReason::EndTurn;

        while let Some(event) = stream.next().await {
            match event {
                StreamEvent::ContentDelta { delta } => {
                    iter_content.push_str(&delta);
                }
                StreamEvent::ToolCallStart { tool_call } => {
                    tool_calls.push(tool_call);
                }
                StreamEvent::Done {
                    stop_reason: sr, ..
                } => {
                    stop_reason = sr;
                    break;
                }
                StreamEvent::Error { error } => {
                    warn!("[SubAgent] Stream error: {}", error);
                    break;
                }
                _ => {}
            }
        }

        info!(
            "[SubAgent] iter={} content_len={} tool_calls={} stop={:?}",
            iteration,
            iter_content.len(),
            tool_calls.len(),
            stop_reason
        );

        // If no tool calls, we're done
        if stop_reason != StopReason::ToolUse || tool_calls.is_empty() {
            output = iter_content;
            break;
        }

        // Push assistant message with tool calls
        messages.push(ChatMessage::assistant_with_tool_calls(
            iter_content.clone(),
            tool_calls.iter().map(|tc| crate::llm::streaming::ToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            }).collect(),
        ));

        // Execute tool calls
        for tc in &tool_calls {
            // Check allowed
            if !config.allowed_tools.contains(&tc.name) {
                let err_msg = format!("Tool '{}' not available in this sub-agent", tc.name);
                messages.push(ChatMessage::tool_result(&tc.id, &tc.name, err_msg));
                continue;
            }

            info!("[SubAgent] Executing tool '{}' (id={})", tc.name, tc.id);

            let result = tool_registry
                .execute(&tc.name, plugin_ctx, tc.arguments.clone())
                .await;

            match result {
                Ok(tool_output) => {
                    // Collect file paths from tool output
                    for f in &tool_output.generated_files {
                        files.push(f.clone());
                    }

                    // Detect file paths in content (various formats)
                    for line in tool_output.content.lines() {
                        let trimmed = line.trim();
                        // Match: "File: /path", "**File**: /path", "- **File**: /path"
                        if let Some(rest) = trimmed.strip_prefix("File: ")
                            .or_else(|| trimmed.strip_prefix("**File**: "))
                            .or_else(|| trimmed.strip_prefix("- **File**: "))
                        {
                            let path = rest.trim();
                            if path.starts_with('/') && !files.contains(&path.to_string()) {
                                files.push(path.to_string());
                            }
                        }
                        // Match paths in generated/ directory
                        if trimmed.contains("/generated/") && trimmed.contains(".json") {
                            for word in trimmed.split_whitespace() {
                                let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-');
                                if clean.starts_with('/') && clean.contains("/generated/") && !files.contains(&clean.to_string()) {
                                    files.push(clean.to_string());
                                }
                            }
                        }
                    }

                    let content = if tool_output.content.len() > 8000 {
                        format!("{}...(truncated)", safe_truncate(&tool_output.content, 8000))
                    } else {
                        tool_output.content
                    };

                    messages.push(ChatMessage::tool_result(&tc.id, &tc.name, content));
                }
                Err(e) => {
                    let err_msg = format!("Tool error: {}", e);
                    warn!("[SubAgent] Tool '{}' failed: {}", tc.name, err_msg);
                    messages.push(ChatMessage::tool_result(&tc.id, &tc.name, err_msg));
                }
            }
        }
    }

    if iterations_used >= config.max_iterations && output.is_empty() {
        output = "Sub-agent reached iteration limit.".to_string();
    }

    // Clean up gateway active task entry
    gateway.clear_task(&sub_conv_id);

    info!(
        "[SubAgent] Complete: iterations={}, output_len={}, files={}",
        iterations_used,
        output.len(),
        files.len()
    );

    Ok(SubAgentResult {
        output,
        files,
        iterations_used,
    })
}
