# 2026-04-15 架构闭环 S1-S3 实施计划（v3 — rebased to S0）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 S0 目标架构定义，分三期让权限模型、取消模型、工具上下文向 canonical runtime path 靠拢。每期可独立编译、测试、合并。

**Architecture:** S1 把权限契约升级为三态（Allow/Deny/Ask）并统一入口；S2 把 CancellationToken 改为 child_token cascade；S3 把高价值工具迁到 per-call ExecutionContext。每一步都是缩小现实与 S0 目标的差距，不是在修补 side path。

**Tech Stack:** Rust, Tauri v2, tokio, async_trait

**Design Spec:**
- S0 目标架构：`docs/superpowers/specs/2026-04-15-s0-target-runtime-architecture.md`
- 分期设计：`docs/superpowers/specs/2026-04-15-architecture-closure-phased-design.md`

---

## 总体原则

1. **S0 驱动**：每个 Task 的目标是"让某个维度向 S0 定义的 canonical model 靠拢"
2. **每期可独立合并**：可单独编译、有明确测试、不依赖下一期
3. **验收 = 运行时语义 gate**：不只看代码形状，要验证 runtime 行为不回退
4. **不假装关闭 P1**：S1-S3 不关闭完整 streaming ownership

---

## 文件范围

| 文件 | S1 | S2 | S3 | 说明 |
|---|---|---|---|---|
| `src-tauri/src/runtime/tools/permission.rs` | ✅ |  |  | PermissionDecision 三态 + PermissionPipeline trait 升级 |
| `src-tauri/src/runtime/store/permission_store.rs` | ✅ |  |  | StorePolicyPipeline.authorize() 返回三态 |
| `src-tauri/src/runtime/tools/dispatcher.rs` | ✅ |  |  | dispatch() 处理三态 + allow_all 收缩 |
| `src-tauri/src/plugin/registry.rs` | ✅ |  |  | execute() legacy 回退走统一 pipeline |
| `src-tauri/src/runtime/cancellation.rs` |  | ✅ |  | child_token() cascade |
| `src-tauri/src/runtime/state.rs` |  | ✅ |  | TurnState.with_cancellation() + build_execution_context() |
| `src-tauri/src/runtime/query_engine.rs` |  | ✅ |  | 使用 turn.cancellation.child_token() |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` |  | ✅ | ✅ | round_turn 注入 session cancel child; precompute 去桥 |
| `src-tauri/src/runtime/tools/builtin/file.rs` |  |  | ✅ | load_file 迁到 ExecutionContext |
| `src-tauri/src/runtime/tools/context.rs` |  |  | ✅ | ExecutionContext 精简（移除原始 state 暴露）|
| `src-tauri/tests/` | ✅ | ✅ | ✅ | 各期 regression tests |

---

# S1：权限模型升级到三态 + 单一入口

## 目标

把后端权限契约从二态（allow/deny + `Result<()>`）升级为三态（Allow/Deny/Ask + `PermissionDecision`），从第一天起预留 Ask 路径。同时消除 `allow_all()` bypass，统一所有入口到同一个 pipeline。

## 非目标

- 不在 S1 实现前端 Ask UI（Ask 暂时转为 Deny）
- 不在 S1 改动 LLM streaming ownership

---

## Task 1：定义三态 PermissionDecision 类型

**文件**
- Modify: `src-tauri/src/runtime/tools/permission.rs`

- [ ] 新增 `PermissionDecision` 枚举（S0 定义的三态）：
  ```rust
  pub enum PermissionDecision {
      Allow { reason: PermissionReason },
      Deny { message: String, reason: PermissionReason },
      Ask { message: String, reason: PermissionReason },
  }
  ```
- [ ] 新增 `PermissionReason` 枚举：
  ```rust
  pub enum PermissionReason {
      StoredPolicy,
      Capability,
      UnknownScope,
      Mode(String),
      Other(String),
  }
  ```
- [ ] 修改 `PermissionPipeline` trait 的 `authorize()` 返回类型从 `Result<()>` 改为 `PermissionDecision`
- [ ] 更新 `AllowAllPermissionPipeline`：返回 `PermissionDecision::Allow`
- [ ] 更新 `CapabilityPermissionPipeline`：已知 scope 返回 Allow/Deny，unknown scope 返回 Deny（保持 fail-closed）
- [ ] 更新 `StorePolicyPipeline`：已知 stored allow → Allow，stored deny → Deny，未存储 + unknown scope → Ask（而非直接 Deny）
- [ ] 编译验证

**完成标准**
- `PermissionPipeline::authorize()` 返回三态 `PermissionDecision`
- 所有现有 pipeline 实现已适配

---

## Task 2：ToolDispatcher.dispatch() 处理三态

**文件**
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`

- [ ] `dispatch()` 中的权限检查从 `pipeline.authorize().map_err(...)` 改为 match `PermissionDecision`：
  ```rust
  match self.permission_pipeline.authorize(&definition, &input, &ctx) {
      PermissionDecision::Allow { .. } => { /* proceed */ },
      PermissionDecision::Deny { message, .. } => {
          return Err(ToolError::PermissionDenied(message));
      },
      PermissionDecision::Ask { message, .. } => {
          // S1 过渡：Ask 转为 Deny（后端契约已就位，Ask UI 在 S6 实现）
          // 当 Ask UI 就位后，此处改为发送 RuntimeEvent::PermissionAsk 并等待响应
          return Err(ToolError::PermissionDenied(
              format!("Permission requires user confirmation (ask): {}", message)
          ));
      },
  }
  ```
- [ ] 将 `allow_all()` 标记为 `#[cfg(test)]`
- [ ] 编译验证

**完成标准**
- dispatch() 能区分 Allow/Deny/Ask 三种情况
- Ask 暂转 Deny 但有明确标记（不是静默 deny）
- 生产代码中 `allow_all()` 不可用

---

## Task 3：统一 registry.execute() 的权限入口

**文件**
- Modify: `src-tauri/src/plugin/registry.rs`

- [ ] `execute()` 中 legacy fallback（line 340）从 `ToolDispatcher::allow_all()` 改为与 runtime path 同级别的 pipeline（复用已有 `permission_store` 模式）
- [ ] 确保 runtime path（line 308-320）和 legacy fallback path 使用同一个 `PermissionPipeline` trait 返回三态
- [ ] 编译验证

**完成标准**
- `execute()` 中不再存在 `allow_all()` bypass
- runtime path 和 legacy path 经过同一个 pipeline

---

## Task 4：S1 regression tests

**文件**
- Create: `src-tauri/tests/permission_three_state_test.rs`

- [ ] 测试 1：StorePolicyPipeline 对未存储 + unknown scope 返回 `Ask`（不是直接 Deny）
- [ ] 测试 2：ToolDispatcher.dispatch() 收到 Ask 时返回 PermissionDenied 且消息包含 "ask"
- [ ] 测试 3：legacy tool 经 registry.execute() 时也经过统一 pipeline（unknown scope 不被放行）
- [ ] 全量回归：`cargo test review_ --tests --no-fail-fast`

**完成标准**
- 三条测试直接证明权限契约是三态的

---

## S1 验收

- [ ] `PermissionPipeline::authorize()` 返回 `PermissionDecision`（三态），不是 `Result<()>`
- [ ] `rg "allow_all\\(" src-tauri/src` 的生产路径调用归零
- [ ] StorePolicyPipeline 对 unknown scope 返回 Ask（不是 Deny）
- [ ] legacy fallback 与 runtime path 使用同一个 pipeline
- [ ] `cargo test` 全绿

---

# S2：Cancel model 改为 child_token cascade

## 目标

把 CancellationToken 从"各处自建"改为"层级 cascade"：session root → turn child → tool_call child。对标 claude-code-best 的 `createChildAbortController` 模式。

## 非目标

- 不在 S2 改动 PluginContext（不加字段、不透传）
- 不在 S2 要求所有 legacy tool 都支持 cancel

---

## Task 5：CancellationToken 新增 child_token()

**文件**
- Modify: `src-tauri/src/runtime/cancellation.rs`

- [ ] 新增 `child_token()` 方法——parent cancel 传播到 child，child cancel 不影响 parent：
  ```rust
  impl CancellationToken {
      pub fn child_token(&self) -> CancellationToken {
          let child = CancellationToken::new();
          if self.is_cancelled() {
              child.cancel();
              return child;
          }
          let parent_cancelled = self.cancelled.clone();
          let child_cancelled = child.cancelled.clone();
          std::thread::spawn(move || {
              // 轮询 parent 状态（简单实现；后续可改为 notify）
              while !parent_cancelled.load(Ordering::SeqCst) {
                  std::thread::sleep(std::time::Duration::from_millis(50));
              }
              child_cancelled.store(true, Ordering::SeqCst);
          });
          child
      }
  }
  ```
  注意：这是一个最小可用实现。后续可以用 `tokio::sync::watch` 或 `Arc<Notify>` 替代轮询。在 S2 范围内，正确性优先于效率。
- [ ] 写单元测试：parent cancel 传播到 child
- [ ] 写单元测试：child cancel 不传播到 parent
- [ ] 写单元测试：parent 已 cancelled 时 child_token() 立即 cancelled
- [ ] 编译验证

**完成标准**
- `child_token()` cascade 语义正确

---

## Task 6：TurnState 使用 session cancel 的 child_token

**文件**
- Modify: `src-tauri/src/runtime/state.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

- [ ] `TurnState` 新增 `with_cancellation(token)` builder（如果尚未存在）
- [ ] `TurnState` 新增 `build_execution_context()` 方法（S0 定义），内部使用 `self.cancellation.child_token()` 为每次 tool call 创建 call-scoped token
- [ ] `chat_runtime_impl.rs` 中构造 `round_turn` 时使用 `.with_cancellation(cancel_token.clone())`——将 AgentContext 的 cancel_token 注入 TurnState
- [ ] `query_engine.rs` 中构造 `ToolExecutionContext` 时使用 `turn.cancellation().child_token()` 替代 `turn.cancellation()`（tool-call-scoped 而非 turn-scoped）
- [ ] 编译验证

**完成标准**
- cancel token 形成 session → turn → tool_call 三级 cascade

---

## Task 7：禁止生产路径 CancellationToken::new()

**文件**
- Modify: `src-tauri/src/plugin/registry.rs`（line 308, 352 的 `new()` 改为接收外部 token 或使用 default）

- [ ] `registry.execute()` 的两处 `CancellationToken::new()` 改为 `CancellationToken::default()`（语义不变但代码可 grep 区分）
  - 或者更好：给 execute() 加 `cancel_token: CancellationToken` 参数，调用方传入 turn-scoped token
  - 但 S2 不碰 PluginContext，所以如果调用方无法提供真实 token，用 default 并添加 `// TODO(S3/S4): wire from turn state once PluginContext is removed`
- [ ] grep 验证：`CancellationToken::new()` 在生产路径中只剩 `cancellation.rs` 定义本身和 `TurnState::new()` 的默认值

**完成标准**
- 生产热路径不再随手 `CancellationToken::new()`

---

## Task 8：S2 regression tests

**文件**
- Create: `src-tauri/tests/cancel_cascade_test.rs`

- [ ] 测试 1：TurnState.with_cancellation(parent) 后，parent.cancel() 导致 turn.cancellation().is_cancelled()
- [ ] 测试 2：TurnState.build_execution_context() 返回的 ctx.cancellation 是 turn cancellation 的 child（parent cancel 传播到 ctx，ctx cancel 不传播到 parent）
- [ ] 测试 3：RuntimeTool 经 ToolDispatcher 调用时，收到的 ctx.cancellation 是 turn 的 child_token
- [ ] 全量回归

**完成标准**
- cascade 传播链有直接测试覆盖

---

## S2 验收

- [ ] CancellationToken 支持 child_token() cascade
- [ ] cancel 形成 session → turn → tool_call 三级层级
- [ ] `CancellationToken::new()` 在生产热路径归零（合法位置除外）
- [ ] cascade regression tests 通过
- [ ] `cargo test` 全绿

---

# S3：高价值工具迁到 ExecutionContext

## 目标

把 `load_file` 和 precompute auto-load 从 PluginContext 回桥改为直接消费 per-call ExecutionContext。验收不仅看代码形状，还验运行时语义。

## 非目标

- 不在 S3 回收完整 streaming ownership
- 不在 S3 一次性迁完所有 legacy tools
- 不在 S3 解决 synthetic message_persisted

---

## Task 9：重构 LoadFileRuntimeTool 使用 ExecutionContext

**文件**
- Modify: `src-tauri/src/runtime/tools/builtin/file.rs`
- Modify: `src-tauri/src/llm/tool_executor/file_load.rs`（如需拆出 runtime-friendly helper）

- [ ] 把 `handle_load_file(ctx: &PluginContext, ...)` 的核心逻辑拆为 runtime-friendly helper，接收明确的 deps：
  - `workspace_path: &Path`
  - `conversation_id: &str`
  - `run_id: Option<&RunId>`
  - `storage: &AppStorage`（或更窄的 trait）
  - `file_manager: &FileManager`
- [ ] `LoadFileRuntimeTool::execute()` 从 `ExecutionContext.capability` 获取所需 deps，直接调用 helper
- [ ] 删除 `build_plugin_ctx()` 桥接函数
- [ ] 编译验证

**完成标准**
- LoadFileRuntimeTool 不再构造 PluginContext

---

## Task 10：precompute auto-load 去桥

**文件**
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

- [ ] precompute auto-load 阶段（约 line 1812）改为使用 Task 9 拆出的 runtime-friendly helper
- [ ] 不再构造 full PluginContext——只传入所需的窄 deps
- [ ] 编译验证

**完成标准**
- precompute auto-load 路径不再依赖 full PluginContext

---

## Task 11：S3 运行时语义 gate

**文件**
- Create or extend: `src-tauri/tests/load_file_runtime_semantics_test.rs`

验收不仅看"是否去掉了 PluginContext 构造"，还要锁住运行时语义不回退：

- [ ] 测试 1：load_file 经 runtime path 调用后，`loaded:` key 仍然按原语义设置（`loaded:{scope_id}:{file_id}`）
- [ ] 测试 2：load_file 返回的 ToolResult 的 file_meta 仍然被 TurnDriver 正确收集（`all_file_metas` 不为空）
- [ ] 测试 3：load_file 的 cancellation token 来自 turn cascade（parent cancel 时 load_file 可观察到 cancelled）
- [ ] 测试 4：load_file 经过统一 permission pipeline（不是 allow_all）
- [ ] 全量回归

**完成标准**
- runtime 语义 gate 通过，不是假绿

---

## Task 12：评估 execute_python 迁移边界

**文件**
- `src-tauri/src/llm/tool_executor/python.rs`

- [ ] 梳理 execute_python 当前依赖 PluginContext 的最小字段集
- [ ] 产出明确结论：
  - 哪些字段可在 S4 迁成 ExecutionContext + CapabilityContext
  - 哪些仍需暂时保留（如 session_manager 需要特殊处理）
- [ ] 将结论写入 `docs/2026-04-15-execute-python-migration-boundary.md`

**完成标准**
- execute_python 的迁移边界被显式定义

---

## S3 验收

- [ ] LoadFileRuntimeTool 不再回桥到 PluginContext
- [ ] precompute auto-load 不再构造 full PluginContext
- [ ] **运行时语义 gate**：
  - loaded/load_failed key 语义保持
  - file_meta/generatedFiles 透传到 TurnDriver
  - cancellation 来自 turn cascade
  - 经过统一 permission pipeline
- [ ] execute_python 迁移边界已明确
- [ ] `cargo test` 全绿

---

## 最终文档回写

S1-S3 任一期完成后，更新：
- `docs/superpowers/plans/README.md`
- `docs/2026-04-15-current-architecture-improvement-needs.md`

注意：S1-S3 完成后**不关闭 P1**。只有完整 ownership 回收（S4）做完才讨论。

---

## 一句话执行顺序

1. **S1**：三态权限契约 + bypass 消除 + 单一入口
2. **S2**：child_token cascade + 禁止 new()
3. **S3**：load_file / precompute 迁到 ExecutionContext + 运行时语义 gate
