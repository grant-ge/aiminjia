//! Sub-agent executor — runs a mini agent loop with its own system prompt,
//! tool set, and iteration budget. Used by delegation tools like `browse_data`
//! to isolate complex multi-step tasks from the main conversation context.

use anyhow::Result;
use futures::StreamExt;
use log::{info, warn};
use std::sync::Arc;

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent, ToolDefinition};
use crate::models::settings::AppSettings;
use crate::plugin::context::PluginContext;
use crate::plugin::registry::ToolRegistry;

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
                MaskingLevel::None,
                Some(&config.system_prompt),
                dynamic_ctx,
                Some(tool_defs.clone()),
                4096,
                None,
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

                    // Check for saved_file_path pattern in content
                    if tool_output.content.contains("File: ") {
                        for line in tool_output.content.lines() {
                            if let Some(path) = line.strip_prefix("File: ") {
                                files.push(path.trim().to_string());
                            }
                        }
                    }

                    let content = if tool_output.content.len() > 8000 {
                        format!("{}...(truncated)", &tool_output.content[..8000])
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
