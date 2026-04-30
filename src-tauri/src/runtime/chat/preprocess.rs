use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::hash::{Hash, Hasher};

use serde_json::Value;

use crate::runtime::chat::compaction::{
    build_compact_boundary_record, compact_messages_via_llm, microcompact, AutoCompactConfig,
    AutoCompactState, CompactBoundaryRecord, CompactTrigger, MicrocompactConfig,
};
use crate::runtime::chat::turn_config::TurnError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreprocessTrigger {
    Normal,
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

#[derive(Debug, Clone, Default)]
pub struct PreprocessConfig {
    pub budget: ToolResultBudgetConfig,
    pub microcompact: MicrocompactConfig,
    pub collapse: CollapseConfig,
    pub auto_compact: AutoCompactConfig,
}

#[derive(Debug, Clone)]
pub struct PreprocessResult {
    pub messages: Vec<Value>,
    pub executed_stages: Vec<PreprocessStage>,
    pub compact_boundary: Option<CompactBoundaryRecord>,
    pub retry: PreprocessRetryAction,
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

fn has_existing_compact_artifacts(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
            || (message.get("role").and_then(Value::as_str) == Some("system")
                && message.get("subtype").and_then(Value::as_str) == Some("compact_boundary"))
    })
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
    if has_existing_compact_artifacts(&messages) {
        let result = PreprocessResult {
            messages,
            executed_stages: Vec::new(),
            compact_boundary: None,
            retry: PreprocessRetryAction::None,
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
    let mut current_messages = messages;

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
            PreprocessTrigger::PromptTooLongRecovery => true,
        };

    let mut compact_boundary = None;
    let mut retry = PreprocessRetryAction::None;
    if should_run_auto_compact {
        match summary_fn(current_messages.clone()).await {
            Ok(summary_text) if !summary_text.is_empty() => {
                let tail_message_id = latest_non_summary_message_id(&current_messages);
                let output = compact_messages_via_llm(current_messages, summary_text.clone());
                let mut boundary_record = build_compact_boundary_record(
                    conversation_id,
                    CompactTrigger::Auto,
                    output.pre_tokens,
                    output.post_tokens,
                    output.messages_summarized,
                );
                boundary_record.summary_text = summary_text;
                boundary_record.tail_message_id = tail_message_id;
                current_messages = output.new_messages;
                compact_boundary = Some(boundary_record);
                compact_state.record_success();
                executed_stages.push(PreprocessStage::AutoCompact);
                if trigger == PreprocessTrigger::PromptTooLongRecovery {
                    retry = PreprocessRetryAction::RetryTurn;
                    runtime_state.last_prompt_too_long_signature = Some(input_signature);
                }
            }
            Ok(_) => {}
            Err(_) => {
                compact_state.record_failure();
            }
        }
    }

    let result = PreprocessResult {
        messages: current_messages,
        executed_stages,
        compact_boundary,
        retry,
    };
    runtime_state.last_transition = Some(PreprocessTransition {
        trigger,
        retry: result.retry,
        executed_stages: result.executed_stages.clone(),
        message_signature: input_signature,
    });
    Ok(result)
}
