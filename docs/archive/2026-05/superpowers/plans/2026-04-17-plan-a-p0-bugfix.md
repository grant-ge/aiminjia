# P0 Bug 修复计划（Plan-A）

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans`（或 `superpowers:subagent-driven-development`）按 Task 顺序逐个执行。本计划已按 `claude-code-best` 实际源码语义校准。

**Goal:** 修复 6 项 P0 级正确性问题，消除 cancel 后对话历史不合法、权限 Ask 无法到达前端、registry poison panic、Python session 串扰、sandbox 路径边界绕过、`build_env_info` 阻塞 tokio 线程等风险。

**Architecture:**
- A1 修复 S4 driver 的“assistant tool call / tool result”轨迹完整性
- A2 修复 Ask 事件从 runtime 到前端的路由，但本期仍不做阻塞式等待用户响应
- A3 仅修复 `RuntimeRunRegistry` 的 poison panic，保持同步 API，不做高扩散 async 化
- A4 收敛 Python 会话作用域到 `run_id`，移除生产路径的 conversation 级 fallback
- A5 修复 embedded Python sandbox 的路径前缀绕过
- A6 将 `build_env_info` 改成异步 git 子进程，避免阻塞运行时

**Tech Stack:** Rust, tokio, async_trait, embedded Python sandbox preamble（`src-tauri/src/python/sandbox.rs`）

**Worktree branch:** `fix/p0-bugfix`

---

## 对标校准纪要

### 参考来源
- Cancel / abort 语义：`/Users/a20250311/github/claude-code-best/src/query.ts`
- Ask 路由语义：`/Users/a20250311/github/claude-code-best/src/services/tools/toolExecution.ts`
- AbortController / run 生命周期：`/Users/a20250311/github/claude-code-best/src/QueryEngine.ts`
- lotus 旧的正确消息形态：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

### 对齐原则
- `claude-code-best` 的核心不变量不是“某个分支里补一条字符串消息”，而是：**只要 assistant 发出了 tool request，后续历史里就必须有与之配对的 tool result，才能安全进入下一轮模型调用。**
- lotus S4 路径当前使用的是归一化 `ChatMessage` 形态，而不是直接存 Anthropic `content[].tool_use` block。因此 A1 的修复必须以 lotus 当前消息模型为准：`assistant.toolCalls[*].id` ↔ `tool.toolCallId` 配对。
- `AskRequired` 在当前 runtime 中已经是结构化 outcome；本期缺的是 **driver emit runtime event + adapter 映射**，不是把 UI 逻辑塞回 `QueryEngine`。
- `RuntimeRunRegistry` 当前锁不跨 `await`，P0 在于 poison panic，不在于必须换成 async mutex。为避免调用链大爆炸，A3 采用最小 blast radius 修法。
- Python session 的主路径已经大量采用 `run_id`；A4 重点是消灭剩余 production fallback，而不是重写整个 session manager。

---

## 执行约束

- 严格按 A1 → A2 → A3 → A4 → A5 → A6 执行，一次只做一个 Task。
- 每个 Task 必须遵循 TDD：先写失败测试 → 确认失败 → 最小实现 → 确认通过 → commit。
- 每个 Task 完成后立即停下汇报，不得连续推进到下一个 Task。
- `runtime/` 内禁止引入 `tauri::*` 依赖；事件桥接只放在 transport 层。
- 若实现过程中发现与本计划或 `claude-code-best` 仍有偏差，以 `claude-code-best` 的真实语义为准，并先更新计划再继续编码。

---

## 改造视角

### A1：Cancel 后补齐缺失的 tool result，并保留 assistant tool call 轨迹

**当前状态**：
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` 在 `LlmStepResult::ToolCalls` 分支只在 `assistant_content` 非空时推入纯文本 assistant message，没有把 `tool_calls` 一并写入 `state.messages`。
- 因此下一轮 LLM 输入里可能出现“tool result 没有前序 assistant tool call”或“cancel 后 assistant tool call 没有匹配 tool result”的残缺轨迹。
- 这与 `claude-code-best` 在 abort 时通过 `yieldMissingToolResultBlocks(...)` 维持轨迹完整性的设计不一致。

**目标状态**：
- 每次 `ToolCalls` 结果都要把 assistant 的 `toolCalls` 写入 `state.messages`，对齐 lotus 旧路径的 `ChatMessage::assistant_with_tool_calls(...)`。
- cancel 或迭代末尾检测到取消时，补齐所有缺失的 synthetic tool result，保证每个 assistant tool call 都有匹配的 `tool.toolCallId`。
- synthetic 文案统一为：`Tool execution was interrupted by user cancellation.`

**迁移路径**：
1. 在 ToolCalls 分支先把 assistant + `toolCalls` 轨迹写入 `state.messages`
2. 提取纯函数/小 helper 扫描消息历史并注入缺失的 synthetic tool result
3. 在 `LlmStepResult::Cancelled` 和轮次末尾 cancel 检查处都调用该 helper
4. 保持正常 tool result 收集逻辑不变

**回归验证**：
- `cargo test --test p0_a1_cancel_synthetic_tool_result_test -- --nocapture`
- `cargo test --test s4_driver_loop_test -- --nocapture`

---

### A2：权限 Ask 路径接通到前端

**当前状态**：
- `src-tauri/src/runtime/query_engine.rs` 已经把权限 Ask 保留为 `RuntimeToolCallOutcome::AskRequired`，这一步本身没有丢语义。
- 真正丢语义的是 `src-tauri/src/runtime/chat/tool_result_collector.rs`：它把 `AskRequired` 通过 `outcome.content()` 降级成普通 tool result 文本。
- `RuntimeEventKind` 里没有 `PermissionAskRequired`，`src-tauri/src/transport/tauri_event_adapter.rs` 也没有 `permission:ask` 的映射，所以前端看不到 Ask。

**目标状态**：
- driver 在收集 tool results 之前，先为每个 `AskRequired` outcome emit `RuntimeEventKind::PermissionAskRequired`。
- adapter 把该 runtime event 映射为 legacy 事件 `permission:ask`，payload 至少包含 `conversationId`、`runId`、`toolCallId`、`toolName`、`message`、`suggestions`。
- 本期仍保留 tool result 的文本 fallback 让 LLM 继续收到反馈；阻塞式“等用户点 Allow/Deny 再继续”留到后续专项。

**迁移路径**：
1. `events.rs` 新增 `PermissionAskRequired`
2. `tauri_event_adapter.rs` 新增 `permission:ask` 映射
3. `chat_turn_driver.rs` 在 `collect_results(...)` 之前发 Ask 事件
4. `query_engine.rs` 只保留结构化 outcome，不承担前端事件职责

**回归验证**：
- `cargo test --test p0_a2_permission_ask_routing_test -- --nocapture`
- `cargo test --test tauri_event_adapter_test -- --nocapture`

---

### A3：消除 RuntimeRunRegistry 的 poison panic，保持同步 API

**当前状态**：
- `src-tauri/src/runtime/run_registry.rs` 全部使用 `std::sync::Mutex::lock().unwrap()`。
- 一旦某个持锁路径 panic，mutex 进入 poisoned，后续所有 `.unwrap()` 都会再次 panic，造成进程级连锁崩溃。
- 但该 registry 当前的锁并不跨 `await`，若强行改成 `tokio::sync::Mutex + async fn`，会把改动扩散到 `gateway.rs` 等整条调用链，超出本 Task 的最小修复面。

**目标状态**：
- registry 对 poison 可恢复：后续操作记录告警并继续使用 `into_inner()` 恢复，不再 panic。
- 保持 `reserve / attach_stream / cancel / clear / is_busy ...` 现有同步签名，避免高扩散重构。

**迁移路径**：
1. 在 `run_registry.rs` 增加统一的私有加锁 helper，封装 poison recovery
2. 替换所有 `.lock().unwrap()`
3. 新增单元测试直接制造 poison，验证后续操作不 panic
4. 跑现有 `runtime_run_registry_test` 保证外部语义不变

**回归验证**：
- `cargo test run_registry -- --nocapture`
- `cargo test --test runtime_run_registry_test -- --nocapture`

---

### A4：Python session 生产路径强制 per-run

**当前状态**：
- `src-tauri/src/python/session.rs` 已提供 `session_key_for_run(...)`、`execute_for_run(...)`、`interrupt_run(...)`、`destroy_run(...)`。
- 主分析路径已经大量使用 per-run API，例如 `chat_runtime_impl.rs` 的 precompute 执行。
- 仍残留的风险点是 `src-tauri/src/llm/tool_executor/python.rs`：旧持久 Python 分支若 `ctx.run_id` 缺失，会 fallback 到 `session_manager.execute(&ctx.conversation_id, ...)`，重新退化为 conversation scope。

**目标状态**：
- 旧持久 Python 分支的生产路径必须显式要求 `run_id`，不能再悄悄退回到 conversation scope。
- conversation-scope 的 `execute / interrupt / destroy` 仅保留给 legacy / 非 run-aware 调用者，不再被分析主路径依赖。

**迁移路径**：
1. 先审计所有 callsite，确认真正的 production 漏口
2. 将 `llm/tool_executor/python.rs` 中 analysis 分支的 conversation fallback 改为显式错误或等价硬保护
3. 保留 `session.rs` 的 legacy API，但补注释明确其非主路径定位
4. 用测试锁死“analysis 必须有 run_id”的语义

**回归验证**：
- `cargo test --test python_run_scope_test -- --nocapture`
- `cargo test python --tests -- --nocapture`

---

### A5：Sandbox 路径边界绕过修复

**当前状态**：
- 漏洞实际位于 `src-tauri/src/python/sandbox.rs` 生成的 embedded Python preamble 中，不是独立 `sandbox.py` 文件。
- `_safe_open` 目前用 `abs_path.startswith(os.path.realpath(p))` 判断是否落在白名单内，会把 `/workspace.backup/...` 错当成 `/workspace/...` 子路径。

**目标状态**：
- 白名单判断必须要求“等于根目录”或“以 `root + os.sep` 开头”。
- 保持 workspace 根目录本身、以及其真实子目录仍可写。

**迁移路径**：
1. 修改 `sandbox.rs` 中 preamble 里的 `_safe_open`
2. 增加测试锁定生成代码必须包含 path-separator 边界判断
3. 跑已有 sandbox 相关测试回归

**回归验证**：
- `cargo test --test p0_a5_sandbox_path_boundary_test -- --nocapture`
- `cargo test sandbox -- --nocapture`

---

### A6：`build_env_info` 改成异步 git 子进程

**当前状态**：
- `src-tauri/src/runtime/chat/context_builder.rs` 的 `build_env_info(...)` 仍用 `std::process::Command::new("git").output()`。
- 该函数已经被 async 调用链间接调用（`TauriLegacyTurnExecutor::get_env_info` 本身就是 async），同步子进程会阻塞 tokio worker 线程。

**目标状态**：
- `build_env_info(...)` 改成 `async fn`
- git status 改用 `tokio::process::Command`
- 所有调用方和单测统一加 `.await`

**迁移路径**：
1. 先把 `context_builder.rs` 的函数签名升级为 async
2. 再更新 `transport/tauri_commands/chat.rs` 的调用点
3. 最后更新 `context_builder.rs` 现有单测和新增回归测试

**回归验证**：
- `cargo test --test p0_a6_env_info_async_test -- --nocapture`
- `cargo test build_env_info -- --nocapture`

---

## Task A1：补齐 tool-call 轨迹与 cancel synthetic tool_result

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Add tests: `src-tauri/tests/p0_a1_cancel_synthetic_tool_result_test.rs`

- [ ] **Step A1-1: 先写失败测试**
  - 测试 1：第一轮返回 `ToolCalls` 后，第二轮 `run_llm_step(...)` 收到的 `input.messages` 中必须出现 assistant message，且其 `toolCalls[0].id == tool_call_id`
  - 测试 2：对“assistant 已发 tool call、tool result 尚未写回”场景执行 cancel helper，必须补出 `role=tool` + `toolCallId=<id>` 的 synthetic result
  - 测试 3：driver cancel 后仍发出 `StreamDone`

- [ ] **Step A1-2: 验证测试失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a1_cancel_synthetic_tool_result_test -- --nocapture
```

- [ ] **Step A1-3: 最小实现**
  - 在 `LlmStepResult::ToolCalls` 分支里，无论 `assistant_content` 是否为空，都写入一个归一化 assistant message：`role=assistant`，`content=<assistant_content>`，`toolCalls=[...]`
  - 提取 helper，扫描 `state.messages` 里 assistant 的 `toolCalls[*].id`，找出尚无匹配 `tool.toolCallId` 的 id，并补写 synthetic tool result
  - 该 helper 至少在两处调用：
    - `LlmStepResult::Cancelled` 分支
    - 每轮末尾 `if cancel.is_cancelled()` 的 break 之前

- [ ] **Step A1-4: 验证通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a1_cancel_synthetic_tool_result_test -- --nocapture && \
  cargo test --test s4_driver_loop_test -- --nocapture
```

- [ ] **Step A1-5: Commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  git add src-tauri/src/runtime/chat/chat_turn_driver.rs \
          src-tauri/tests/p0_a1_cancel_synthetic_tool_result_test.rs && \
  git commit -m "fix(runtime): preserve assistant tool calls and inject synthetic tool results on cancel"
```

---

## Task A2：把 AskRequired 路由到 runtime event 与前端 adapter

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Add tests: `src-tauri/tests/p0_a2_permission_ask_routing_test.rs`

- [ ] **Step A2-1: 先写失败测试**
  - `RuntimeEventKind::PermissionAskRequired` 存在，且会自动填充 `event.tool_call_id`
  - `map_runtime_event(...)` 把它映射到 `permission:ask`
  - driver 在 tool round 返回 `AskRequired` 时会发出该 runtime event

- [ ] **Step A2-2: 验证测试失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a2_permission_ask_routing_test -- --nocapture
```

- [ ] **Step A2-3: 最小实现**
  - `events.rs` 新增 `PermissionAskRequired { tool_call_id, tool_name, message, suggestions }`
  - `tauri_event_adapter.rs` 映射为 `permission:ask`
  - `chat_turn_driver.rs` 在 `collect_results(...)` 前遍历 `round_results`，遇到 `AskRequired` 就 emit 事件
  - `tool_result_collector.rs` 的文本 fallback 暂时保留，不做阻塞式等待

- [ ] **Step A2-4: 验证通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a2_permission_ask_routing_test -- --nocapture && \
  cargo test --test tauri_event_adapter_test -- --nocapture
```

- [ ] **Step A2-5: Commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  git add src-tauri/src/runtime/events.rs \
          src-tauri/src/transport/tauri_event_adapter.rs \
          src-tauri/src/runtime/chat/chat_turn_driver.rs \
          src-tauri/tests/p0_a2_permission_ask_routing_test.rs && \
  git commit -m "feat(permissions): route AskRequired to runtime event and legacy adapter"
```

---

## Task A3：修复 RuntimeRunRegistry 的 poison panic

**Files:**
- Modify: `src-tauri/src/runtime/run_registry.rs`
- Reuse tests: `src-tauri/tests/runtime_run_registry_test.rs`
- Add unit tests in: `src-tauri/src/runtime/run_registry.rs`

- [ ] **Step A3-1: 先写失败测试**
  - 在 `run_registry.rs` 的单元测试里故意 poison `active_runs`
  - 断言 poison 之后 `reserve / cancel / clear / is_busy` 不再 panic，而是恢复继续工作

- [ ] **Step A3-2: 验证测试失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test run_registry -- --nocapture
```

- [ ] **Step A3-3: 最小实现**
  - 给 `RuntimeRunRegistry` 增加统一的 `lock_active_runs()` 私有 helper
  - `match self.active_runs.lock()`：
    - `Ok(guard)` 正常返回
    - `Err(poisoned)` 记录告警后 `poisoned.into_inner()`
  - 替换所有 `.lock().unwrap()`
  - 不修改 public API 签名，不引入 `tokio::sync::Mutex`

- [ ] **Step A3-4: 验证通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test run_registry -- --nocapture && \
  cargo test --test runtime_run_registry_test -- --nocapture
```

- [ ] **Step A3-5: Commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  git add src-tauri/src/runtime/run_registry.rs \
          src-tauri/tests/runtime_run_registry_test.rs && \
  git commit -m "fix(runtime): recover poisoned run registry mutex without panicking"
```

---

## Task A4：旧持久 Python 分支强制使用 run-scoped Python session

**Files:**
- Modify: `src-tauri/src/llm/tool_executor/python.rs`
- Optional docs tweak: `src-tauri/src/python/session.rs`
- Reuse/add tests: `src-tauri/tests/python_run_scope_test.rs`，必要时补充 `python.rs` 内部单测

- [ ] **Step A4-1: 先审计调用点并记录结论**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  rg -n "execute_for_run|\.execute\(&ctx\.conversation_id|interrupt_run|destroy_run|session_key_for_run" src tests
```
  - 预期结论：主路径基本已走 per-run，风险点集中在 `llm/tool_executor/python.rs`

- [ ] **Step A4-2: 先写失败测试**
  - 测试 1：`session_key_for_run(...)` 与已有 `python_run_scope_test` 继续锁住 run scope
  - 测试 2：旧持久 Python 分支若缺少 `run_id`，不允许再静默回退到 conversation-scoped session

- [ ] **Step A4-3: 验证测试失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test python_run_scope -- --nocapture
```

- [ ] **Step A4-4: 最小实现**
  - 修改 `src-tauri/src/llm/tool_executor/python.rs`
  - 在 analysis 分支中：
    - `Some(run_id)` → `execute_for_run(run_id, ...)`
    - `None` → 返回显式错误，禁止 fallback 到 `execute(&ctx.conversation_id, ...)`
  - 如需要，给 `session.rs` 的 conversation-scope API 补注释：仅 legacy / 非 run-aware 路径使用

- [ ] **Step A4-5: 验证通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test python_run_scope_test -- --nocapture && \
  cargo test python --tests -- --nocapture
```

- [ ] **Step A4-6: Commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  git add src-tauri/src/llm/tool_executor/python.rs \
          src-tauri/src/python/session.rs \
          src-tauri/tests/python_run_scope_test.rs && \
  git commit -m "fix(python): require run-scoped sessions for analysis execution"
```

---

## Task A5：修复 sandbox 路径前缀绕过

**Files:**
- Modify: `src-tauri/src/python/sandbox.rs`
- Add tests: `src-tauri/tests/p0_a5_sandbox_path_boundary_test.rs`

- [ ] **Step A5-1: 先写失败测试**
  - preamble 中必须包含 `abs_path == root` 或 `abs_path.startswith(root + os.sep)` 语义
  - 禁止继续出现裸 `startswith(os.path.realpath(p))`
  - workspace 根目录和真实子目录仍然被允许

- [ ] **Step A5-2: 验证测试失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a5_sandbox_path_boundary_test -- --nocapture
```

- [ ] **Step A5-3: 最小实现**
  - 修改 `src-tauri/src/python/sandbox.rs` 中 preamble 的 `_safe_open`
  - 将：
    - `abs_path.startswith(os.path.realpath(p))`
  - 改为：
    - `abs_path == os.path.realpath(p) or abs_path.startswith(os.path.realpath(p) + os.sep)`

- [ ] **Step A5-4: 验证通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a5_sandbox_path_boundary_test -- --nocapture && \
  cargo test sandbox -- --nocapture
```

- [ ] **Step A5-5: Commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  git add src-tauri/src/python/sandbox.rs \
          src-tauri/tests/p0_a5_sandbox_path_boundary_test.rs && \
  git commit -m "fix(sandbox): enforce path boundary checks in safe_open"
```

---

## Task A6：将 build_env_info 改成 async + tokio::process::Command

**Files:**
- Modify: `src-tauri/src/runtime/chat/context_builder.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Add tests: `src-tauri/tests/p0_a6_env_info_async_test.rs`

- [ ] **Step A6-1: 先写失败测试**
  - 编译期测试：`build_env_info(...).await` 成立，证明它已是 async fn
  - 实现检查：源码必须使用 `tokio::process::Command`
  - 运行时测试：非 git 目录下静默跳过 git 仍返回 `[当前环境]`
  - 不做易抖动的“固定 2 秒内完成”时间断言

- [ ] **Step A6-2: 验证测试失败**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a6_env_info_async_test -- --nocapture
```

- [ ] **Step A6-3: 最小实现**
  - `context_builder.rs`：`build_env_info(...) -> async fn`
  - git 子进程改用 `tokio::process::Command::new("git").output().await`
  - `transport/tauri_commands/chat.rs` 的 `get_env_info(...)` 调用处加 `.await`
  - `context_builder.rs` 现有同步单测改为 `#[tokio::test]` 或等价 async 测试

- [ ] **Step A6-4: 验证通过**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test p0_a6_env_info_async_test -- --nocapture && \
  cargo test build_env_info -- --nocapture
```

- [ ] **Step A6-5: Commit**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  git add src-tauri/src/runtime/chat/context_builder.rs \
          src-tauri/src/transport/tauri_commands/chat.rs \
          src-tauri/tests/p0_a6_env_info_async_test.rs && \
  git commit -m "fix(context-builder): make build_env_info async and non-blocking"
```

---

## 最终验证

- [ ] **Rust 测试总回归**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --tests --no-fail-fast 2>&1 | grep -E "FAILED|^error" || true
```

- [ ] **review_ 系列架构约束回归**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast
```

- [ ] **前端关键事件回归**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
  pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts
```

---

## 修复点汇总

| Task | 主要文件 | 核心修复 | 关键风险控制 |
|------|----------|----------|--------------|
| A1 | `runtime/chat/chat_turn_driver.rs` | 保留 assistant `toolCalls` 并在 cancel 时补齐缺失 tool result | 防止进入下一轮时出现残缺 tool trajectory |
| A2 | `runtime/events.rs` / `transport/tauri_event_adapter.rs` / `runtime/chat/chat_turn_driver.rs` | AskRequired 到前端事件桥接 | 本期只通知，不阻塞等待用户点击 |
| A3 | `runtime/run_registry.rs` | poison recovery，避免 `.unwrap()` 连锁 panic | 保持同步 API，避免 async 化 blast radius |
| A4 | `llm/tool_executor/python.rs` | 旧持久 Python 分支禁止退回 conversation-scoped session | 强制主路径 run 隔离 |
| A5 | `python/sandbox.rs` | `_safe_open` 增加路径边界判断 | 防止 `/workspace.backup` 前缀绕过 |
| A6 | `runtime/chat/context_builder.rs` / `transport/tauri_commands/chat.rs` | git 子进程异步化 | 避免阻塞 tokio worker |
