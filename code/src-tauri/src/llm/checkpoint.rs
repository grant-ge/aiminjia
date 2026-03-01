//! Checkpoint extraction — system-controlled LLM call at step boundaries.
//!
//! At each step transition, makes a non-streaming LLM call with the full
//! message history and a step-specific extraction prompt. Parses the JSON
//! response into a [`StepCheckpoint`] and saves to enterprise memory.
//! Falls back gracefully to `auto_capture_step_context` on any failure.

use std::time::Duration;
use serde::{Deserialize, Serialize};

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::ChatMessage;
use crate::models::settings::AppSettings;
use crate::storage::file_store::AppStorage;

/// Structured checkpoint extracted by a dedicated LLM call at step boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepCheckpoint {
    pub summary: String,
    pub key_findings: Vec<String>,
    #[serde(default)]
    pub data_artifacts: Option<String>,
    #[serde(default)]
    pub decisions: Option<Vec<String>>,
    pub next_step_input: String,
}

/// Maximum time to wait for the extraction LLM call.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(30);

/// Perform checkpoint extraction for a completed step.
///
/// Makes a non-streaming LLM call with the step's full message history
/// and a caller-provided extraction prompt. Returns `None` on any failure
/// (timeout, API error, parse error, validation error, empty prompt) — the
/// caller should fall back to `auto_capture_step_context`.
pub async fn checkpoint_extract(
    gateway: &LlmGateway,
    settings: &AppSettings,
    conversation_id: &str,
    step_num: u32,
    messages: &[ChatMessage],
    db: &AppStorage,
    extract_prompt: &str,
) -> Option<StepCheckpoint> {
    // If no extract prompt is provided, skip extraction entirely.
    if extract_prompt.trim().is_empty() {
        log::info!(
            "[checkpoint] No extract prompt provided for step {} in conversation {}, skipping",
            step_num,
            conversation_id
        );
        return None;
    }

    match tokio::time::timeout(
        EXTRACT_TIMEOUT,
        do_extract(gateway, settings, conversation_id, step_num, messages, db, extract_prompt),
    )
    .await
    {
        Ok(Some(cp)) => Some(cp),
        Ok(None) => {
            log::warn!(
                "[checkpoint] Extraction returned None for step {} in conversation {}",
                step_num,
                conversation_id
            );
            None
        }
        Err(_) => {
            log::warn!(
                "[checkpoint] Extraction timed out ({}s) for step {} in conversation {}",
                EXTRACT_TIMEOUT.as_secs(),
                step_num,
                conversation_id
            );
            None
        }
    }
}

async fn do_extract(
    gateway: &LlmGateway,
    settings: &AppSettings,
    conversation_id: &str,
    step_num: u32,
    messages: &[ChatMessage],
    db: &AppStorage,
    extract_prompt: &str,
) -> Option<StepCheckpoint> {
    // 1. Use the caller-provided extraction system prompt, with date injection
    let now = chrono::Local::now();
    let system_prompt = format!(
        "{}\n\n今天是 {}。",
        extract_prompt,
        now.format("%Y年%m月%d日")
    );

    // 2. Filter messages: keep only assistant + tool messages (remove user confirmations)
    let extract_messages: Vec<ChatMessage> = messages
        .iter()
        .filter(|m| m.role == "assistant" || m.role == "tool")
        .cloned()
        .collect();

    if extract_messages.is_empty() {
        log::warn!(
            "[checkpoint] No assistant/tool messages to extract for step {}",
            step_num
        );
        return None;
    }

    // 3. Call LLM (non-streaming, no tools)
    let response = match gateway
        .send_message(
            settings,
            extract_messages,
            MaskingLevel::Standard,
            Some(&system_prompt),
            None,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[checkpoint] LLM call failed for step {}: {}", step_num, e);
            return None;
        }
    };

    // 4. Parse JSON from response
    let checkpoint = match parse_checkpoint_json(&response.content, step_num) {
        Some(cp) => cp,
        None => return None,
    };

    // 5. Validate required fields
    if checkpoint.summary.trim().is_empty() {
        log::warn!("[checkpoint] Empty summary for step {}", step_num);
        return None;
    }
    if checkpoint.key_findings.is_empty() {
        log::warn!("[checkpoint] Empty key_findings for step {}", step_num);
        return None;
    }
    if checkpoint.next_step_input.trim().is_empty() {
        log::warn!("[checkpoint] Empty next_step_input for step {}", step_num);
        return None;
    }

    // 6. Save to enterprise memory
    let note_key = format!("note:{}:step{}_checkpoint", conversation_id, step_num);
    let json_value = match serde_json::to_string(&checkpoint) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "[checkpoint] Failed to serialize checkpoint for step {}: {}",
                step_num,
                e
            );
            return None;
        }
    };

    match db.set_memory(&note_key, &json_value, Some("checkpoint_extract")) {
        Ok(_) => {
            log::info!(
                "[checkpoint] Saved step {} checkpoint ({} chars) for conversation {}",
                step_num,
                json_value.len(),
                conversation_id
            );
            Some(checkpoint)
        }
        Err(e) => {
            log::warn!(
                "[checkpoint] Failed to save step {} checkpoint: {}",
                step_num,
                e
            );
            None
        }
    }
}

/// Parse a StepCheckpoint from LLM response text.
///
/// Tries multiple strategies:
/// 1. Direct JSON parse of the full response
/// 2. Extract JSON from ```json ... ``` fenced block
/// 3. Find first { ... } block in the text
fn parse_checkpoint_json(text: &str, step_num: u32) -> Option<StepCheckpoint> {
    let trimmed = text.trim();

    // Strategy 1: direct parse
    if let Ok(cp) = serde_json::from_str::<StepCheckpoint>(trimmed) {
        return Some(cp);
    }

    // Strategy 2: fenced code block ```json ... ```
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7;
        if let Some(end) = trimmed[json_start..].find("```") {
            let json_str = trimmed[json_start..json_start + end].trim();
            if let Ok(cp) = serde_json::from_str::<StepCheckpoint>(json_str) {
                return Some(cp);
            }
        }
    }

    // Strategy 3: find first { ... } block (greedy from first { to last })
    if let Some(brace_start) = trimmed.find('{') {
        if let Some(brace_end) = trimmed.rfind('}') {
            if brace_end > brace_start {
                let json_str = &trimmed[brace_start..=brace_end];
                if let Ok(cp) = serde_json::from_str::<StepCheckpoint>(json_str) {
                    return Some(cp);
                }
            }
        }
    }

    log::warn!(
        "[checkpoint] Failed to parse JSON from LLM response for step {} (len={})",
        step_num,
        text.len()
    );
    None
}

/// Format a checkpoint for injection into the system prompt.
///
/// `is_recent` controls whether `data_artifacts` and `decisions` are truncated.
pub fn format_checkpoint_for_injection(
    checkpoint: &StepCheckpoint,
    step_num: u32,
    display_name: &str,
    is_recent: bool,
) -> String {
    let mut out = format!("## 第 {} 步：{} (checkpoint)\n", step_num, display_name);

    // summary — never truncated
    out.push_str(&format!("### 总结\n{}\n\n", checkpoint.summary));

    // key_findings — never truncated
    out.push_str("### 关键发现\n");
    for finding in &checkpoint.key_findings {
        out.push_str(&format!("- {}\n", finding));
    }
    out.push('\n');

    // next_step_input — never truncated
    out.push_str(&format!(
        "### 传递给下一步的信息\n{}\n\n",
        checkpoint.next_step_input
    ));

    // data_artifacts — cap at 4000 chars for recent step, 2000 for older
    const RECENT_ARTIFACTS_MAX: usize = 4000;
    const OLD_ARTIFACTS_MAX: usize = 2000;
    if let Some(ref artifacts) = checkpoint.data_artifacts {
        if !artifacts.trim().is_empty() {
            let max = if is_recent { RECENT_ARTIFACTS_MAX } else { OLD_ARTIFACTS_MAX };
            let content = if artifacts.len() > max {
                let end = truncate_at_char_boundary(artifacts, max);
                format!("{}...(truncated)", &artifacts[..end])
            } else {
                artifacts.clone()
            };
            out.push_str(&format!("### 数据产出\n{}\n\n", content));
        }
    }

    // decisions — older steps keep only first 3
    if let Some(ref decisions) = checkpoint.decisions {
        if !decisions.is_empty() {
            out.push_str("### 决策\n");
            let limit = if is_recent {
                decisions.len()
            } else {
                3.min(decisions.len())
            };
            for d in decisions.iter().take(limit) {
                out.push_str(&format!("- {}\n", d));
            }
            if !is_recent && decisions.len() > 3 {
                out.push_str(&format!(
                    "...({} more decisions omitted)\n",
                    decisions.len() - 3
                ));
            }
            out.push('\n');
        }
    }

    out
}

/// Truncate a string at a char boundary, returning a safe byte offset.
///
/// Given a max byte count, walks backward to find the nearest char boundary.
/// Consistent with the same function in `chat.rs`.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}
