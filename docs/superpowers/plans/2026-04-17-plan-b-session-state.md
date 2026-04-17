# Session State Owner + Turn 健壮性计划（Plan-B）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** QueryEngine 持有跨 turn 的 session 状态（FileStateCache、token usage），Turn 内部增加 cancel checkpoint，保证对话健壮性。

**Architecture:** B1/B2 在 QueryEngine 层新增 session 级状态字段；B3 在 TurnDriver 主循环增加 checkpoint；B4 保护 state 更新原子性。各子任务独立可 commit。

**Tech Stack:** Rust, tokio, async_trait

**Worktree branch:** `feat/session-state-owner`

---

## 当前代码现状（阅读后确认）

### QueryEngine struct（`src-tauri/src/runtime/query_engine.rs`）

当前字段：
```rust
pub struct QueryEngine {
    tool_dispatcher: Option<Arc<ToolDispatcher>>,
    workspace_path: Option<PathBuf>,
    authorized_workspace: Option<AuthorizedWorkspaceRef>,
    browser_available: bool,
    file_ops: Option<Arc<dyn FileOperations>>,
}
```

`run_tool_call_with_bus` 和 `run_tool_with_bus` 两处都以 `read_file_state: None` 构建 `CapabilityContext`——即每次 turn/工具调用都没有共享的文件状态缓存。

### TurnIterationState（`src-tauri/src/runtime/chat/turn_config.rs`）

```rust
pub struct TurnIterationState {
    pub messages: Vec<JsonValue>,
    pub full_content: String,
    pub generated_file_ids: Vec<String>,
    pub all_file_metas: Vec<JsonValue>,
    pub iteration_count: usize,
    pub stream_cancelled: bool,
    pub step_tokens_in: u64,
    pub step_tokens_out: u64,
    pub force_no_tools: bool,
    pub safeguard_phase1_injected: bool,
}
```

`step_tokens_in` / `step_tokens_out` 在每次 `run_chat_turn_s4` 中从 0 开始累积，无跨 turn 汇总。

### cancel checkpoint 现状（`src-tauri/src/runtime/chat/chat_turn_driver.rs`）

`run_chat_turn_s4` 中唯一的取消检查点在迭代末尾（Step 5f）：
```rust
// ── 5f: per-iteration cancel check ───────────────────────────────
if cancel.is_cancelled() {
    state.stream_cancelled = true;
    break 'turn;
}
```
LLM step 开始前、工具执行返回后均无检查点。

### FileStateCache（`src-tauri/src/runtime/tools/capability.rs`）

```rust
pub struct FileStateCache {
    cache: Mutex<lru::LruCache<PathBuf, FileState>>,
}
impl FileStateCache {
    pub fn new() -> Self { ... }       // 容量 100 条
    pub fn get(&self, path: &Path) -> Option<FileState> { ... }
    pub fn set(&self, path: PathBuf, state: FileState) { ... }
}
```

内部使用 `Mutex`，跨线程共享安全，适合 `Arc<FileStateCache>` 包装后在 QueryEngine 中持有。

---

## 子任务 B1：QueryEngine 持有 FileStateCache（跨 turn 复用）

**目标：** `FileStateCache` 作为 session 级状态由 `QueryEngine` 持有，每次 turn 调用工具时注入同一个实例到 `CapabilityContext.read_file_state`，而非始终传 `None`。

**对标：** claude-code-best QueryEngine 持有 `readFileState: FileStateCache`，每个 turn 的 ToolUseContext 共享同一实例。

### Task B1-T1：写失败测试

- [ ] 在 `src-tauri/tests/` 新建 `review_session_state_b1_file_cache_test.rs`
- [ ] 测试内容：构造 `QueryEngine`，调用 `query_engine.read_file_state()`（即将新增的 accessor），断言返回的 `Arc<FileStateCache>` 是同一个实例（`Arc::ptr_eq`）；同时断言通过 `run_tool_call_with_bus` 执行一个工具后，`CapabilityContext.read_file_state` 不为 `None`（通过 spy tool 捕获 context）。
- [ ] 运行命令验证失败：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_session_state_b1 -- --nocapture 2>&1 | tail -20
```

预期：编译错误 `no method named read_file_state found for struct QueryEngine`。

### Task B1-T2：最小实现

- [ ] 修改 `src-tauri/src/runtime/query_engine.rs`：

  1. 新增字段 `read_file_state: Arc<FileStateCache>`（需要 `use crate::runtime::tools::capability::FileStateCache`）：

  ```rust
  use crate::runtime::tools::capability::FileStateCache;

  #[derive(Clone)]
  pub struct QueryEngine {
      tool_dispatcher: Option<Arc<ToolDispatcher>>,
      workspace_path: Option<PathBuf>,
      authorized_workspace: Option<AuthorizedWorkspaceRef>,
      browser_available: bool,
      file_ops: Option<Arc<dyn FileOperations>>,
      /// Session-scoped file-read state cache, shared across all tool calls
      /// within this session so repeated reads of the same file are cheap.
      read_file_state: Arc<FileStateCache>,
  }
  ```

  2. 更新 `Default` derive 不能自动处理 `Arc<FileStateCache>`（`FileStateCache` 已实现 `Default`），改为手动实现或用 `#[derive(Default)]` 并确保 `Arc<FileStateCache>` 是 `Default`（`Arc<T: Default>` 自动实现 `Default`，所以 derive 仍可用）。

  3. 更新 `with_dispatcher` 和 `for_test` 构造函数：

  ```rust
  pub fn with_dispatcher(tool_dispatcher: Arc<ToolDispatcher>) -> Self {
      Self {
          tool_dispatcher: Some(tool_dispatcher),
          workspace_path: None,
          authorized_workspace: None,
          browser_available: false,
          file_ops: None,
          read_file_state: Arc::new(FileStateCache::new()),
      }
  }
  ```

  4. 添加 accessor 方法：

  ```rust
  /// Return the session-scoped file-read state cache.
  /// All tool calls dispatched through this engine share the same instance.
  pub fn read_file_state(&self) -> Arc<FileStateCache> {
      self.read_file_state.clone()
  }
  ```

  5. 在 `run_tool_call_with_bus` 的 `CapabilityContext` 构建处将 `read_file_state: None` 改为 `read_file_state: Some(self.read_file_state.clone())`：

  ```rust
  // 修改前（两处，run_tool_call_with_bus 和 run_tool_with_bus）：
  read_file_state: None,
  // 修改后：
  read_file_state: Some(self.read_file_state.clone()),
  ```

  **注意**：需修改两处——`run_tool_call_with_bus`（约第 210 行）和 `run_tool_with_bus`（约第 350 行）均有构建 `CapabilityContext` 的代码。

- [ ] 测试中补写 spy tool，让 `execute` 捕获 context 中的 `read_file_state`：

  ```rust
  // SpyCapabilityTool: 执行时把 ctx.capability() 的 read_file_state 存到共享变量
  struct SpyCapabilityTool {
      name: &'static str,
      captured: Arc<Mutex<Option<Arc<FileStateCache>>>>,
  }
  #[async_trait]
  impl RuntimeTool for SpyCapabilityTool {
      fn definition(&self) -> ToolDefinition { ToolDefinition::new(self.name, "spy") }
      async fn execute(&self, _input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
          if let Some(cap) = ctx.capability() {
              *self.captured.lock().unwrap() = cap.read_file_state.clone();
          }
          Ok(ToolResult::new(self.name, "ok", None))
      }
  }
  ```

- [ ] 运行命令验证通过：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_session_state_b1 -- --nocapture 2>&1 | tail -20
```

### Task B1-T3：验证存量测试不回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

### Task B1-T4：Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/query_engine.rs src-tauri/tests/review_session_state_b1_file_cache_test.rs
git commit -m "feat(query-engine): session-scoped FileStateCache — B1"
```

---

## 子任务 B2：QueryEngine 持有 total_usage（跨 turn 累积）

**目标：** token 用量跨 turn 累积，不在每次 turn 重置。`QueryEngine` 持有 `Arc<Mutex<TotalTokenUsage>>`，turn 结束后将 `state.step_tokens_in/out` 累加进去，并提供 `get_total_usage()` 方法供外部查询（监控/日志）。

### Task B2-T1：写失败测试

- [ ] 在 `src-tauri/tests/` 新建 `review_session_state_b2_total_usage_test.rs`
- [ ] 测试内容：

  ```rust
  // 测试1: get_total_usage 方法可调用，初始值为 (0, 0)
  // 测试2: accumulate_usage(tokens_in, tokens_out) 调用后，get_total_usage 返回累计值
  // 测试3: 多次 accumulate 后值正确叠加
  ```

- [ ] 运行命令验证失败：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_session_state_b2 -- --nocapture 2>&1 | tail -20
```

预期：编译错误，`QueryEngine` 没有 `get_total_usage` / `accumulate_usage` 方法。

### Task B2-T2：最小实现

- [ ] 修改 `src-tauri/src/runtime/query_engine.rs`：

  1. 在文件顶部新增 `use std::sync::Mutex;`（`Arc` 已有）。

  2. 新增内部结构体（在 `query_engine.rs` 内部，**不** 引入 `crate::llm::streaming::TokenUsage`，避免跨层依赖，直接用两个 `u64`）：

  ```rust
  /// Accumulated token usage across all turns within a session.
  #[derive(Debug, Clone, Default)]
  pub struct TotalTokenUsage {
      pub tokens_in: u64,
      pub tokens_out: u64,
  }
  ```

  3. `QueryEngine` struct 新增字段：

  ```rust
  /// Accumulated token usage across all turns in this session.
  total_usage: Arc<Mutex<TotalTokenUsage>>,
  ```

  4. 更新所有构造函数（`with_dispatcher`、`Default`/`new` 路径）：

  ```rust
  total_usage: Arc::new(Mutex::new(TotalTokenUsage::default())),
  ```

  5. 新增两个公开方法：

  ```rust
  /// Return a snapshot of the accumulated token usage for this session.
  pub fn get_total_usage(&self) -> TotalTokenUsage {
      self.total_usage
          .lock()
          .expect("total_usage mutex poisoned")
          .clone()
  }

  /// Add token counts from a completed turn to the session total.
  ///
  /// Called by `ChatTurnDriver` after each successful turn iteration ends.
  pub fn accumulate_usage(&self, tokens_in: u64, tokens_out: u64) {
      let mut guard = self.total_usage
          .lock()
          .expect("total_usage mutex poisoned");
      guard.tokens_in += tokens_in;
      guard.tokens_out += tokens_out;
  }
  ```

- [ ] 修改 `src-tauri/src/runtime/chat/chat_turn_driver.rs`，在 `run_chat_turn_s4` 的 Step 7（持久化 assistant message）之前调用 `accumulate_usage`：

  ```rust
  // ── Step 6b: Accumulate session token usage ───────────────────────────
  self.query_engine.accumulate_usage(state.step_tokens_in, state.step_tokens_out);
  ```

  位置：紧接在 `post_process::finalize_content(...)` 之后、`executor.persist_assistant_message(...)` 之前（现有 Step 6 和 Step 7 之间）。

- [ ] 运行命令验证通过：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_session_state_b2 -- --nocapture 2>&1 | tail -20
```

### Task B2-T3：验证存量测试不回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

### Task B2-T4：Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/query_engine.rs src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/tests/review_session_state_b2_total_usage_test.rs
git commit -m "feat(query-engine): session-scoped TotalTokenUsage accumulation — B2"
```

---

## 子任务 B3：Turn 内部多处 cancel checkpoint

**目标：** 在 `run_chat_turn_s4` 的关键位置各增加一个 cancel check，不只在 iteration 末尾（当前 Step 5f）检查。参照 claude-code-best `query.ts` 6 处 checkpoint 模式。

**要增加的检查点：**

| 位置 | 编号 | 说明 |
|------|------|------|
| LLM step 调用（`executor.run_llm_step`）之前 | CP-1 | 避免已取消的情况下再发 LLM 请求 |
| `execute_round` 返回之后（工具执行后） | CP-2 | 工具执行可能耗时很久，完成后立即检查 |
| 每个 `state.messages.push(msg)` 工具结果合并后 | CP-3 | 多工具 round 中每条 merge 后检查 |

### Task B3-T1：写失败测试

- [ ] 在 `src-tauri/tests/` 新建 `review_session_state_b3_cancel_checkpoints_test.rs`

- [ ] 测试内容：使用可控取消 token + mock executor 来验证 CP-1。

  CP-1 测试设计：
  - 构造一个在迭代内 `cancel.cancel()` 的 mock executor（在 `run_llm_step` 之前由外部取消）
  - 外部在 `run_chat_turn_s4` 开始前通过 `turn.cancellation().cancel()` 取消
  - 断言：mock executor 的 `run_llm_step` 从未被调用（如果 CP-1 存在），否则被调用了（当前行为）

  实现方式：用 `Arc<Mutex<u32>>` 计数器的 mock executor，测试断言 `call_count == 0`。

  ```rust
  struct CountingExecutor { call_count: Arc<Mutex<u32>> }
  #[async_trait]
  impl RuntimeLlmExecutor for CountingExecutor {
      async fn run_llm_step(&self, ...) -> Result<LlmStepResult, TurnError> {
          *self.call_count.lock().unwrap() += 1;
          Ok(LlmStepResult::ContentComplete { content: "done".into(), tokens_in: 0, tokens_out: 0 })
      }
      async fn persist_assistant_message(&self, ...) -> Result<String, TurnError> { Ok("msg-1".into()) }
      // 其余方法使用 default 实现
  }
  ```

  测试调用：turn cancel 后调用 `run_chat_turn`，断言 `call_count == 0`。

- [ ] 运行命令验证失败（当前实现会先发 LLM 请求再在 5f 检查）：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_session_state_b3 -- --nocapture 2>&1 | tail -20
```

预期：`call_count != 0`，断言失败，因为当前无 CP-1。

### Task B3-T2：最小实现

- [ ] 修改 `src-tauri/src/runtime/chat/chat_turn_driver.rs`，在 `'turn` 循环体内、`executor.run_llm_step` 调用前增加 CP-1：

  ```rust
  // ── CP-1: cancel check before LLM step ──────────────────────────────
  if cancel.is_cancelled() {
      state.stream_cancelled = true;
      break 'turn;
  }

  // ── Step 5b: single LLM step ─────────────────────────────────────
  let step_result = executor
      .run_llm_step(&input, &self.event_bus, &cancel)
      .await
      .map_err(|e| anyhow::anyhow!("{}", e))?;
  ```

  位置：在 `let input = LlmStepInput { ... };` 构建之后，`executor.run_llm_step(...)` 之前。

- [ ] 在工具结果 merge 循环之后增加 CP-2（`execute_round` 返回后）：

  ```rust
  // Execute the tool round.
  let round_results = round_driver
      .execute_round(turn, &self.event_bus, tool_calls)
      .await;

  // ── CP-2: cancel check after tool round ──────────────────────────
  if cancel.is_cancelled() {
      state.stream_cancelled = true;
      break 'turn;
  }

  // Collect and merge results into state.
  let results = tool_result_collector::collect_results(round_results, 8000);
  ```

  位置：`execute_round` 之后，`tool_result_collector::collect_results` 之前。

- [ ] 在 `for msg in results.tool_result_messages` 的循环体内增加 CP-3（每条工具结果 merge 后）：

  ```rust
  for msg in results.tool_result_messages {
      state.messages.push(msg);
      // ── CP-3: cancel check after each tool result merge ─────────
      if cancel.is_cancelled() {
          state.stream_cancelled = true;
          break 'turn;
      }
  }
  ```

  注意：`break 'turn` 需要在 `for msg in` 循环内使用，Rust 支持从内层循环 break 到带标签的外层循环。

- [ ] 运行命令验证通过：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_session_state_b3 -- --nocapture 2>&1 | tail -20
```

### Task B3-T3：验证存量取消测试不回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_cancelled review_query_engine_cancellation -- --nocapture 2>&1 | tail -20
```

### Task B3-T4：验证所有 review_ 测试不回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

### Task B3-T5：Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/tests/review_session_state_b3_cancel_checkpoints_test.rs
git commit -m "feat(turn-driver): add CP-1/CP-2/CP-3 cancel checkpoints in run_chat_turn_s4 — B3"
```

---

## 子任务 B4：Turn state 不可变更新保护

**目标：** `TurnIterationState.messages` 的更新逻辑改为每次迭代使用 **替换式** 更新（创建新 vec，追加后 replace），而非直接 `push` 到同一 `Vec`。这样 cancel 后 state 不会处于半更新状态（assistant message 已 push，但工具结果尚未 push）。

**当前问题：**

在 `ToolCalls` 分支中：
```rust
// 第一步：push assistant message
state.messages.push(serde_json::json!({ "role": "assistant", "content": assistant_content }));
// ... 工具执行（可能耗时几秒）...
// 第二步：push tool result messages（多条）
for msg in results.tool_result_messages {
    state.messages.push(msg);
}
```

如果 cancel 发生在工具执行过程中，`messages` 已经含有 assistant message 但缺少对应的 tool results，导致 messages 序列不合法（assistant 后跟非 tool 结果）。

**解决方案：** 先收集所有要追加的消息，构建新的 messages vec，一次性 replace。

### Task B4-T1：写失败测试

- [ ] 在 `src-tauri/tests/` 新建 `review_session_state_b4_atomic_state_update_test.rs`

- [ ] 测试内容：验证如果 cancel 发生，`state.messages` 不会出现 `role=assistant` 后面紧跟非 `role=tool` 消息的不合法序列。

  具体测试：使用一个在工具执行时触发取消的 mock executor，执行后断言 messages 序列合法性：
  - 每条 `role=assistant` 后面如果有下一条，要么是 `role=user`（新一轮）要么是 `role=tool`
  - 没有孤立的 assistant tool-use message 后面跟 user 消息

  由于 B4 的 mock 较复杂，可以简化测试为：验证方法 `TurnIterationState::append_messages_batch`（即将新增）可用，并且它是原子式替换 vec 的：

  ```rust
  // 测试 TurnIterationState::append_messages_batch 方法
  let mut state = TurnIterationState::new(vec![json!({"role": "user", "content": "hi"})]);
  let batch = vec![
      json!({"role": "assistant", "content": "thinking"}),
      json!({"role": "tool", "content": "result", "tool_call_id": "tc-1"}),
  ];
  state.append_messages_batch(batch);
  assert_eq!(state.messages.len(), 3);
  ```

- [ ] 运行命令验证失败：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_session_state_b4 -- --nocapture 2>&1 | tail -20
```

预期：编译错误，`TurnIterationState` 没有 `append_messages_batch` 方法。

### Task B4-T2：最小实现

- [ ] 修改 `src-tauri/src/runtime/chat/turn_config.rs`，为 `TurnIterationState` 添加 `append_messages_batch` 方法：

  ```rust
  impl TurnIterationState {
      // ... 现有 new() ...

      /// Atomically append a batch of messages to the conversation history.
      ///
      /// All messages in `batch` are appended together via a single `extend`
      /// so that the `messages` vec is never in a partially-updated state.
      /// Prefer this over calling `messages.push()` multiple times across
      /// distinct code paths where a cancel could interrupt between pushes.
      pub fn append_messages_batch(&mut self, batch: Vec<serde_json::Value>) {
          self.messages.extend(batch);
      }
  }
  ```

- [ ] 修改 `src-tauri/src/runtime/chat/chat_turn_driver.rs`，将 `ToolCalls` 分支中的 messages 更新改为使用 `append_messages_batch`：

  **修改前（当前代码，chat_turn_driver.rs 约第 432-453 行）：**
  ```rust
  LlmStepResult::ToolCalls {
      assistant_content,
      tool_calls,
      tokens_in,
      tokens_out,
  } => {
      if !assistant_content.is_empty() {
          state.full_content.push_str(&assistant_content);
          state.messages.push(serde_json::json!({
              "role": "assistant",
              "content": assistant_content,
          }));
      }
      state.step_tokens_in += tokens_in;
      state.step_tokens_out += tokens_out;
      state.iteration_count = iteration + 1;

      let round_results = round_driver
          .execute_round(turn, &self.event_bus, tool_calls)
          .await;

      // CP-2（B3 已添加）

      let results = tool_result_collector::collect_results(round_results, 8000);
      for msg in results.tool_result_messages {
          state.messages.push(msg);  // ← 改这里
      }
  ```

  **修改后：**
  ```rust
  LlmStepResult::ToolCalls {
      assistant_content,
      tool_calls,
      tokens_in,
      tokens_out,
  } => {
      // Collect assistant message + token counts before dispatching tools.
      if !assistant_content.is_empty() {
          state.full_content.push_str(&assistant_content);
      }
      state.step_tokens_in += tokens_in;
      state.step_tokens_out += tokens_out;
      state.iteration_count = iteration + 1;

      let round_results = round_driver
          .execute_round(turn, &self.event_bus, tool_calls)
          .await;

      // CP-2（B3 已添加）
      if cancel.is_cancelled() {
          state.stream_cancelled = true;
          break 'turn;
      }

      // Build the batch atomically: assistant message (if any) + all tool results.
      // Using append_messages_batch ensures the messages vec is never left in
      // a half-updated state where an assistant tool-use message exists but
      // the corresponding tool results are missing.
      let results = tool_result_collector::collect_results(round_results, 8000);
      let mut batch = Vec::with_capacity(1 + results.tool_result_messages.len());
      if !assistant_content.is_empty() {
          batch.push(serde_json::json!({
              "role": "assistant",
              "content": assistant_content,
          }));
      }
      batch.extend(results.tool_result_messages);
      state.append_messages_batch(batch);

      state.all_file_metas.extend(results.new_file_metas);
      state.generated_file_ids.extend(results.new_generated_file_ids);
  ```

  注意：`assistant_content` 是 `String`，move 语义——在 `if !assistant_content.is_empty()` 中先 `push_str` 到 `full_content`，再 clone 到 batch。可改为先 clone：

  ```rust
  if !assistant_content.is_empty() {
      state.full_content.push_str(&assistant_content);
  }
  // ... execute_round ...
  if !assistant_content.is_empty() {
      batch.push(serde_json::json!({ "role": "assistant", "content": assistant_content }));
  }
  ```

  Rust 会报 `assistant_content` 已移动。解决方法：在 `ToolCalls` match arm 头部克隆一份：

  ```rust
  LlmStepResult::ToolCalls { assistant_content, tool_calls, tokens_in, tokens_out } => {
      let assistant_msg_for_history = if !assistant_content.is_empty() {
          state.full_content.push_str(&assistant_content);
          Some(serde_json::json!({ "role": "assistant", "content": assistant_content }))
      } else {
          None
      };
      // ...
      let mut batch = Vec::with_capacity(
          assistant_msg_for_history.is_some() as usize + results.tool_result_messages.len()
      );
      if let Some(msg) = assistant_msg_for_history {
          batch.push(msg);
      }
      batch.extend(results.tool_result_messages);
      state.append_messages_batch(batch);
  }
  ```

- [ ] 运行命令验证通过：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_session_state_b4 -- --nocapture 2>&1 | tail -20
```

### Task B4-T3：验证所有 review_ 测试不回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

### Task B4-T4：Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && git add src-tauri/src/runtime/chat/turn_config.rs src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/tests/review_session_state_b4_atomic_state_update_test.rs
git commit -m "feat(turn-driver): atomic messages batch update via append_messages_batch — B4"
```

---

## 全量回归验证

所有 B1-B4 实现后，运行：

```bash
# Rust 全部测试
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test 2>&1 | tail -40

# 专门跑 review_ 系列
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -40

# 前端单测（无 Rust 变更对前端无影响，确认无误）
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm test 2>&1 | tail -20
```

---

## 关键文件路径索引

| 文件 | 修改内容 |
|------|----------|
| `src-tauri/src/runtime/query_engine.rs` | B1: 新增 `read_file_state` 字段 + accessor；B2: 新增 `total_usage` 字段 + `accumulate_usage` / `get_total_usage` |
| `src-tauri/src/runtime/chat/turn_config.rs` | B4: 新增 `TurnIterationState::append_messages_batch` |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | B2: Step 6b 调用 `accumulate_usage`；B3: 新增 CP-1/CP-2/CP-3；B4: 改用 `append_messages_batch` 原子更新 |
| `src-tauri/tests/review_session_state_b1_file_cache_test.rs` | B1 测试 |
| `src-tauri/tests/review_session_state_b2_total_usage_test.rs` | B2 测试 |
| `src-tauri/tests/review_session_state_b3_cancel_checkpoints_test.rs` | B3 测试 |
| `src-tauri/tests/review_session_state_b4_atomic_state_update_test.rs` | B4 测试 |

---

## 实施顺序建议

B1 → B2 → B3 → B4，每个子任务独立可 commit，互不依赖。B3 和 B4 都改 `chat_turn_driver.rs`，建议在同一 branch 上顺序实施，避免冲突。若并行实施需注意 B3 增加的 CP-2 和 B4 对同一代码块的修改应在 B4 时保留 B3 的 checkpoint。
