use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::hash::{Hash, Hasher};

use serde_json::Value;

use crate::llm::streaming::SystemPromptSegment;
use crate::runtime::chat::compaction::{
    build_compact_boundary_record, compact_messages_via_llm, microcompact, AutoCompactConfig,
    AutoCompactState, CompactBoundaryRecord, CompactTrigger, MicrocompactConfig,
};
use crate::runtime::chat::turn_config::TurnError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreprocessTrigger {
    Normal,
    ManualCompact,
    PromptTooLongRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreprocessStage {
    ToolResultBudget,
    Microcompact,
    Collapse,
    AutoCompact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreprocessRetryAction {
    None,
    RetryTurn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessTransition {
    pub trigger: PreprocessTrigger,
    pub retry: PreprocessRetryAction,
    pub executed_stages: Vec<PreprocessStage>,
    pub message_signature: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PreprocessRuntimeState {
    pub last_transition: Option<PreprocessTransition>,
    pub last_prompt_too_long_signature: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ToolResultBudgetConfig {
    pub aggregate_char_budget: usize,
    pub keep_recent_tool_results: usize,
    pub preserved_tool_names: HashSet<String>,
    pub replacement_preview_chars: usize,
}

impl Default for ToolResultBudgetConfig {
    fn default() -> Self {
        Self {
            aggregate_char_budget: 64_000,
            keep_recent_tool_results: 2,
            preserved_tool_names: MicrocompactConfig::default().preserved_tool_names,
            replacement_preview_chars: 160,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolResultBudgetResult {
    pub messages: Vec<Value>,
    pub executed: bool,
    pub tokens_freed_estimate: usize,
}

#[derive(Debug, Clone)]
pub struct CollapseConfig {
    pub long_result_chars: usize,
    pub keep_recent_tool_results: usize,
    pub replacement_preview_chars: usize,
}

impl Default for CollapseConfig {
    fn default() -> Self {
        Self {
            long_result_chars: 8_000,
            keep_recent_tool_results: 2,
            replacement_preview_chars: 200,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CollapseResult {
    pub messages: Vec<Value>,
    pub executed: bool,
    pub collapsed_count: usize,
}

#[derive(Debug, Clone)]
pub struct PreprocessConfig {
    pub budget: ToolResultBudgetConfig,
    pub microcompact: MicrocompactConfig,
    pub collapse: CollapseConfig,
    pub auto_compact: AutoCompactConfig,
    /// Context window in tokens for the current session model.
    /// Used for dynamic threshold calculation in auto-compact.
    pub context_window: usize,
    /// The latest compact boundary for this conversation.
    /// When set, only messages after the boundary are processed.
    pub compact_boundary: Option<CompactBoundaryRecord>,
    /// Project instruction content to restore after compact. In lotus-app this
    /// is the loaded AGENTS.md context, equivalent to cc-best's CLAUDE.md
    /// project-instruction semantics. File cache, skills, and MCP discovery
    /// are out of scope.
    pub project_instruction_content: Option<String>,
}

impl Default for PreprocessConfig {
    fn default() -> Self {
        let context_window = crate::llm::context_decay::CONSERVATIVE_CONTEXT_WINDOW;
        Self {
            budget: ToolResultBudgetConfig::default(),
            microcompact: MicrocompactConfig::default(),
            collapse: CollapseConfig::default(),
            auto_compact: AutoCompactConfig::with_context_window(context_window),
            context_window,
            compact_boundary: None,
            project_instruction_content: None,
        }
    }
}

impl PreprocessConfig {
    pub fn project_instruction_system_segment(&self) -> Option<SystemPromptSegment> {
        let content = self.project_instruction_content.as_deref()?.trim();
        if content.is_empty() {
            return None;
        }
        Some(SystemPromptSegment {
            text: format!("<project_context>\n{}\n</project_context>", content),
            cache: false,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PreprocessResult {
    pub messages: Vec<Value>,
    pub executed_stages: Vec<PreprocessStage>,
    pub compact_boundary: Option<CompactBoundaryRecord>,
    pub retry: PreprocessRetryAction,
    pub post_compact_system_segments: Vec<SystemPromptSegment>,
}

#[derive(Debug, Clone)]
struct ToolMessageMeta {
    index: usize,
    tool_name: Option<String>,
    content: String,
    is_error: bool,
    has_generated_file: bool,
    is_recent: bool,
}

fn estimate_total_chars(messages: &[Value]) -> usize {
    messages
        .iter()
        .map(|message| message.to_string().len())
        .sum()
}

fn preview_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if max_bytes >= s.len() {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn tool_call_name_map(messages: &[Value]) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(tool_calls) = message.get("toolCalls").and_then(Value::as_array) else {
            continue;
        };
        for tool_call in tool_calls {
            if let (Some(id), Some(name)) = (
                tool_call.get("id").and_then(Value::as_str),
                tool_call.get("name").and_then(Value::as_str),
            ) {
                names.insert(id.to_string(), name.to_string());
            }
        }
    }
    names
}

fn collect_tool_message_meta_with_recent(
    messages: &[Value],
    keep_recent_tool_results: usize,
) -> Vec<ToolMessageMeta> {
    let names = tool_call_name_map(messages);
    let tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            (message.get("role").and_then(Value::as_str) == Some("tool")).then_some(index)
        })
        .collect();
    let keep_from = tool_indices.len().saturating_sub(keep_recent_tool_results);

    tool_indices
        .iter()
        .enumerate()
        .map(|(position, index)| {
            let message = &messages[*index];
            let tool_name = message
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    message
                        .get("toolCallId")
                        .or_else(|| message.get("tool_call_id"))
                        .and_then(Value::as_str)
                        .and_then(|id| names.get(id).cloned())
                });
            let content = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let is_error = message
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let has_generated_file = content.contains("fileId:") || content.contains("\"fileId\"");
            ToolMessageMeta {
                index: *index,
                tool_name,
                content,
                is_error,
                has_generated_file,
                is_recent: position >= keep_from,
            }
        })
        .collect()
}

fn can_trim_or_collapse(meta: &ToolMessageMeta, preserved_tool_names: &HashSet<String>) -> bool {
    if meta.is_error || meta.has_generated_file || meta.is_recent {
        return false;
    }
    if meta.content.starts_with("[collapsed]") {
        return false;
    }
    !meta
        .tool_name
        .as_ref()
        .map(|name| preserved_tool_names.contains(name))
        .unwrap_or(false)
}

pub fn apply_tool_result_budget(
    messages: &[Value],
    config: &ToolResultBudgetConfig,
) -> ToolResultBudgetResult {
    let tool_metas =
        collect_tool_message_meta_with_recent(messages, config.keep_recent_tool_results);
    let total_tool_chars: usize = tool_metas.iter().map(|meta| meta.content.len()).sum();
    if total_tool_chars <= config.aggregate_char_budget {
        return ToolResultBudgetResult {
            messages: messages.to_vec(),
            executed: false,
            tokens_freed_estimate: 0,
        };
    }

    let mut rewritten = messages.to_vec();
    let mut current_chars = total_tool_chars;
    let mut freed_chars = 0usize;

    for meta in &tool_metas {
        if current_chars <= config.aggregate_char_budget {
            break;
        }
        if !can_trim_or_collapse(meta, &config.preserved_tool_names) {
            continue;
        }

        let preview = preview_at_char_boundary(&meta.content, config.replacement_preview_chars);
        let replacement = format!(
            "[budget-trimmed]\n{}\n[trimmed {} chars to stay within the tool-result budget]",
            preview,
            meta.content.len()
        );
        if let Some(object) = rewritten[meta.index].as_object_mut() {
            object.insert("content".to_string(), Value::String(replacement));
        }
        current_chars = current_chars.saturating_sub(meta.content.len());
        current_chars += preview.len();
        freed_chars += meta.content.len().saturating_sub(preview.len());
    }

    ToolResultBudgetResult {
        messages: rewritten,
        executed: freed_chars > 0,
        tokens_freed_estimate: freed_chars / 4,
    }
}

pub fn collapse_tool_results(messages: &[Value], config: &CollapseConfig) -> CollapseResult {
    let preserved_tool_names = MicrocompactConfig::default().preserved_tool_names;
    let tool_metas =
        collect_tool_message_meta_with_recent(messages, config.keep_recent_tool_results);
    let mut rewritten = messages.to_vec();
    let mut collapsed_count = 0usize;
    let mut seen_contents: HashSet<String> = HashSet::new();

    for meta in tool_metas {
        if !can_trim_or_collapse(&meta, &preserved_tool_names) {
            continue;
        }
        let is_duplicate = !seen_contents.insert(meta.content.clone());
        let is_long = meta.content.len() > config.long_result_chars;
        if !is_duplicate && !is_long {
            continue;
        }
        let preview = preview_at_char_boundary(&meta.content, config.replacement_preview_chars);
        let reason = if is_duplicate { "duplicate" } else { "long" };
        let replacement = format!(
            "[collapsed]\n{}\n[{} tool result hidden: original size {} chars]",
            preview,
            reason,
            meta.content.len()
        );
        if let Some(object) = rewritten[meta.index].as_object_mut() {
            object.insert("content".to_string(), Value::String(replacement));
        }
        collapsed_count += 1;
    }

    CollapseResult {
        messages: rewritten,
        executed: collapsed_count > 0,
        collapsed_count,
    }
}

/// Strip user-provided image blocks from messages to reduce token count during
/// compaction. Only user messages are rewritten; assistant/tool content is left
/// intact so transcript and protocol state stay auditable.
///
/// This is called unconditionally at the start of the preprocess pipeline
/// to prevent images from inflating character counts and triggering
/// false-positive compact thresholds.
pub fn strip_images_from_messages(messages: &[Value]) -> (Vec<Value>, bool) {
    let mut stripped = false;
    let result: Vec<Value> = messages
        .iter()
        .map(|msg| {
            let mut msg = msg.clone();
            if msg.get("role").and_then(Value::as_str) != Some("user") {
                return msg;
            }

            if msg
                .get("content")
                .and_then(Value::as_str)
                .map(is_image_data_url)
                .unwrap_or(false)
            {
                msg["content"] = Value::String("[image]".to_string());
                stripped = true;
                return msg;
            }

            if let Some(arr) = msg.get("content").and_then(Value::as_array) {
                let new_blocks: Vec<Value> = arr
                    .iter()
                    .map(|block| {
                        if is_user_image_block(block) {
                            stripped = true;
                            serde_json::json!({"type": "text", "text": "[image]"})
                        } else {
                            block.clone()
                        }
                    })
                    .collect();
                msg["content"] = Value::Array(new_blocks);
            }
            msg
        })
        .collect();
    (result, stripped)
}

fn is_user_image_block(block: &Value) -> bool {
    matches!(
        block.get("type").and_then(Value::as_str),
        Some("image" | "image_url" | "input_image")
    ) || block.get("image_url").is_some()
        || block
            .get("source")
            .and_then(|source| source.get("media_type").or_else(|| source.get("mediaType")))
            .and_then(Value::as_str)
            .map(|media_type| media_type.starts_with("image/"))
            .unwrap_or(false)
        || block
            .get("text")
            .and_then(Value::as_str)
            .map(is_image_data_url)
            .unwrap_or(false)
}

fn is_image_data_url(text: &str) -> bool {
    text.trim_start()
        .to_ascii_lowercase()
        .starts_with("data:image/")
}

/// Build a `PreservedSegment` from the compacted output messages.
///
/// Scans for the first real message after the compact boundary + summary
/// (which are always the first two messages in `compact_messages_via_llm`
/// output). The first preserved message is the head, the summary is the
/// anchor, and the last preserved message is the tail.
fn build_preserved_segment(
    new_messages: &[Value],
) -> Option<crate::runtime::chat::compaction::PreservedSegment> {
    if new_messages.len() < 3 {
        return None;
    }
    // Messages 0 and 1 are always boundary + summary
    let summary_id = new_messages[1].get("id")?.as_str()?.to_string();
    let tail = &new_messages[2..];
    let first = tail.first()?.get("id")?.as_str()?.to_string();
    let last = tail.last()?.get("id")?.as_str()?.to_string();
    let token_count = crate::llm::context_decay::estimate_tokens_from_json(tail) as u64;
    Some(crate::runtime::chat::compaction::PreservedSegment {
        first_preserved_message_id: first,
        anchor_message_id: summary_id,
        tail_message_id: last,
        preserved_token_count: token_count,
    })
}

fn ensure_preserved_messages_have_ids(messages: &mut [Value]) {
    for message in messages.iter_mut().skip(2) {
        if message.get("id").and_then(Value::as_str).is_some() {
            continue;
        }
        if let Some(object) = message.as_object_mut() {
            object.insert(
                "id".to_string(),
                Value::String(uuid::Uuid::new_v4().to_string()),
            );
        }
    }
}

fn compact_trigger_metadata_value(trigger: &CompactTrigger) -> &'static str {
    match trigger {
        CompactTrigger::Auto => "auto",
        CompactTrigger::Manual => "manual",
    }
}

fn sync_boundary_message_with_record(messages: &mut [Value], record: &CompactBoundaryRecord) {
    let Some(boundary) = messages.iter_mut().find(|message| {
        message.get("role").and_then(Value::as_str) == Some("system")
            && message.get("subtype").and_then(Value::as_str) == Some("compact_boundary")
    }) else {
        return;
    };

    let Some(object) = boundary.as_object_mut() else {
        return;
    };

    object.insert("id".to_string(), Value::String(record.id.clone()));
    object.insert(
        "conversationId".to_string(),
        Value::String(record.conversation_id.clone()),
    );
    object.insert(
        "createdAt".to_string(),
        Value::String(record.created_at.clone()),
    );

    let mut metadata = serde_json::json!({
        "trigger": compact_trigger_metadata_value(&record.trigger),
        "preTokens": record.pre_tokens,
        "postTokens": record.post_tokens,
        "tokensSaved": record.pre_tokens.saturating_sub(record.post_tokens),
        "messagesSummarized": record.messages_summarized,
    });
    if let Some(tail_message_id) = record
        .tail_message_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        metadata["tailMessageId"] = Value::String(tail_message_id.to_string());
    }
    if let Some(preserved) = &record.preserved_segment {
        metadata["preservedSegment"] = serde_json::json!({
            "firstPreservedMessageId": preserved.first_preserved_message_id,
            "anchorMessageId": preserved.anchor_message_id,
            "tailMessageId": preserved.tail_message_id,
            "preservedTokenCount": preserved.preserved_token_count,
            "headUuid": preserved.first_preserved_message_id,
            "anchorUuid": preserved.anchor_message_id,
            "tailUuid": preserved.tail_message_id,
        });
    }

    object.insert("compactMetadata".to_string(), metadata);
}

fn has_existing_compact_artifacts(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
            || (message.get("role").and_then(Value::as_str) == Some("system")
                && message.get("subtype").and_then(Value::as_str) == Some("compact_boundary"))
    })
}

fn latest_compact_boundary_index(messages: &[Value]) -> Option<usize> {
    messages.iter().rposition(|message| {
        message.get("role").and_then(Value::as_str) == Some("system")
            && message.get("subtype").and_then(Value::as_str) == Some("compact_boundary")
    })
}

fn compact_boundary_tail_message_id(boundary: &Value) -> Option<&str> {
    boundary
        .get("compactMetadata")
        .and_then(|metadata| {
            metadata
                .get("tailMessageId")
                .or_else(|| metadata.get("tail_message_id"))
        })
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn compact_artifacts_end_at_preserved_tail(messages: &[Value]) -> bool {
    let Some(boundary_index) = latest_compact_boundary_index(messages) else {
        return false;
    };
    let Some(tail_id) = compact_boundary_tail_message_id(&messages[boundary_index]) else {
        return false;
    };
    messages
        .iter()
        .position(|message| message.get("id").and_then(Value::as_str) == Some(tail_id))
        .map(|tail_index| tail_index + 1 == messages.len())
        .unwrap_or(false)
}

fn is_compact_summary_context_message(message: &Value) -> bool {
    if message.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    message
        .get("content")
        .and_then(Value::as_str)
        .map(|content| content.trim_start().starts_with("<context>"))
        .unwrap_or(false)
}

fn boundary_slice_start_for_tail(messages: &[Value], tail_pos: usize) -> usize {
    messages
        .iter()
        .take(tail_pos)
        .rposition(is_compact_summary_context_message)
        .unwrap_or(tail_pos)
}

fn latest_non_summary_message_id(messages: &[Value]) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        if message.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        message
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn message_signature(messages: &[Value]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for message in messages {
        message.to_string().hash(&mut hasher);
    }
    hasher.finish()
}

pub async fn prepare_messages_for_llm<F, Fut>(
    messages: Vec<Value>,
    conversation_id: &str,
    trigger: PreprocessTrigger,
    config: &PreprocessConfig,
    compact_state: &mut AutoCompactState,
    runtime_state: &mut PreprocessRuntimeState,
    stop_hook_active: bool,
    mut summary_fn: F,
) -> Result<PreprocessResult, TurnError>
where
    F: FnMut(Vec<Value>) -> Fut,
    Fut: Future<Output = Result<String, TurnError>>,
{
    let input_signature = message_signature(&messages);
    let input_has_compact_artifacts = has_existing_compact_artifacts(&messages);
    if input_has_compact_artifacts && compact_artifacts_end_at_preserved_tail(&messages) {
        let result = PreprocessResult {
            messages,
            executed_stages: Vec::new(),
            compact_boundary: None,
            retry: PreprocessRetryAction::None,
            post_compact_system_segments: Vec::new(),
        };
        runtime_state.last_transition = Some(PreprocessTransition {
            trigger,
            retry: result.retry,
            executed_stages: result.executed_stages.clone(),
            message_signature: input_signature,
        });
        return Ok(result);
    }

    if trigger == PreprocessTrigger::PromptTooLongRecovery
        && runtime_state.last_prompt_too_long_signature == Some(input_signature)
    {
        let result = PreprocessResult {
            messages,
            executed_stages: Vec::new(),
            compact_boundary: None,
            retry: PreprocessRetryAction::None,
            post_compact_system_segments: Vec::new(),
        };
        runtime_state.last_transition = Some(PreprocessTransition {
            trigger,
            retry: result.retry,
            executed_stages: result.executed_stages.clone(),
            message_signature: input_signature,
        });
        return Ok(result);
    }

    let mut executed_stages = Vec::new();
    let mut current_messages = if input_has_compact_artifacts {
        latest_compact_boundary_index(&messages)
            .map(|index| messages[index..].to_vec())
            .unwrap_or(messages)
    } else {
        messages
    };

    // R3.2: Boundary 视图隔离 — only process messages after the last compact boundary.
    // This prevents re-processing already-compacted history and aligns with
    // claude-code-best's `getMessagesAfterCompactBoundary()` pattern.
    if !input_has_compact_artifacts {
        if let Some(ref boundary) = config.compact_boundary {
            if let Some(ref tail_id) = boundary.tail_message_id {
                if !tail_id.is_empty() {
                    if let Some(pos) = current_messages
                        .iter()
                        .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(tail_id))
                    {
                        let start = boundary_slice_start_for_tail(&current_messages, pos);
                        current_messages = current_messages[start..].to_vec();
                    }
                }
            }
        }
    }

    // Stage 0: Strip image content before any char counting.
    // Prevents large base64 image data from inflating thresholds.
    let (stripped_messages, images_stripped) = strip_images_from_messages(&current_messages);
    if images_stripped {
        current_messages = stripped_messages;
    }

    let budget_result = apply_tool_result_budget(&current_messages, &config.budget);
    if budget_result.executed {
        current_messages = budget_result.messages;
        executed_stages.push(PreprocessStage::ToolResultBudget);
    }

    let microcompact_result = microcompact(&current_messages, &config.microcompact);
    if microcompact_result.executed {
        current_messages = microcompact_result.messages;
        executed_stages.push(PreprocessStage::Microcompact);
    }

    let collapse_result = collapse_tool_results(&current_messages, &config.collapse);
    if collapse_result.executed {
        current_messages = collapse_result.messages;
        executed_stages.push(PreprocessStage::Collapse);
    }

    let auto_compact_allowed = !compact_state.is_circuit_broken(&config.auto_compact)
        && !(trigger == PreprocessTrigger::PromptTooLongRecovery && stop_hook_active);
    let should_run_auto_compact = auto_compact_allowed
        && match trigger {
            PreprocessTrigger::Normal => {
                estimate_total_chars(&current_messages) >= config.auto_compact.threshold_chars
            }
            PreprocessTrigger::ManualCompact => true,
            PreprocessTrigger::PromptTooLongRecovery => true,
        };

    let mut compact_boundary = None;
    let mut post_compact_system_segments = Vec::new();
    let mut retry = PreprocessRetryAction::None;
    if should_run_auto_compact {
        match summary_fn(current_messages.clone()).await {
            Ok(summary_text) if !summary_text.trim().is_empty() => {
                let tail_message_id = latest_non_summary_message_id(&current_messages);
                let output = compact_messages_via_llm(current_messages, summary_text.clone());
                let mut output_messages = output.new_messages;
                ensure_preserved_messages_have_ids(&mut output_messages);
                let preserved_segment = build_preserved_segment(&output_messages);
                let mut boundary_record = build_compact_boundary_record(
                    conversation_id,
                    match trigger {
                        PreprocessTrigger::ManualCompact => CompactTrigger::Manual,
                        PreprocessTrigger::Normal | PreprocessTrigger::PromptTooLongRecovery => {
                            CompactTrigger::Auto
                        }
                    },
                    output.pre_tokens,
                    output.post_tokens,
                    output.messages_summarized,
                );
                boundary_record.summary_text = summary_text;
                boundary_record.tail_message_id = tail_message_id.or_else(|| {
                    preserved_segment
                        .as_ref()
                        .map(|segment| segment.tail_message_id.clone())
                });
                boundary_record.preserved_segment = preserved_segment;
                current_messages = output_messages;
                sync_boundary_message_with_record(&mut current_messages, &boundary_record);
                if let Some(segment) = config.project_instruction_system_segment() {
                    post_compact_system_segments.push(segment);
                }
                compact_boundary = Some(boundary_record);
                compact_state.record_success();
                executed_stages.push(PreprocessStage::AutoCompact);
                if trigger == PreprocessTrigger::PromptTooLongRecovery {
                    retry = PreprocessRetryAction::RetryTurn;
                    runtime_state.last_prompt_too_long_signature = Some(input_signature);
                }
            }
            Ok(_) => {
                compact_state.record_failure();
                if trigger == PreprocessTrigger::ManualCompact {
                    return Err(TurnError::LlmError(
                        "manual compact summary was empty".to_string(),
                    ));
                }
            }
            Err(err) => {
                compact_state.record_failure();
                if trigger == PreprocessTrigger::ManualCompact {
                    return Err(err);
                }
            }
        }
    }

    let result = PreprocessResult {
        messages: current_messages,
        executed_stages,
        compact_boundary,
        retry,
        post_compact_system_segments,
    };
    runtime_state.last_transition = Some(PreprocessTransition {
        trigger,
        retry: result.retry,
        executed_stages: result.executed_stages.clone(),
        message_signature: input_signature,
    });
    Ok(result)
}
