# Phase 0 Baseline Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 `chat.rs`、事件协议、状态真相源、tool 生命周期的审计基线，并产出来自真实 legacy emit 路径的 golden trace。

**Architecture:** 不修改运行时业务语义，只允许引入审计辅助、测试夹具、零逻辑的 emit wrapper。Phase 0 的所有产物都要直接服务 Phase 1 切换。

**Tech Stack:** Rust, Tauri, markdown docs, cargo test, lightweight trace capture

## 当前实际状态（2026-04-10）

- 状态：部分完成
- 已落地：`src-tauri/src/runtime_audit/mod.rs`、`src-tauri/src/runtime_audit/trace_capture.rs`、`src-tauri/tests/golden_trace_capture.rs`
- 已完成内容偏向“真实 legacy 事件采样与测试基线”
- 已验证：`cargo test golden_trace_capture -- --nocapture` 通过
- 未完成：计划里要求的 `docs/architecture-audit/*.md` 审计文档尚未补齐，所以本期不能算完全完成

---

**Hard constraints:**
- Golden trace 不能手写；必须来自真实 legacy emit 路径采样。
- 测试不能用常量断言冒充 TDD；必须因缺少真实模块、缺少 helper、或事件顺序不符而失败。
- 允许在 `chat.rs` 中抽出零逻辑 `emit_legacy_event(...)` 包装器，但不能顺手改业务编排。

---

### Task 1: 审计 `chat.rs` 职责地图

**Files:**
- Modify: `docs/architecture-audit/chat-responsibility-map.md`
- Read: `src-tauri/src/commands/chat.rs`
- Test: `src-tauri/src/commands/chat.rs` 相关现有测试

- [x] **Step 1: 写职责地图文档骨架**

```markdown
# chat.rs Responsibility Map

| Responsibility | Current Function/Section | Dependencies | Side Effects | Target Layer |
|---|---|---|---|---|
| input normalization | send_message prelude | AppHandle, payload | none | Transport/Input |
| conversation load | ... | AppStorage | reads storage | SessionRuntime |
| auth check | ... | AuthManager | may emit auth:expired | Auth/Permission |
| context build | ... | prompts/router | allocates prompt | QueryEngine |
| tool dispatch | ... | tool registry/context | emits tool:* | Tool Runtime |
| sub-agent | ... | sub_agent | emits agent:* | Agent Runtime |
| stream emit | ... | AppHandle | emits streaming:* | TauriAdapter |
| persist | ... | AppStorage | writes messages | SessionStore |
```

- [x] **Step 2: 运行搜索并定位 send_message 相关 helper**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app && rg -n "send_message|emit\\(|sub_agent|precompute|tool|message:updated|streaming:delta|streaming:done" src-tauri/src/commands/chat.rs`
Expected: 输出主流程相关段落，足以填充职责表

- [x] **Step 3: 填完整职责地图**

```markdown
补全每个 responsibility 对应的代码段、依赖对象、副作用和目标归属层。
要求至少覆盖：鉴权、上下文构建、模型选择、tool loop、sub-agent、stream emit、持久化、取消/清理。
每一行都要带当前代码位置，避免后续迁移时再回头定位。
```

- [x] **Step 4: 提交职责地图**

```bash
git add docs/architecture-audit/chat-responsibility-map.md
git commit -m "docs: add chat command responsibility audit"
```

### Task 2: 审计状态真相源与事件契约

**Files:**
- Modify: `docs/architecture-audit/state-owner-matrix.md`
- Modify: `docs/architecture-audit/event-contract-matrix.md`
- Read: `src-tauri/src/llm/gateway.rs`
- Read: `src-tauri/src/storage/file_store/mod.rs`
- Read: `src/hooks/useStreaming.ts`
- Read: `src/hooks/useChat.ts`

- [x] **Step 1: 写状态矩阵骨架**

```markdown
# State Owner Matrix

| State | Current Source of Truth | Readers | Writers | Conflict | Future Source |
|---|---|---|---|---|---|
| busy | LlmGateway.active_tasks | hooks/chat | gateway/chat | duplicates run.lock | RunState |
| run lock | file_store/run.lock | chat/storage | storage | duplicates busy | RunStore |
| streaming | event-driven/front-end | hooks | chat emit | inferred only | RunState |
| python session | PythonSessionManager | tools | python/session | conversation scoped | RunScoped PythonSession |
```

- [x] **Step 2: 写事件契约矩阵模板**

```markdown
# Current Event Contract

| Event | Emitter | Consumer | Required Payload | Ordering Contract |
|---|---|---|---|---|
| streaming:delta | chat.rs | useStreaming | delta, conversation_id? | before streaming:done |
| streaming:done | chat.rs | useStreaming | final status | after all deltas |
| tool:executing | tool path | useChat/useStreaming | tool metadata | before tool:completed |
| tool:completed | tool path | useChat/useStreaming | result metadata | after executing |
| message:updated | persist path | chat UI | message/conversation refs | after content flush |
| agent:idle | sub_agent path | UI | agent summary | terminal idle signal |
```

- [x] **Step 3: 运行搜索验证真实事件名与读写方**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app && rg -n "streaming:delta|streaming:done|tool:executing|tool:completed|message:updated|agent:idle" src-tauri src`
Expected: 能定位到 emitter/consumer；若有差异，更新文档并锁定真实名字

- [x] **Step 4: 补充状态真相源判定**

```markdown
矩阵中额外增加两列：
- "Migration blocker"：为什么这个状态会阻塞 runtime-first 改造
- "Phase to fix"：预计在哪一期收敛
```

- [x] **Step 5: 提交状态与事件审计**

```bash
git add docs/architecture-audit/state-owner-matrix.md docs/architecture-audit/event-contract-matrix.md
git commit -m "docs: add backend state and event contract audit"
```

### Task 3: 产出真实 legacy trace 与回放基线

**Files:**
- Create: `docs/architecture-audit/golden-traces.md`
- Create: `src-tauri/src/runtime_audit/mod.rs`
- Create: `src-tauri/src/runtime_audit/trace_capture.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Test: `src-tauri/tests/golden_trace_capture.rs`

- [x] **Step 1: 写失败测试，要求通过真实 legacy emit 路径捕获事件**

```rust
use app_lib::runtime_audit::trace_capture::{capture_legacy_trace, LegacyTraceScenario};

#[tokio::test]
async fn captures_real_legacy_trace_for_basic_chat_flow() {
    let trace = capture_legacy_trace(LegacyTraceScenario::BasicChat)
        .await
        .expect("trace should be captured from legacy emit path");

    assert_eq!(
        trace.event_names(),
        vec!["streaming:delta", "message:updated", "streaming:done"]
    );
}
```

- [x] **Step 2: 运行测试确认当前没有 trace capture 支撑**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test golden_trace_capture -- --nocapture`
Expected: FAIL，原因应为 `app_lib::runtime_audit` 不存在、`capture_legacy_trace` 不存在，或当前 `chat.rs` 仍直接调用 `app.emit(...)`

- [x] **Step 3: 抽出零逻辑 legacy emit 包装器并加 test-only recorder**

```rust
// src-tauri/src/commands/chat.rs
fn emit_legacy_event<E: LegacyEventSink>(
    sink: &E,
    name: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    sink.emit(name, payload)
}

// src-tauri/src/runtime_audit/trace_capture.rs
pub trait LegacyEventSink {
    fn emit(&self, name: &str, payload: &serde_json::Value) -> anyhow::Result<()>;
}

pub struct LegacyEventRecorder { /* records ordered events */ }
```

- [x] **Step 4: 用真实路径生成 golden trace 文档，而不是手写事件序列**

```markdown
# Golden Traces

Source of truth:
- Test: src-tauri/tests/golden_trace_capture.rs
- Capture helper: src-tauri/src/runtime_audit/trace_capture.rs
- Legacy emit wrapper: src-tauri/src/commands/chat.rs

## Trace 01 - Basic Chat
Captured from current legacy flow; copy the exact event order and key payload fields from recorder output.

## Trace 02 - Single Tool
Captured from current legacy flow; include tool:executing -> tool:completed ordering and any interleaved message update.
```

- [x] **Step 5: 再写一个工具路径采样断言，覆盖 `tool:*` 顺序**

```rust
use app_lib::runtime_audit::trace_capture::{capture_legacy_trace, LegacyTraceScenario};

#[tokio::test]
async fn captures_real_legacy_trace_for_single_tool_flow() {
    let trace = capture_legacy_trace(LegacyTraceScenario::SingleTool)
        .await
        .expect("tool trace should be captured");

    assert_eq!(
        trace.event_names(),
        vec!["tool:executing", "message:updated", "tool:completed", "streaming:done"]
    );
}
```

- [x] **Step 6: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test golden_trace_capture -- --nocapture`
Expected: PASS

- [x] **Step 7: 提交**

```bash
git add docs/architecture-audit/golden-traces.md src-tauri/src/runtime_audit/mod.rs src-tauri/src/runtime_audit/trace_capture.rs src-tauri/src/commands/chat.rs src-tauri/tests/golden_trace_capture.rs
git commit -m "test: add real legacy event golden trace audit"
```

## Definition of Done

- `docs/architecture-audit/chat-responsibility-map.md` 能把 `chat.rs` 的主要职责映射到未来层级。
- `docs/architecture-audit/state-owner-matrix.md` 和 `docs/architecture-audit/event-contract-matrix.md` 锁定真实状态拥有者与 legacy 事件协议。
- Golden trace 来自真实 emit 路径，而不是手写 markdown。
- 后续 Phase 1 可以直接复用 Phase 0 采集到的事件序列做回归。
