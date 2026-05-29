# LLM Output Fidelity and Interleaved Rendering Design

Date: 2026-05-26
Last updated: 2026-05-29（实施后回写关键偏离）
Status: Implemented with deviations (see §0 below)
Branch: feat/llm-output-fidelity（实际工作在 impl/llm-output-fidelity）
Supersedes: 2026-05-19-interleaved-tool-message-rendering-design (WIP), 2026-05-20-tool-call-narration-and-collapse-design (WIP)

## 0. 实施后关键偏离 (post-implementation drift, 2026-05-29)

落地实现与本 spec 初稿在几处明显偏离，按重要性排序：

### 0.1 `state.full_content` 已不参与最终消息组装（改 §3）

spec §3 描述了 `iter_emitted_content + final_only_content == full_content` 的不变式，并把 `full_content` 当作"safeguard / stop_hook 等的派生只读视图，由 driver 在每个 step push_str 维护"。

实际实现里这个不变式没建立起来。而且更重要的是：spec §3 写的是"用 `final_only_content` 持久化、不依赖 `full_content` 重新组装"——但**旧实现**在 `chat_turn_driver.rs` Step 6（post-process）里**还是借道 full_content**：先对 `full_content` 跑 `post_process::finalize_content`，然后按"前后长度差"反推 `final_only_content`（变长 → push diff；变短 → 整体覆盖）。

这个反推路径在 `strip_hallucinated_xml` 遇到未闭合 `<function_calls>` 半标签时翻车（truncate 到末尾导致 full_content 变短 → 触发"empty fallback 替换"分支 → `final_only_content = full_content.clone()` → 整段累积 iter 文字被复读为 final message）。

**2026-05-29 修复（commit `92797f61`）**：直接对 `final_only_content` 跑 post-process，删除长度差反推逻辑。`full_content` 退化为 safeguard / stop_hook 的旁路缓冲，**不再参与最终回答组装**。spec §3 / §4 描述的不变式不再尝试维护，那些字段虽还在 state 里但仅作为 safeguard 输入。

### 0.2 前端 turn 级折叠（CompletedStepsBlock）先实施后删除（改 §6）

spec §6 描述了"已完成 N 步"折叠 + Codex 风格 InlineToolBlock。实施过程：
- 2026-05-28：`ToolStepGroupBlock` + `ToolStepRow` 替换 `InlineToolBlock`（实施了 spec §6.5 的连续工具调用折叠）
- 2026-05-28：加 `CompletedStepsBlock` 把 turn 完成后的所有 iter 文字 + 工具卡塞进"已完成 N 步"灰条
- **2026-05-29 删除 `CompletedStepsBlock`**（commit `dcd80358`）：LLM 经常把"最后一个工具结果的分析"+"综合总结"塞同一条 ContentComplete，最终消息看起来像过程而非 summary，跟折叠语义不匹配。所有 blocks 改为按时序统一展开。

详见 `2026-05-28-tool-block-collapsing-design.md` 的"实施后偏离"段。

### 0.3 `InlineToolBlock` / `ToolGroupCard` / `toolDisplayMode` 已删除（改 §6）

spec §6 大量引用 `InlineToolBlock`、`CollapsedToolGroupCard`、`ToolGroupCard`、`toolDisplayMode === 'grouped'` 等。这套 legacy 聚合卡片模式已在 commit `55111103` 整体删除，全仓库只剩 interleaved 一条路径，无 `toolDisplayMode` 设置。spec §6 中所有 InlineToolBlock / CollapsedToolGroupCard / ToolGroupCard 的描述应替换为：`ToolStepGroupBlock`（折叠摘要 + 一级展开）+ `ToolStepRow`（单步行 + 二级展开 ToolTraceIO）。

### 0.4 ToolStepRow 行首 `⎿` 字符 → CSS 进度连线（改 §6 视觉描述）

`⎿` unicode corner 字符已被 CSS guide line 替代：父容器 `border-l` 画垂直主干、`ToolStepRow ::before` 画水平 stub 接到主干。详见 `2026-05-28-tool-block-collapsing-design.md`。

## 1. Goals and Scope

This design addresses three coupled problems in one branch, one design, one PR (delivery plan A).

| sub-spec | Topic | Symptoms resolved |
|---|---|---|
| **0. Fidelity** | Every fragment of LLM text must persist and must not be dropped, overwritten, or lost. Loading state must be driven per-segment, not per-turn. | "Only tool cards, no text"; "streaming output overwrites previous text"; "loading indicator disappears mid-turn" |
| **1. Interleaved rendering** | Render `assistant text → tool → assistant text → tool → ...` in the natural model order within a turn. | "Reply appears after tool cards"; "model's explanation invisible" |
| **2. Collapse + prompt guidance** | Group consecutive read-only tools into a single "正在读取 N 个文件" card; prompt model to narrate before multi-file / search / batch operations. | "Model fires 10 Reads in a row, looks like silent programming" |

### Old-conversation Compatibility (Hard Requirement)

Message schema is **unchanged**. New and old message rows coexist in `messages.jsonl`. The frontend distinguishes "legacy turn" from "interleaved turn" per-turn from the data itself; legacy turns render identically to the pre-upgrade UI. No migration, no rewrite, no destruction of user history.

### Out of Scope

- Limiting backend tool-call count per LLM step
- Changing the scheduling of concurrent read-only tools
- Changing the LLM provider protocol
- Migrating existing data
- Adding a feature flag (see §11)

## 2. Root Causes (Symptom → Code)

> Code references use **`file::symbol`** form rather than line numbers; line numbers drift quickly as `main` advances. All paths below verified against the merge-tip of `feat/llm-output-fidelity` (post sync with `main`).

### 2.1 `assistant_content` dropped from iteration persistence

`src-tauri/src/runtime/chat/chat_turn_driver.rs` — the `LlmStepResult::ToolCalls` arm of the main `'turn:` loop:

```rust
LlmStepResult::ToolCalls { assistant_content, tool_calls, ... } => {
    if !assistant_content.is_empty() {
        state.full_content.push_str(&assistant_content);   // accumulate only
    }
    ...
    executor.persist_iteration_assistant_message(
        config.conversation_id.as_str(),
        &normalized_tool_calls,                            // assistant_content NOT passed
    ).await?;
}
```

Trait `RuntimeLlmExecutor::persist_iteration_assistant_message` (same file) does not accept content. Result: iteration assistant messages persist as `text=""` + `toolCalls`. The final assistant message ends up holding all pre-tool text from every iteration concatenated together — order vs the tool calls is lost.

### 2.2 Stream retry contract is implicit

In `src-tauri/src/transport/tauri_commands/chat.rs::TauriLegacyTurnExecutor::run_llm_step`, the retry / chunk-timeout branches clear the local `iter_content` accumulator but already-emitted deltas cannot be recalled. `state.full_content` is updated only on successful step completion. This works today but depends on an implicit "step must complete cleanly before push" contract.

### 2.3 Single `streamingContent` buffer shared across steps

In `src/stores/streamingStore.ts`, every conversation has one `streamingContent: string` field that is grown by direct concatenation:

```ts
streamingContent: previous.streamingContent + delta   // whole-turn buffer
```

In `src/hooks/useStreaming.ts`, the `isIterationToolCallsMessage` predicate (inside the `message:updated` listener) is guarded by `!message.content.text` — if iteration messages start carrying text (fix in §5.1), this guard inverts and incorrectly clears loading. WIP branch missed this.

### 2.4 Frontend aggregates all tool calls into one `toolGroup` rendered before `aiSegments`

`src/hooks/useTurnRenderModel.ts::buildTurnsFromMessages` collects every tool call across all iterations into a single `turn.toolGroup`; `src/components/chat/MessageList.tsx` renders `t.toolGroup` **before** `t.aiSegments` within each turn (the JSX block that emits `<ToolGroupCard>` immediately above the `aiSegments.map`). Even if messages are ordered correctly, render order is wrong.

### 2.5 `streaming:retry-reset` only fires on retry, not on normal step boundaries

`chat.rs::run_llm_step` emits `StreamRetryReset` only on the gateway-error, chunk-timeout, and stream-error retry branches. The normal "step1 text → tools → step2 text" boundary has no backend signal telling the frontend "previous step is sealed". This is the direct cause of symptom B; §4.1's `streaming:segment-done` is the resolution.

## 3. Data Model Changes

### 3.1 `LlmStepResult` carries `iter_index`

```rust
// turn_config.rs
pub enum LlmStepResult {
    ToolCalls {
        assistant_content: String,
        tool_calls: Vec<RuntimeToolCallRequest>,
        iter_index: u32,                  // NEW — 0-indexed
        ...
    },
    ContentComplete {
        content: String,
        iter_index: u32,                  // NEW
        ...
    },
    Cancelled,
}
```

Driver passes `state.iteration_count` as `iter_index`.

### 3.2 Trait signature changes

```rust
async fn persist_iteration_assistant_message(
    &self,
    conversation_id: &str,
    assistant_content: &str,              // NEW
    tool_calls: &[serde_json::Value],
    iter_index: u32,                      // NEW
) -> Result<Option<String>, TurnError>;

async fn persist_assistant_message(
    &self,
    conversation_id: &str,
    final_only_content: &str,             // SEMANTICS CHANGED: only the ContentComplete text
    suggestions: &[String],
    generated_file_ids: &[String],
    file_metas: &[serde_json::Value],
) -> Result<String, TurnError>;
```

### 3.3 `TurnIterationState` splits accumulators

```rust
pub struct TurnIterationState {
    ...
    pub iter_emitted_content: String,     // concatenation of all iteration pre-tool text
    pub final_only_content: String,       // text from the single ContentComplete step
    ...
}
```

`state.full_content` is **retained** as a derived read-only view for `safeguard::check_iteration`, `stop_hooks`, etc. Maintained by driver as `iter_emitted_content + final_only_content`.

### 3.4 Message schema: `iterIndex` (optional)

Both assistant and tool messages gain an optional `iterIndex: number` field:

- Iteration assistant message → `iterIndex: N` (N = 0,1,2,...)
- Final assistant message → no `iterIndex`
- Tool message → `iterIndex: N` pointing to the iter of its owning tool_call
- Old messages → no field (serde `#[serde(default)]`)

This is the authoritative signal the frontend uses to classify legacy vs interleaved turns (§6.1).

### 3.5 Frontend Message type

```ts
interface AssistantContent {
  text?: string
  toolCalls?: ToolCall[]
  generatedFiles?: GeneratedFile[]
  suggestions?: string[]
  iterIndex?: number          // NEW
}
```

### 3.6 Frontend `streamingContent` → `streamingSegments`

```ts
interface StreamSegment {
  iterIndex: number
  text: string
  status: 'streaming' | 'sealed'
}

interface ConversationStreamState {
  isStreaming: boolean
  streamingSegments: StreamSegment[]      // replaces streamingContent
  toolExecutions: ToolExecution[]
  ...
}
```

Legacy field `streamingContent: string` is preserved as a derived getter returning `segments.map(s => s.text).join('')` so existing callers and tests do not break.

### 3.7 Frontend `RenderTurnBlock` (spec1 render model)

```ts
type RenderTurnBlock =
  | { kind: 'assistantText'; id: string; iterIndex?: number; message: Message }
  | { kind: 'toolStep'; toolCallId: string; iterIndex?: number; step: RenderToolStep }
  | { kind: 'collapsedToolGroup'; iterIndex?: number; group: CollapsedToolGroup }
  | { kind: 'generatedFile'; file: RenderGeneratedFile }
  | { kind: 'suggestions'; suggestions: string[] }
  | { kind: 'streamingSegment'; segment: StreamSegment }

interface RenderTurn {
  userMessage?: RenderUserMessage
  format: 'legacy' | 'interleaved'
  blocks?: RenderTurnBlock[]            // present iff format === 'interleaved'
  // Legacy fields preserved for format === 'legacy':
  aiSegments?: RenderAiSegment[]
  toolGroup?: RenderToolGroup
  generatedFiles?: RenderGeneratedFile[]
  suggestions?: string[]
  // Shared by both formats:
  peerBanners: RenderPeerBanner[]
  teamMarker?: {
    kind: 'create' | 'delete'
    toolCallId: string
    blockIndex?: number          // interleaved only: index into blocks[] where TeamCreate/Delete appeared
  }
}
```

For `format === 'interleaved'`, the `teamMarker` field still drives the dedicated `<TeamProgressBlock>` anchor, with `blockIndex` controlling where in the block sequence it renders (§6.7). The `RenderTurnBlock` variant `{ kind: 'teamMarker' }` is removed from the block list — team markers are not inline blocks.

## 4. Event Protocol Changes

### 4.1 NEW: `streaming:segment-done`

Backend emits at the end of every LLM step, before any subsequent action:
- After `ToolCalls`: emit immediately after `persist_iteration_assistant_message` completes, before tool execution begins.
- After `ContentComplete`: emit immediately after writing `state.final_only_content`, before stop-hook evaluation and before the driver exits the iteration loop.

Both paths emit exactly once per step.

**Event ordering contract (authoritative):** within one iteration step the driver emits in this fixed order:

```
…streaming:delta×N → message:updated (iter assistant) → streaming:segment-done → tool:executing… → tool:completed… → next step
                     (only on ToolCalls path)            (both paths)
```

The runtime event bus is serial within a single turn (§5.6), so the frontend receives them in this order. Consequence for §4.4: the iter-message `message:updated` arrives **before** the `segment-done` for the same iter; sealing in both listeners is idempotent and serves as a double safety net.

```rust
pub enum RuntimeEventKind {
    ...
    StreamSegmentDone {
        iter_index: u32,
        reason: SegmentDoneReason,
    },
}

pub enum SegmentDoneReason {
    ToolCalls,
    ContentComplete,
}
```

```ts
interface StreamingSegmentDonePayload {
  conversationId: string
  iterIndex: number
  reason: 'tool_calls' | 'content_complete'
}
```

Frontend on receipt: seal the segment matching `iterIndex`; do not clear or remove. New deltas with a higher `iterIndex` create a new segment.

### 4.2 `streaming:delta` payload adds `iterIndex`

```ts
interface StreamingDeltaPayload {
  conversationId: string
  delta: string
  iterIndex: number          // NEW
}
```

Defensive: frontend uses `delta.iterIndex` to decide which segment it belongs to. `segment-done` remains the authoritative boundary signal; per-delta `iterIndex` covers event-ordering edge cases.

### 4.3 `streaming:retry-reset` semantics adjusted

Instead of clearing the entire `streamingContent`, reset only the segment matching the current active iter; previously sealed segments are untouched.

### 4.4 `message:updated` decision rewritten

```
if (message.role === 'assistant') {
  const isIterationMessage = (message.toolCalls?.length ?? 0) > 0
                          && message.content.iterIndex != null
  if (!isIterationMessage) {
    flushConversationDeltas(...)
    clearConversationStreamState(...)
    removeBusyConversation(...)
  } else {
    // also seal the matching segment as a double safety net
    sealConversationStreamingSegment(conversationId, message.content.iterIndex)
  }
}
```

Predicate changes from "text is empty" to "has iterIndex" — robust, no implicit contract.

### 4.5 Unchanged events

`streaming:done`, `streaming:error`, `tool:executing`, `tool:completed`, `turn:stage`, `turn:heartbeat`, `turn:completed`, `agent:idle`, `permission:ask`, `interaction:required`, `file:generated` — semantics and payload unchanged.

### 4.6 Compatibility

- Same desktop release ships matching frontend + backend; no mixed-version concern in production.
- For dev hot-reload scenarios where versions might mismatch, new frontend without `segment-done` from old backend will fall back to using `delta.iterIndex` alone — degrades gracefully without crashing.
- Old frontend reading new messages.jsonl: `iterIndex` is ignored, rendering degrades to current aggregated behavior — no crash.

### 4.7 LLM provider protocol compatibility

The persisted message shape is OpenAI-flavored (`role: 'assistant', content.toolCalls, tool_call_id` on tool rows). Both LLM provider paths must accept the new per-iter `assistant(text + toolCalls)` series without modification:

**Anthropic path** (`src-tauri/src/llm/providers/lotus.rs` → `src-tauri/src/llm/providers/claude.rs`, production for >99% of users): `claude::build_request_body` already serializes `assistant.content + assistant.tool_calls` into structured `[text, tool_use]` content blocks. The Anthropic API requires `user → assistant → user → assistant` alternation; in our series, each iter's `tool` message is sent as `role: 'user'` with a `tool_result` block, which naturally separates consecutive iter assistant messages. **No code change needed in `claude.rs`.**

**OpenAI path** (`src-tauri/src/llm/providers/openai.rs`, used only by `src-tauri/src/llm/providers/custom.rs::send_openai_compat` for user-configured OpenAI-compatible endpoints): `openai::build_request_body` maps each persisted message 1:1 to the OpenAI Chat Completions schema with no merging or reordering. The OpenAI protocol explicitly permits multi-step function-calling sequences of the form `assistant(content + tool_calls) → tool → assistant(content + tool_calls) → tool → assistant(content)`. **No code change needed in `openai.rs`.** Future maintainers should NOT add a "merge_consecutive_assistant" pass — the multi-iter assistant series is intentional and protocol-legal.

**Old conversations** (no iterIndex anywhere): persist as a single aggregated final assistant + iter-`text=""` rows, identical to pre-upgrade. Both paths handle them as before.

**Persistence is decoupled from wire format.** The on-disk OpenAI-flavored shape is translated to Anthropic content blocks at request build time on the Anthropic path; no double-source-of-truth.

## 5. Backend Implementation

### 5.1 `chat_turn_driver.rs` main loop

```rust
LlmStepResult::ToolCalls {
    assistant_content,
    tool_calls,
    iter_index,
    ...
} => {
    // 1. Maintain split accumulators
    if !assistant_content.is_empty() {
        if !state.iter_emitted_content.is_empty() {
            state.iter_emitted_content.push('\n');
        }
        state.iter_emitted_content.push_str(&assistant_content);
    }
    state.full_content = format!(
        "{}{}", state.iter_emitted_content, state.final_only_content
    );

    // 2. Persist iteration assistant message with text + toolCalls + iterIndex
    if !tool_calls.is_empty() {
        executor.persist_iteration_assistant_message(
            config.conversation_id.as_str(),
            &assistant_content,
            &normalized_tool_calls,
            iter_index,
        ).await?;
    }

    // 3. Emit segment-done AFTER persistence, BEFORE tool execution
    self.event_bus.emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::StreamSegmentDone {
            iter_index,
            reason: SegmentDoneReason::ToolCalls,
        },
    )).await?;

    // 4. Continue with existing MessagePersisted emit + tool execution
    ...
}

LlmStepResult::ContentComplete {
    content,
    iter_index,
    stop_reason,
    ...
} => {
    state.final_only_content = content.clone();
    state.full_content = format!(
        "{}{}", state.iter_emitted_content, state.final_only_content
    );
    self.event_bus.emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::StreamSegmentDone {
            iter_index,
            reason: SegmentDoneReason::ContentComplete,
        },
    )).await?;
    ...
}
```

Final assistant message persistence (step 7) uses `state.final_only_content`, not `state.full_content`. Empty `final_only_content` is acceptable (e.g. MaxIterations cutoff) and renders as an empty turn tail.

### 5.2 `chat.rs::TauriLegacyTurnExecutor::run_llm_step`

`LlmStepInput<'a>` gains `pub iter_index: u32`. The driver passes the **pre-step** value of `state.iteration_count`: i.e. the index of the LLM call about to happen (0 for the first call of the turn). `state.iteration_count = iteration + 1` is assigned only **after** the step's `LlmStepResult` has been matched, so the value passed in equals `iteration` — the loop variable. This makes the iter_index value of the step's events match the iter_index that will be written to the iteration assistant message persisted at the end of the same step.

Delta emission uses the iter_index:

```rust
bus.emit(RuntimeEvent::stream_delta(
    session_id.clone(),
    run_id.clone(),
    clean,
    input.iter_index,
)).await;
```

Return values carry `iter_index: input.iter_index`.

### 5.3 `runtime/events.rs` and `tauri_event_adapter.rs`

```rust
RuntimeEventKind::StreamDelta { delta, iter_index } => Some(LegacyEvent {
    name: "streaming:delta",
    payload: json!({
        "conversationId": ..., "delta": delta, "iterIndex": iter_index,
    }),
}),
RuntimeEventKind::StreamSegmentDone { iter_index, reason } => Some(LegacyEvent {
    name: "streaming:segment-done",
    payload: json!({
        "conversationId": ...,
        "iterIndex": iter_index,
        "reason": match reason {
            SegmentDoneReason::ToolCalls => "tool_calls",
            SegmentDoneReason::ContentComplete => "content_complete",
        },
    }),
}),
```

### 5.4 `runtime/store/` Message persistence

`messages.jsonl` assistant rows gain an optional `iterIndex` field:

```json
{
  "id": "msg-abc",
  "role": "assistant",
  "content": { "text": "我先看一下目录" },
  "toolCalls": [...],
  "iterIndex": 0
}
```

`#[serde(default, skip_serializing_if = "Option::is_none")]` on the field. Old jsonl lines parse with `iterIndex=None`. Old files are never rewritten.

### 5.5 `llm/prompts.rs` TOOL_PREFERENCE_SECTION addition

Append the tool-communication guidance:

```md
【工具调用沟通】
- 工具外的文字会展示给用户；需要沟通目的、计划或结论时，用简短自然语言说明。
- 当准备进行多文件读取、搜索、运行命令、批量处理或可能耗时的操作时，先用一句话说明接下来要做什么。
- 不要用冒号引出工具调用；应使用句号，因为工具调用本身可能以独立状态展示。
- 简单、快速、单步的工具调用可以不解释；不要为了说明而打断流畅性。
```

### 5.6 Edge cases

| Edge | Behavior |
|---|---|
| iter=0 with empty `assistant_content` | Still calls `persist_iteration_assistant_message` with empty text — same as today |
| Retry during a step | No `segment-done` emitted; `streaming:retry-reset` already covers this path |
| `streaming:done` ordering vs `segment-done` | `segment-done` always emits within the step loop before driver exits; `streaming:done` emits after step 7 — bus is serial within turn |
| `message:updated` (iter assistant) vs `segment-done` ordering | Within one step, `MessagePersisted` (→ `message:updated`) for the iteration assistant is emitted **before** `StreamSegmentDone` (per §4.1 ordering contract). Both listeners call `sealConversationStreamingSegment(iter)`; the second call is an idempotent no-op. **Do not reorder these two emits** without updating §4.4. |
| MaxIterations / Cancelled | No final `segment-done`; `streaming:done` still fires; frontend clears via existing paths |

### 5.7 Backend tests

- Unit: `persist_iteration_assistant_message` writes text + iterIndex correctly
- Unit: `state.full_content == iter_emitted_content + final_only_content` invariant
- Integration `runtime_output_fidelity_test.rs`: mock LLM returning [text→tools→text→tools→text] produces event bus ordered as [delta×N, segment-done(0), tool×N, delta×N, segment-done(1), tool×N, delta×N, segment-done(2), done]
- `review_message_storage_v2_test.rs`: schema test for iterIndex round-trip + old-row compatibility
- Existing `review_*` tests must continue to pass (architectural invariants)

## 6. Frontend Implementation

### 6.1 Turn classification

```ts
function classifyTurn(messagesInTurn: Message[]): 'legacy' | 'interleaved' {
  return messagesInTurn.some(
    m => m.role === 'assistant' && m.content.iterIndex != null
  ) ? 'interleaved' : 'legacy'
}
```

Only `iterIndex` is considered. No heuristics. Old conversations: no message has the field → all turns classified legacy → render identically to pre-upgrade UI.

Edge: simple new turns with no tool calls (single ContentComplete) have no message with `iterIndex` → classified legacy. This is correct — there is no visual difference between legacy and interleaved rendering for a tool-free turn.

### 6.2 `buildTurnsFromMessages` skeleton

```ts
export function buildTurnsFromMessages(
  messages: Message[],
  toolExecutions: ToolExecution[],
  streamingSegments: StreamSegment[],
): RenderTurn[] {
  const turns: RenderTurn[] = []
  let current: RenderTurn | null = null
  let currentMessages: Message[] = []

  function finalizeCurrentTurn() {
    if (!current) return
    current.format = classifyTurn(currentMessages)
    if (current.format === 'interleaved') {
      current.blocks = projectInterleaved(currentMessages, current)
    } else {
      Object.assign(current, projectLegacy(currentMessages))
    }
  }

  for (const m of messages) {
    if (m.role === 'user') {
      finalizeCurrentTurn()
      current = newTurn(m); currentMessages = []
      turns.push(current); continue
    }
    if (!current) { /* same as today */ }
    currentMessages.push(m)
  }
  finalizeCurrentTurn()
  applyLiveSegmentsAndToolExecutions(turns, streamingSegments, toolExecutions)
  return turns
}
```

`projectLegacy` is the existing function extracted verbatim; no behavior change.

### 6.3 `projectInterleaved`

Walk messages in order. For each assistant message: emit `assistantText` block (if text), then a `toolStep` block per toolCall, then `generatedFile` / `suggestions` blocks. For each tool message: merge `toolResult` into the existing `toolStep` block by `toolCallId`; if missing, append a standalone result block (orphan fallback). Hidden team-substrate tools are filtered as today. `TeamCreate` calls set `turn.teamMarker` (existing behavior) and are not emitted as blocks.

After projection, run `collapseAdjacentReadOnlyTools` (§6.4).

### 6.4 Collapse channel

```ts
const COLLAPSIBLE_TOOLS = new Set(['Read', 'Glob', 'Grep'])
const COLLAPSIBLE_BASH_CMDS = new Set(['find', 'ls', 'rg', 'grep', 'cat', 'head', 'tail', 'wc', 'pwd', 'tree'])

// Disallow ANY shell metacharacter that could chain or redirect to a
// non-whitelisted command. We only collapse when we are 100% sure the
// command is a single, side-effect-free invocation.
const BASH_DANGEROUS_TOKENS = /[|;&><$`]|\&\&|\|\||\$\(/

function isCollapsibleBash(inputJson: unknown): boolean {
  const cmd = extractBashCommand(inputJson)
  if (cmd == null) return false
  if (BASH_DANGEROUS_TOKENS.test(cmd)) return false           // any pipe / redirect / chain disqualifies
  const head = cmd.trim().split(/\s+/)[0]
  return COLLAPSIBLE_BASH_CMDS.has(head)
}

function isCollapsible(step: RenderToolStep): boolean {
  if (COLLAPSIBLE_TOOLS.has(step.name)) return true
  if (step.name === 'Bash') return isCollapsibleBash(step.inputJson)
  return false
}
```

Group consecutive collapsible `toolStep` blocks with no intervening `assistantText`. Any non-collapsible block (assistant text, non-collapsible tool, generated file) breaks the group. Single-step groups un-collapse back to a regular `toolStep` (avoid "正在读取 1 个文件" cards). Failed steps default folded; the group title shows failure count.

**Collapse vs visibility contract:** the group's collapsed-state shows count + summary title (no file paths). The expanded-state lists each step as an `InlineToolBlock` with full path / args / output visible — i.e. user never loses access to "which file was read", they just have to expand to see. This is intentional for sensitive-file scenarios (e.g. `.env`): folded by default is fine because users can always drill in, and collapsed state still names the tool family ("已读取 N 个文件") so unusual activity is not hidden.

Title derivation:

- All reads: `正在读取 5 个文件` / `已读取 5 个文件`
- All searches: `正在搜索 3 次` / `已搜索 3 次`
- Mixed read/search: `正在查看 8 项资料` / `已查看 8 项资料`
- Bash read-only: `正在运行 2 个只读命令` / `已运行 2 个只读命令`
- Failure annotation: `已查看 8 项资料，其中 1 项失败`

### 6.5 `streamingStore` API

```ts
appendConversationStreamingDelta(convId, delta, iterIndex)
  // Append to segments[last] if last.iterIndex === iterIndex && status==='streaming'
  // Otherwise push a new segment {iterIndex, text: delta, status: 'streaming'}

sealConversationStreamingSegment(convId, iterIndex)
  // Mark the matching segment status='sealed'; no-op if not found

resetConversationStreamingSegment(convId, iterIndex)
  // Clear the matching segment's text; preserve structure (retry path)

clearConversationStreamState(convId)
  // Unchanged semantics; also clears streamingSegments
```

**Legacy `streamingContent` is a selector, not a state field.** It is computed at read-time inside the existing selector hook (`useConversationStreamState` / equivalent) by joining the active segments:

```ts
function selectStreamingContent(state: StreamingStoreState, convId: string): string {
  const segments = state.conversations[convId]?.streamingSegments ?? []
  return segments.map(s => s.text).join('')
}
```

No mirrored state field, no concat-on-every-mutation. Cost is O(N segments) per render, but N is bounded by the iter count in a single turn (typically ≤ 6). Selector results are referentially stable when `streamingSegments` is unchanged because store mutations always allocate a new `streamingSegments` array (§6.10), so memo on consumers continues to hold.

### 6.6 `useStreaming.ts` listeners

`streaming:delta` buffer is keyed by `(conversationId, iterIndex)`. `flushDeltas` dispatches `appendConversationStreamingDelta` per `(conv, iter)`.

New `streaming:segment-done` listener: flush deltas for that iter synchronously, then call `sealConversationStreamingSegment`. New `message:updated` decision is per §4.4.

`streaming:retry-reset`: call `resetConversationStreamingSegment(conv, currentIter)` rather than clearing all content.

Watchdog: touch on `delta` and on `segment-done`. Force-clear path unchanged (`clearConversationStreamState` already clears segments).

### 6.7 `MessageList` rendering branches

```tsx
{turns.map((t, i) => (
  <div key={i} ...>
    {/* day divider, peerBanners, userMessage — same as today */}
    {t.format === 'legacy'
      ? <LegacyTurnRows turn={t} ... />
      : <InterleavedTurn turn={t} ... />}
  </div>
))}
{/* StreamingBubble placement: see §6.8 */}
```

`LegacyTurnRows` is the existing render path extracted as a component. No visual change.

`InterleavedTurn` walks `turn.blocks` and groups consecutive `assistantText` / `generatedFile` / `suggestions` blocks into a single `<ChatRow role="assistant">`; tool blocks (`toolStep`, `collapsedToolGroup`) render outside `ChatRow` (matching the legacy position of `ToolGroupCard`).

**`<TeamProgressBlock>` placement in interleaved mode.** In legacy mode, `MessageList` renders `<TeamProgressBlock>` once per turn, sandwiched between `t.toolGroup` and `t.aiSegments`. In interleaved mode there is no single `toolGroup` "slot"; the rule is:

> Anchor `<TeamProgressBlock>` to the position of the `TeamCreate` tool call in the iteration ordering. Concretely: when projecting `turn.blocks` (§6.3), at the moment a `TeamCreate` toolCall is encountered, set `turn.teamMarker = { kind: 'create', toolCallId, blockIndex: blocks.length }`. `InterleavedTurn` reads `teamMarker.blockIndex` and renders `<TeamProgressBlock>` immediately after rendering the block at that index. `TeamDelete` follows the same rule with `kind: 'delete'`.

This keeps the team substrate visually adjacent to the iteration that created/destroyed it, instead of floating at turn top. Legacy classifyTurn turns still use the existing fixed-slot placement (no behavior change there).

### 6.8 StreamingBubble placement

For interleaved turns: streaming activity is represented inside `turn.blocks` as `{ kind: 'streamingSegment' }` blocks (the active, unsealed segment). `InterleavedTurn` renders these via `<StreamingBubble content={segment.text} />` at the natural position in the block sequence.

For legacy turns: `<StreamingBubble>` remains in its current location as the trailing child of `MessageList`.

### 6.9 New components

Both new components live under `src/components/chat-scene/` (the existing chat-scene primitives directory — `ToolTraceDetails.tsx`, `ToolTraceIO.tsx`, `ToolGroupStepRow.tsx`, etc. all live here). Note that `MessageList.tsx` itself lives under `src/components/chat/` (one level up); the chat-scene/chat split is intentional — `chat-scene/` holds reusable inline rendering primitives, `chat/` holds top-level scene-shell components. New tool-block components belong in `chat-scene/`.

- `src/components/chat-scene/InlineToolBlock.tsx` — single-tool collapsed/expanded card. Failures stay folded; the title uses destructive color + `AlertCircle` icon.
- `src/components/chat-scene/CollapsedToolGroupCard.tsx` — group card with title (derived per §6.4), step count, failure count; expanded view lists `InlineToolBlock` children.

Both components reuse existing `ToolTraceStep` / `ToolTraceIO` primitives where applicable. Both wrap in `React.memo` per CLAUDE.md §5.

### 6.10 React.memo and immutable contracts

- All segment mutations go through store methods (`appendConversationStreamingDelta`, `sealConversationStreamingSegment`, `resetConversationStreamingSegment`) which return new array / object references.
- `RenderTurnBlock[]` is `useMemo`-derived; stable `id`s on `assistantText` (`message.id`) and `toolStep` (`toolCallId`) blocks ensure leaf component memo holds.
- Existing `AiBubble.memo.test.tsx` invariants must keep passing.

## 7. Error Paths and Resilience

### 7.1 `segment-done` lost in flight

Three layers of recovery:

1. `streaming:delta` self-describes via `iterIndex`. Next step's delta with a higher iter implicitly opens a new segment.
2. `message:updated` for an iteration assistant message seals the matching segment (§4.4).
3. `streaming:done` clears full state — at worst the trailing active segment is dropped visually; persisted data unaffected.

### 7.2 Iteration persist failure

Existing contract preserved: `?` propagates up, turn ends with `TurnError`. Frontend receives `streaming:error` + `turn:completed{outcome: ExecutionError}` → clears state.

### 7.3 Stream retry within a step

`streaming:retry-reset` is emitted (existing). Frontend resets only the active segment's text, segment structure preserved. No `segment-done` because the step has not ended.

### 7.4 Old-frontend × new-backend (dev only)

Not a production concern (same desktop release). Degraded gracefully: old frontend ignores `iterIndex`, all delta accumulates into the same buffer, renders as today.

### 7.5 New-frontend × old-conversation

Falls into §6.1 legacy path. Rendering identical to pre-upgrade. New code MUST NOT modify legacy projection output.

### 7.6 Watchdog

`lastActivityRef` touches on `delta` and `segment-done`. Force-clear remains at 90s and goes through `clearConversationStreamState` which clears segments.

### 7.7 Non-success `turn:completed`

`clearConversationStreamState` clears active segments; already-sealed segments reappear via `buildTurnsFromMessages` from persisted messages. User sees what was actually said, not the never-finished trailing fragment.

### 7.8 Cancellation mid-step

`LlmStepResult::Cancelled` → driver exits without `segment-done`. Active segment persists in UI until `streaming:done` / `turn:completed` clears state. Same UX as today.

## 8. Test Plan

### 8.1 Backend unit

- `chat_turn_driver`: non-empty `assistant_content` writes to iteration message with iterIndex
- `chat_turn_driver`: `final_only_content` does not duplicate iteration text
- `state.full_content == iter_emitted_content + final_only_content` invariant after every step

### 8.2 Backend integration

- `tests/runtime_output_fidelity_test.rs`: 5-step mock turn produces correctly ordered event bus output
- `tests/review_message_storage_v2_test.rs`: iterIndex round-trip + old-row compatibility
- `tests/review_provider_request_shape_test.rs` (new): build a fixture conversation with [user, assistant(text+toolCalls,iter=0), tool, assistant(text+toolCalls,iter=1), tool, assistant(text), user(next-turn)] and assert:
  - `claude::build_request_body` produces a valid Anthropic body (alternating user/assistant via tool_result-as-user; structured content blocks correct)
  - `openai::build_request_body` produces a valid OpenAI body (1:1 mapping; tool_call_id pairing intact; no consecutive-assistant merging)
- All existing `review_*` tests pass

### 8.3 Frontend unit

`src/hooks/__tests__/useTurnRenderModel.test.ts`:

- `classifyTurn`: presence of iterIndex on any assistant message → 'interleaved'; absence → 'legacy'
- legacy projection: field-by-field identical to current output (regression guard)
- interleaved projection: block ordering matches message ordering
- orphan tool result fallback
- generatedFile / suggestions adjacency to producing assistant message
- TeamCreate / hidden team tools filtering preserved
- Collapse rules from §6.4 (all combinations)
- Mixed legacy + interleaved turns in one conversation

`src/stores/streamingStore.test.ts`:

- `appendConversationStreamingDelta` across iterIndex creates new segments
- `sealConversationStreamingSegment` halts continuation
- `resetConversationStreamingSegment` clears text without removing segment
- Legacy `streamingContent` getter equals concatenation

`src/hooks/useStreaming.integration.test.tsx`:

- Post-segment-done delta does not append to previous segment
- segment-done lost: delta.iterIndex still creates new segment
- message:updated for iteration message seals segment
- retry-reset only clears active segment
- watchdog clears all segments on force-clear

### 8.4 Frontend component

- `InlineToolBlock.test.tsx`: collapsed/expanded states; failure visual
- `CollapsedToolGroupCard.test.tsx`: title derivation; failure count display; expansion lists InlineToolBlock children
- `MessageList.test.tsx`:
  - legacy turn renders ToolGroupCard + AiBubble (current snapshot)
  - interleaved turn renders InlineToolBlock + AiBubble in message order
  - active segment in interleaved turn renders StreamingBubble inline
  - legacy turn + streaming → StreamingBubble at MessageList trailing position

### 8.5 Manual verification checklist

1. Open a pre-upgrade conversation → visual matches baseline screenshot
2. New conversation, simple "hello" → legacy render (no tools)
3. New conversation, "look at the src directory" → interleaved: assistant explains → tool card → assistant summarizes
4. New conversation, 5 consecutive Reads → folded into "已读取 5 个文件"
5. New conversation, Read + Bash(`npm test`) → not folded (npm test outside whitelist)
6. Mid-turn network drop → retry-reset → active segment cleared, sealed segments intact
7. Restart app, reopen conversation → render order matches streaming order
8. Cancel a turn → active segment disappears, sealed segments persist
9. Multi-step text+tools+text+tools+text → text positioning correct, no overwrite
10. Turn with multiple sealed segments and empty final → no hanging visual remnants

## 9. Effort Estimate

| Module | Lines | Days |
|---|---|---|
| Backend `LlmStepResult` + `TurnIterationState` + `chat_turn_driver` | ~150 | 0.5 |
| Backend `chat.rs` executor + delta payload + segment-done emit | ~80 | 0.3 |
| Backend `events.rs` + adapter new event | ~50 | 0.2 |
| Backend Message schema iterIndex | ~30 | 0.2 |
| Backend prompts.rs TOOL_PREFERENCE_SECTION | ~20 | 0.1 |
| Backend tests | ~400 | 0.7 |
| Frontend streamingStore segments + legacy getter | ~150 | 0.5 |
| Frontend useStreaming listeners | ~80 | 0.3 |
| Frontend useTurnRenderModel classify + projectInterleaved + collapse | ~300 | 1.0 |
| Frontend InlineToolBlock + CollapsedToolGroupCard | ~250 | 0.5 |
| Frontend MessageList branches + StreamingBubble embed | ~120 | 0.4 |
| Frontend lib/tauri.ts event bindings | ~30 | 0.1 |
| Frontend tests | ~700 | 1.2 |
| Manual verification + integration fix-up | — | 0.5 |
| Plan / commit slicing / review | — | 0.5 |

**Total ≈ 7 person-days of focused work.** Realistic calendar window for a single developer including review iteration, manual verification across legacy + new conversations, and unforeseen integration fix-ups: **8–10 working days**. The original 5–7 day estimate underweights frontend test breadth (~700 lines of tests across 4 areas) and the cross-stack integration debugging that always surfaces in event-protocol changes.

## 10. Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| `iterIndex` written before old frontend ships → crash | Desktop release rollback hard | Same-release ship; serde `default` defensive |
| Segment model refactor breaks existing tests | Test-fix-up time blowup | Legacy `streamingContent` getter preserved; old tests keep passing |
| `classifyTurn` misjudges a turn | Visual oddity for users | Single authoritative signal (`iterIndex`); driver guarantees write-side |
| Collapse whitelist mis-folds dangerous tools | Lost visibility on writes | Conservative whitelist; uncertain → do not fold |
| AiBubble memo broken by segments refactor | Long-turn jank | All mutations through store methods; existing memo tests gate |
| StreamingBubble position change breaks scroll anchor | UX regression on "scroll to bottom" | `data-aijia-streaming-bubble` selector retained; verify manually |
| `state.full_content` dual-source drift | Hook / safeguard misbehave | `full_content` is a derived view, recomputed every step; invariant unit test |
| Per-iter message count increase → jsonl bloat | Slow load times | Typical turn ≤ 6 iters × few KB; measured impact <10%; no optimization needed |
| Mixed legacy + interleaved within one conversation | User confusion | Expected and documented; turn-level classification is intentional |

## 11. Rollout

### 11.1 Strategy

No feature flag. Render + persistence are deeply coupled — flag flipping risk exceeds direct release risk. Desktop releases are atomic, rollback is by version.

### 11.2 Branch and commit ordering

```
feat/llm-output-fidelity:
  1. feat(runtime): add iter_index to LlmStepResult + stream events + LlmStepInput,
                    split iter_emitted_content vs final_only_content,
                    update persist_iteration_assistant_message trait signature
                    (text + iterIndex), wire driver to pass them.
                    [single commit — these changes have circular type dependencies
                     and any partial subset fails to compile.]
  2. feat(runtime): emit streaming:segment-done after each LLM step (ToolCalls and
                    ContentComplete paths); event ordering contract in §4.1.
  3. feat(prompts): tool communication guidance in TOOL_PREFERENCE_SECTION
  4. test(runtime): output fidelity integration + review tests + provider request shape
  5. feat(frontend): streamingSegments model in store + iterIndex-aware delta API
                     + selector for legacy streamingContent
  6. feat(frontend): subscribe streaming:segment-done; rewrite message:updated decision
                     per §4.4
  7. feat(frontend): InlineToolBlock + CollapsedToolGroupCard components
  8. feat(frontend): classifyTurn + projectInterleaved + collapse in useTurnRenderModel;
                     teamMarker.blockIndex
  9. feat(frontend): MessageList legacy/interleaved branches; embed StreamingBubble
                     inline for interleaved
  10. test(frontend): turn projection, store, integration, components
  11. docs: spec0/1/2 design notes
```

Each commit self-contained, compiles, tests green. Commit 1 is intentionally larger than the original draft's 3 separate commits — the trait signature, the `LlmStepResult` enum fields, and the driver consumer have circular type dependencies and cannot be split without leaving an intermediate commit that fails to typecheck.

### 11.3 ToolGroupCard sunset path (out of scope for this PR)

Documented for follow-up:

> When the share of active conversations holding `ToolGroupCard`-rendered turns drops below ~5% (≥ 6 months from this release), file an RFC to: collapse `format='legacy'` projection into "list InlineToolBlock per step in legacy aggregate order" and remove `ToolGroupCard`. Until then, `ToolGroupCard` is frozen — bug fixes only, no feature work.

### 11.4 Post-release monitoring

Week 1 after release:

- Diagnostics: ratio of `streaming.segment_done` to `store.streaming.append` should be ~1:N (N = iter count). Anomalies indicate event loss.
- User feedback: zero increment in "text disappears / loading stuck / tool flood" categories
- jsonl file size growth: <10%

Regression response:

- Render-layer bug: frontend hotfix possible (e.g. `classifyTurn` forced to 'legacy' as a one-line emergency knob)
- Persistence-layer bug: rolled-back release still readable by both new and old code (iterIndex ignored gracefully)

## 12. Supersession Note

This design supersedes the two WIP specs on `codex/llm-tool-output-rendering-wip`:

- `2026-05-19-interleaved-tool-message-rendering-design.md` (concept of interleaved render)
- `2026-05-20-tool-call-narration-and-collapse-design.md` (concept of collapse + prompt)

The WIP implementation is not merged. Its valuable concepts (interleaved blocks, collapsed groups, prompt guidance) are absorbed into this design. The WIP branch base is significantly behind `main` and carries unrelated reverts; re-doing on `main` is judged less costly than rebasing.

Symptom B (text overwrite across LLM steps, loading lost), not addressed by either WIP spec, is treated as the foundational problem (spec0 "fidelity") in this design and is the reason the segments-based stream model exists.
