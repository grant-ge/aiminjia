# Mandatory Checkpoint Extraction — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a system-controlled LLM extraction call at step boundaries to reliably capture structured analysis context, replacing dependence on LLM self-discipline.

**Architecture:** At each step transition (N → N+1), before clearing message history, make a non-streaming LLM call with a step-specific extraction prompt. Parse the JSON response into a `StepCheckpoint` struct and save to enterprise memory. The existing `auto_capture_step_context` remains as a fallback. The `analysis_notes_context` injection is updated to prefer checkpoint data with field-level decay rules.

**Tech Stack:** Rust (Tauri backend), serde_json, tokio::time::timeout, existing LlmGateway::send_message()

---

### Task 1: Create extraction prompt files

**Files:**
- Create: `code/src-tauri/prompts/extract/base_extract.md`
- Create: `code/src-tauri/prompts/extract/extract_step0.md`
- Create: `code/src-tauri/prompts/extract/extract_step1.md`
- Create: `code/src-tauri/prompts/extract/extract_step2.md`
- Create: `code/src-tauri/prompts/extract/extract_step3.md`
- Create: `code/src-tauri/prompts/extract/extract_step4.md`
- Create: `code/src-tauri/prompts/extract/extract_step5.md`

**Step 1: Create the extract/ directory and base_extract.md**

```markdown
<!-- base_extract.md -->
你是一个分析结论提取器。你的任务是从以下对话记录中提取本分析步骤的关键结论。

严格按以下 JSON 格式输出，不要输出任何其他内容：

{
  "summary": "本步骤的完整总结（200-500字），包含核心方法、主要发现、关键数据",
  "keyFindings": ["关键发现1", "关键发现2", ...],
  "dataArtifacts": "关键数据产出（表格、统计数字、映射关系等），如无则为 null",
  "decisions": ["决策1: xxx，原因: xxx", ...],
  "nextStepInput": "下一步分析需要依赖的本步骤产出摘要"
}

规则：
- summary 必须包含具体数据（人数、百分比、字段名），不要用模糊表述如"若干"、"一些"
- keyFindings 至少 1 条，每条独立、具体、包含数据
- nextStepInput 应包含下一步能直接使用的信息（字段名、文件路径、人数等），避免"参见上述"之类的回指
- dataArtifacts 如果没有表格或统计数据，设为 null
- decisions 如果没有明确决策，设为空数组 []
- 只输出 JSON，不要解释、不要 markdown 包裹
```

**Step 2: Create extract_step0.md**

```markdown
<!-- extract_step0.md -->
本步骤是"分析方向确认"（Step 0），请特别关注提取：
- summary：文件概述（文件名、行数、列数、数据类型）、用户关注的分析方向
- keyFindings：文件的基本特征（如"包含 N 人的月度工资表，含 M 列"）
- dataArtifacts：列名清单（如有）
- nextStepInput：文件路径、用户分析方向、文件基本信息（行列数、关键列名）
```

**Step 3: Create extract_step1.md**

```markdown
<!-- extract_step1.md -->
本步骤是"数据清洗与理解"（Step 1），请特别关注提取：
- summary：分析总人数、排除人数及各原因分布、数据来源文件描述、整体数据质量评估
- keyFindings：字段映射关系（原始列名→语义，如"基本工资列=XX"）、薪酬结构（固定/浮动比例）、主要排除原因
- dataArtifacts：完整字段映射表、排除规则明细及各原因人数、数据质量问题清单
- decisions：排除规则的确认结果、字段映射的确认结果
- nextStepInput：保留分析人数、可用于岗位归一化的字段（部门列名、职位列名）、数据文件路径、薪酬结构总结
```

**Step 4: Create extract_step2.md**

```markdown
<!-- extract_step2.md -->
本步骤是"岗位归一化与岗位族构建"（Step 2），请特别关注提取：
- summary：行业推断结论、选定的岗位族方案（数量和名称）、归一化覆盖率、低置信度归类处理结果
- keyFindings：行业类型及推断依据、各岗位族人数分布、合并/归一化的关键决策
- dataArtifacts：完整岗位归一化映射表（原始职位→标准岗位→岗位族，含人数）、低置信度归类最终决定
- decisions：岗位族方案选择及原因、低置信度归类的最终确认
- nextStepInput：行业类型、岗位族列表及人数、标准岗位列表、用于职级推断的关键信息
```

**Step 5: Create extract_step3.md**

```markdown
<!-- extract_step3.md -->
本步骤是"职级推断与定级"（Step 3），请特别关注提取：
- summary：选定的职级通道方案、IPE 推断方法、薪酬聚类细分结果概述、异常标记统计
- keyFindings：各通道级数分布、异常标记人数（偏高/偏低）、关键子级划分
- dataArtifacts：职级框架定义（通道×级数）、各岗位族定级结果总览（标准岗位×职级×人数×平均薪酬）、异常标记清单摘要
- decisions：职级通道方案选择、子级划分规则
- nextStepInput：职级通道方案名称及各级定义、各岗位族×职级人数分布、异常标记分布、地域差异系数（如有）
```

**Step 6: Create extract_step4.md**

```markdown
<!-- extract_step4.md -->
本步骤是"薪酬公平性诊断"（Step 4），请特别关注提取：
- summary：整体健康指标（Gini 系数、R²、CR 合规率、倒挂率）、六维度诊断概述、高优先级异常数量
- keyFindings：每个维度的核心发现（内部公平性、跨岗位、回归分析、倒挂检测、结构合理性、CR 分析）、主要根因类型
- dataArtifacts：整体健康指标数值、六维度评分/状态表、高优先级异常清单摘要（人数、涉及岗位族、主要异常类型分布）、薪酬倒挂详情
- decisions：异常阈值确认、根因分析结论
- nextStepInput：健康指标数值、高优先级异常人数及主要类型、倒挂岗位组列表、需要调薪的目标群体特征、制度建设方向
```

**Step 7: Create extract_step5.md**

```markdown
<!-- extract_step5.md -->
本步骤是"行动方案与报告生成"（Step 5），请特别关注提取：
- summary：三档调薪方案对比（人数、预算、平均调幅、CR 合规率提升）、推荐方案及理由、ROI 测算结果
- keyFindings：各方案关键指标差异、ROI 数字、生成的文件清单
- dataArtifacts：三方案详细对比表、ROI 计算过程
- decisions：推荐方案的选择理由
- nextStepInput：（最终步骤，此字段写"分析已完成，无后续步骤"）
```

**Step 8: Verify all 7 files exist**

Run: `ls -la code/src-tauri/prompts/extract/`
Expected: 7 files (base_extract.md + extract_step0.md through extract_step5.md)

**Step 9: Commit**

```bash
git add code/src-tauri/prompts/extract/
git commit -m "feat: add checkpoint extraction prompt files for 6-step analysis"
```

---

### Task 2: Create checkpoint.rs — StepCheckpoint struct + extraction logic

**Files:**
- Create: `code/src-tauri/src/llm/checkpoint.rs`
- Modify: `code/src-tauri/src/llm/mod.rs:10` (add module declaration)

**Step 1: Create checkpoint.rs with StepCheckpoint struct and prompt loading**

```rust
//! Checkpoint extraction — system-controlled LLM call at step boundaries.
//!
//! At each step transition, makes a non-streaming LLM call with the full
//! message history and a step-specific extraction prompt. Parses the JSON
//! response into a [`StepCheckpoint`] and saves to enterprise memory.
//! Falls back gracefully to `auto_capture_step_context` on any failure.

use std::time::Duration;
use anyhow::Result;
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
/// and a step-specific extraction prompt. Returns `None` on any failure
/// (timeout, API error, parse error, validation error) — the caller
/// should fall back to `auto_capture_step_context`.
pub async fn checkpoint_extract(
    gateway: &LlmGateway,
    settings: &AppSettings,
    conversation_id: &str,
    step_num: u32,
    messages: &[ChatMessage],
    db: &AppStorage,
) -> Option<StepCheckpoint> {
    match tokio::time::timeout(EXTRACT_TIMEOUT, do_extract(gateway, settings, conversation_id, step_num, messages, db)).await {
        Ok(Some(cp)) => Some(cp),
        Ok(None) => {
            log::warn!("[checkpoint] Extraction returned None for step {} in conversation {}", step_num, conversation_id);
            None
        }
        Err(_) => {
            log::warn!("[checkpoint] Extraction timed out ({}s) for step {} in conversation {}", EXTRACT_TIMEOUT.as_secs(), step_num, conversation_id);
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
) -> Option<StepCheckpoint> {
    // 1. Build extraction system prompt
    let system_prompt = build_extract_prompt(step_num);

    // 2. Filter messages: keep only assistant + tool messages (remove user confirmations)
    let extract_messages: Vec<ChatMessage> = messages
        .iter()
        .filter(|m| m.role == "assistant" || m.role == "tool")
        .cloned()
        .collect();

    if extract_messages.is_empty() {
        log::warn!("[checkpoint] No assistant/tool messages to extract for step {}", step_num);
        return None;
    }

    // 3. Call LLM (non-streaming, no tools)
    let response = match gateway
        .send_message(settings, extract_messages, MaskingLevel::None, Some(&system_prompt), None)
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
            log::warn!("[checkpoint] Failed to serialize checkpoint for step {}: {}", step_num, e);
            return None;
        }
    };

    match db.set_memory(&note_key, &json_value, Some("checkpoint_extract")) {
        Ok(_) => {
            log::info!(
                "[checkpoint] Saved step {} checkpoint ({} chars) for conversation {}",
                step_num, json_value.len(), conversation_id
            );
            Some(checkpoint)
        }
        Err(e) => {
            log::warn!("[checkpoint] Failed to save step {} checkpoint: {}", step_num, e);
            None
        }
    }
}

/// Build the extraction system prompt by combining base + step-specific prompts.
fn build_extract_prompt(step_num: u32) -> String {
    let base = include_str!("../../prompts/extract/base_extract.md");
    let step_specific = match step_num {
        0 => include_str!("../../prompts/extract/extract_step0.md"),
        1 => include_str!("../../prompts/extract/extract_step1.md"),
        2 => include_str!("../../prompts/extract/extract_step2.md"),
        3 => include_str!("../../prompts/extract/extract_step3.md"),
        4 => include_str!("../../prompts/extract/extract_step4.md"),
        5 => include_str!("../../prompts/extract/extract_step5.md"),
        _ => "",
    };
    format!("{}\n\n{}", base, step_specific)
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
        let json_start = start + 7; // skip ```json
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

    log::warn!("[checkpoint] Failed to parse JSON from LLM response for step {} (len={})", step_num, text.len());
    None
}

/// Format a checkpoint for injection into the system prompt.
///
/// `is_recent` controls whether data_artifacts and decisions are truncated.
pub fn format_checkpoint_for_injection(
    checkpoint: &StepCheckpoint,
    step_num: u32,
    step_display_name: &str,
    is_recent: bool,
) -> String {
    let mut out = format!("## 第 {} 步：{} (checkpoint)\n", step_num, step_display_name);

    // summary — never truncated
    out.push_str(&format!("### 总结\n{}\n\n", checkpoint.summary));

    // key_findings — never truncated
    out.push_str("### 关键发现\n");
    for finding in &checkpoint.key_findings {
        out.push_str(&format!("- {}\n", finding));
    }
    out.push('\n');

    // next_step_input — never truncated
    out.push_str(&format!("### 传递给下一步的信息\n{}\n\n", checkpoint.next_step_input));

    // data_artifacts — truncate older steps to 2000 chars
    if let Some(ref artifacts) = checkpoint.data_artifacts {
        if !artifacts.trim().is_empty() {
            let content = if !is_recent && artifacts.len() > 2000 {
                let end = truncate_at_char_boundary(artifacts, 2000);
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
            let limit = if is_recent { decisions.len() } else { 3.min(decisions.len()) };
            for d in decisions.iter().take(limit) {
                out.push_str(&format!("- {}\n", d));
            }
            if !is_recent && decisions.len() > 3 {
                out.push_str(&format!("...({} more decisions omitted)\n", decisions.len() - 3));
            }
            out.push('\n');
        }
    }

    out
}

/// Truncate a string at a char boundary, returning the byte offset.
fn truncate_at_char_boundary(s: &str, max_chars: usize) -> usize {
    s.char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

/// Step display names for injection formatting.
pub fn step_display_name(step: u32) -> &'static str {
    match step {
        0 => "分析方向确认",
        1 => "数据清洗",
        2 => "岗位归一化",
        3 => "职级推断",
        4 => "公平性诊断",
        5 => "行动方案",
        _ => "未知步骤",
    }
}
```

**Step 2: Add module declaration to llm/mod.rs**

In `code/src-tauri/src/llm/mod.rs`, add after line 10 (`pub mod orchestrator;`):

```rust
pub mod checkpoint;
```

**Step 3: Verify it compiles**

Run: `cd code/src-tauri && cargo check 2>&1 | head -30`
Expected: Compiles without errors (warnings OK)

**Step 4: Commit**

```bash
git add code/src-tauri/src/llm/checkpoint.rs code/src-tauri/src/llm/mod.rs
git commit -m "feat: add checkpoint extraction module with StepCheckpoint struct and LLM extraction logic"
```

---

### Task 3: Integrate checkpoint_extract into step transition in chat.rs

**Files:**
- Modify: `code/src-tauri/src/commands/chat.rs:652-703` (AdvanceToStep block)

**Context:** The step transition happens inside the `StepAction::AdvanceToStep(next_step_id)` match arm in `send_message()`. Currently at line 698-703, only `auto_capture_step_context` is called. We insert `checkpoint_extract` BEFORE it.

**Step 1: Add import at the top of chat.rs**

After the existing import `use crate::llm::orchestrator::{self, StepConfig, StepStatus};` (line 9), the `checkpoint` module needs to be accessible. It's already available via `crate::llm::checkpoint`.

No new `use` statement needed at file top — we'll call it inline as `crate::llm::checkpoint::checkpoint_extract`.

**Step 2: Insert checkpoint_extract call in the AdvanceToStep block**

Find the block at lines 698-703 (the auto_capture section):

```rust
                            // Auto-capture step context BEFORE wiping message history.
                            // This preserves assistant conclusions and key tool outputs
                            // even if the LLM didn't call save_analysis_note.
                            if step_num >= 1 {
                                auto_capture_step_context(&db, &conversation_id, step_num, &chat_messages);
                            }
```

Replace it with:

```rust
                            // --- Checkpoint extraction (Layer 1) ---
                            // Make a dedicated non-streaming LLM call to extract structured
                            // step conclusions BEFORE wiping message history. Falls back to
                            // auto_capture on any failure.
                            if step_num >= 1 {
                                let cp_result = crate::llm::checkpoint::checkpoint_extract(
                                    &gateway, &settings, &conversation_id, step_num, &chat_messages, &db,
                                ).await;
                                if cp_result.is_some() {
                                    log::info!("[step_advance] Checkpoint extraction succeeded for step {}", step_num);
                                } else {
                                    log::warn!("[step_advance] Checkpoint extraction failed for step {}, falling back to auto_capture", step_num);
                                }

                                // --- Auto-capture (Layer 3) --- always runs as fallback
                                auto_capture_step_context(&db, &conversation_id, step_num, &chat_messages);
                            }
```

**Step 3: Verify it compiles**

Run: `cd code/src-tauri && cargo check 2>&1 | head -30`
Expected: Compiles without errors. The `gateway` and `settings` variables are already in scope within the `send_message` function.

**Step 4: Commit**

```bash
git add code/src-tauri/src/commands/chat.rs
git commit -m "feat: integrate checkpoint extraction into step transition flow"
```

---

### Task 4: Update analysis_notes_context to prefer checkpoint data

**Files:**
- Modify: `code/src-tauri/src/commands/chat.rs:964-1046` (analysis_notes_context block)

**Context:** The current `analysis_notes_context` block (lines 964-1046 in `agent_loop`) groups notes by step number and applies flat truncation. We need to:
1. Detect `step{N}_checkpoint` notes and parse them as `StepCheckpoint`
2. Use `format_checkpoint_for_injection` for checkpoint notes
3. Fall back to existing logic for steps without checkpoint data

**Step 1: Replace the analysis_notes_context block**

Find the block starting at line 964:
```rust
    let analysis_notes_context = {
        let notes_prefix = format!("note:{}:", conversation_id);
```

Replace the entire block (lines 964 through 1045: `};`) with:

```rust
    let analysis_notes_context = {
        let notes_prefix = format!("note:{}:", conversation_id);
        match db.get_memories_by_prefix(&notes_prefix) {
            Ok(notes) if !notes.is_empty() => {
                let current_step = current_step_config.as_ref().map(|c| c.step).unwrap_or(0);

                // Separate notes by type: checkpoint, step-grouped, non-step
                let mut checkpoints: std::collections::BTreeMap<u32, crate::llm::checkpoint::StepCheckpoint> = std::collections::BTreeMap::new();
                let mut step_notes: std::collections::BTreeMap<u32, Vec<(String, String)>> = std::collections::BTreeMap::new();
                let mut non_step_notes: Vec<(String, String)> = Vec::new();

                for (key, value) in &notes {
                    let note_name = key.strip_prefix(&notes_prefix).unwrap_or(key);

                    // Try to parse checkpoint notes
                    if note_name.starts_with("step") && note_name.ends_with("_checkpoint") {
                        if let Some(step_str) = note_name.strip_prefix("step") {
                            if let Some(num_str) = step_str.strip_suffix("_checkpoint") {
                                if let Ok(step_num) = num_str.parse::<u32>() {
                                    if let Ok(cp) = serde_json::from_str::<crate::llm::checkpoint::StepCheckpoint>(value) {
                                        checkpoints.insert(step_num, cp);
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // Group remaining notes by step number
                    if note_name.starts_with("step") {
                        if let Some(step_str) = note_name.strip_prefix("step") {
                            if let Some(step_num) = step_str.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok() {
                                step_notes.entry(step_num).or_default().push((note_name.to_string(), value.clone()));
                                continue;
                            }
                        }
                    }
                    non_step_notes.push((note_name.to_string(), value.clone()));
                }

                let mut ctx = String::from("\n\n[前序分析记录]\n");
                ctx.push_str("⚠️ 重要：以下是之前步骤保存的分析结论和关键数据，是当前步骤的唯一数据来源。\n");
                ctx.push_str("· 当前步骤必须基于这些记录继续分析\n");
                ctx.push_str("· 所有数据必须来自 execute_python 实际执行，禁止凭空推断\n");
                ctx.push_str("· 当前步骤结束前必须调用 save_analysis_note 保存关键结论，否则下一步将丢失数据\n\n");

                // Non-step notes (e.g., analysis_direction) — always full
                for (name, value) in &non_step_notes {
                    ctx.push_str(&format!("### {}\n{}\n\n", name, value));
                }

                // Collect all step numbers that have any notes
                let all_steps: std::collections::BTreeSet<u32> = checkpoints.keys()
                    .chain(step_notes.keys())
                    .copied()
                    .collect();

                let max_completed_step = if current_step > 0 { current_step - 1 } else { 0 };
                const OLDER_STEP_MAX_CHARS: usize = 3000;
                const RECENT_STEP_MAX_CHARS: usize = 6000;

                for &step_num in &all_steps {
                    let is_recent = step_num >= max_completed_step && max_completed_step > 0;

                    // Priority: checkpoint > summary > auto_context
                    if let Some(cp) = checkpoints.get(&step_num) {
                        // Use structured checkpoint injection
                        ctx.push_str(&crate::llm::checkpoint::format_checkpoint_for_injection(
                            cp,
                            step_num,
                            crate::llm::checkpoint::step_display_name(step_num),
                            is_recent,
                        ));
                    } else if let Some(notes_for_step) = step_notes.get(&step_num) {
                        // Fallback: use existing note-based injection
                        ctx.push_str(&format!("## 第 {} 步记录\n", step_num));

                        for (name, value) in notes_for_step {
                            let is_summary = name.contains("_summary");

                            if is_summary {
                                ctx.push_str(&format!("### {}\n{}\n\n", name, value));
                            } else if is_recent {
                                let truncated = if value.len() > RECENT_STEP_MAX_CHARS {
                                    let end = truncate_at_char_boundary(value, RECENT_STEP_MAX_CHARS);
                                    format!("{}...(truncated)", &value[..end])
                                } else {
                                    value.clone()
                                };
                                ctx.push_str(&format!("### {}\n{}\n\n", name, truncated));
                            } else {
                                let truncated = if value.len() > OLDER_STEP_MAX_CHARS {
                                    let end = truncate_at_char_boundary(value, OLDER_STEP_MAX_CHARS);
                                    format!("{}...(truncated)", &value[..end])
                                } else {
                                    value.clone()
                                };
                                ctx.push_str(&format!("### {}\n{}\n\n", name, truncated));
                            }
                        }
                    }
                }

                log::info!(
                    "[notes_injection] Injected {} notes ({} checkpoints + {} step groups + {} non-step) for conversation {}, current_step={}",
                    notes.len(), checkpoints.len(), step_notes.len(), non_step_notes.len(), conversation_id, current_step
                );

                ctx
            }
            _ => String::new(),
        }
    };
```

**Step 2: Verify it compiles**

Run: `cd code/src-tauri && cargo check 2>&1 | head -30`
Expected: Compiles without errors

**Step 3: Commit**

```bash
git add code/src-tauri/src/commands/chat.rs
git commit -m "feat: update analysis_notes_context to prefer structured checkpoint data"
```

---

### Task 5: Verify full build and test

**Files:** None (verification only)

**Step 1: Run full cargo check**

Run: `cd code/src-tauri && cargo check 2>&1`
Expected: No errors

**Step 2: Run Rust tests**

Run: `cd code/src-tauri && cargo test 2>&1`
Expected: All existing tests pass

**Step 3: Run TypeScript type check (verify no frontend breakage)**

Run: `cd code && npx tsc --noEmit 2>&1`
Expected: No errors (frontend is unchanged)

**Step 4: Final commit with all verification passing**

If any fixes were needed, commit them:
```bash
git add -A
git commit -m "fix: address build issues from checkpoint extraction integration"
```

---

### Task 6: Update project documentation

**Files:**
- Modify: `CLAUDE.md` (project root)
- Modify: `docs/plans/2026-03-01-checkpoint-extraction-design.md` (mark as implemented)

**Step 1: Update CLAUDE.md project structure**

In the project structure tree, add `checkpoint.rs` under `llm/`:
```
            ├── llm/
            │   ├── gateway.rs
            │   ├── router.rs
            │   ├── providers/
            │   ├── prompts.rs
            │   ├── orchestrator.rs
            │   ├── checkpoint.rs       # 步骤检查点提取（结构化 LLM 提取 + JSON 解析）
            │   ├── masking.rs
            │   └── streaming.rs
```

Add in the "Modifying analysis workflow" section:
```
- 步骤间上下文保留（三层保障）：
  1. checkpoint.rs 的 checkpoint_extract() — 步骤切换时独立 LLM 提取调用，输出结构化 JSON
  2. chat.rs 的 auto_capture_step_context() — 机械捕获 assistant 消息和 tool 输出，作为兜底
  3. LLM 主动调用 save_analysis_note — 分析过程中保存，作为补充
- 注入优先级：checkpoint > summary > auto_context
```

**Step 2: Mark design doc as implemented**

Change the header of `docs/plans/2026-03-01-checkpoint-extraction-design.md`:
```
> 状态：Implemented
```

**Step 3: Commit**

```bash
git add CLAUDE.md docs/plans/2026-03-01-checkpoint-extraction-design.md
git commit -m "docs: update project docs for checkpoint extraction feature"
```
