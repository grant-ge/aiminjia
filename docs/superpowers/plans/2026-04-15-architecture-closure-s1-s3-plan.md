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
| `src-tauri/src/runtime/tools/capability.rs` |  |  | ✅ | FileOperations trait + CapabilityContext 扩展 |
| `src-tauri/src/runtime/tools/context.rs` |  |  | ✅ | ExecutionContext 精简（移除原始 state 暴露）|
| `src-tauri/src/runtime/chat/tool_round_types.rs` |  |  | ✅ | RuntimeToolCallOutcome 确认携带 file_meta/degradation |
| `src-tauri/src/runtime/tools/executor.rs` |  |  | ✅ | ToolResult 确认携带 generatedFiles 聚合字段 |
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
      Allow {
          updated_input: Option<Value>,
          reason: PermissionReason,
      },
      Deny {
          message: String,
          reason: PermissionReason,
      },
      Ask {
          message: String,
          suggestions: Vec<PermissionUpdate>,
          reason: PermissionReason,
      },
  }
  ```
- [ ] `Allow.updated_input` 和 `Ask.suggestions` 来自 S0 规范，S1 实施必须保持同构
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
  let decision = self.permission_pipeline.authorize(&definition, &input, &ctx);
  match decision {
      PermissionDecision::Allow { .. } => { /* proceed to execute */ },
      PermissionDecision::Deny { message, .. } => {
          return Err(ToolError::PermissionDenied(message));
      },
      PermissionDecision::Ask { .. } => {
          // Ask 不在 dispatcher 层处理——返回给调用方（TurnDriver）
          // TurnDriver 按 mode 决定：auto-deny / 发 PermissionAsk event / 等待用户响应
          return Ok(ToolDispatchOutcome::AskRequired(decision));
      },
  }
  ```
- [ ] 扩展 `ToolDispatchOutcome` 枚举以携带 Ask 决策：
  ```rust
  pub enum ToolDispatchOutcome {
      Completed { result: ToolResult, event_names: Vec<String> },
      AskRequired(PermissionDecision),  // 三态真正保住——Ask 上浮到 TurnDriver
  }
  ```
- [ ] 在 TurnDriver / QueryEngine 的调用侧处理 `AskRequired`：
  - S1 过渡实现：遇到 `AskRequired` 时记录 log + 转为 PermissionDenied 错误返回
  - 但关键是：**转换发生在 TurnDriver 层，不是 Dispatcher 层**
  - 当 S6 实现 Ask UI 时，TurnDriver 只需在 `AskRequired` 分支改为发 `RuntimeEvent::PermissionAsk` 并等待响应，不需要改 Dispatcher
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
- [ ] 测试 2：ToolDispatcher.dispatch() 收到 Ask 时返回 `Ok(ToolDispatchOutcome::AskRequired(...))`（不是 `Err(PermissionDenied)`）
- [ ] 测试 3：TurnDriver/QueryEngine 收到 `AskRequired` 后暂转 Deny（验证 Ask→Deny 发生在 TurnDriver 层，不在 Dispatcher 层）
- [ ] 测试 4：legacy tool 经 registry.execute() 时也经过统一 pipeline（unknown scope 不被放行）
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

- [ ] 新增 `child_token()` 方法——parent cancel 传播到 child，child cancel 不影响 parent。

  **实现方案**：基于共享 parent 引用 + cancel 时遍历 children，不用线程轮询：

  ```rust
  use std::sync::{Arc, Mutex, Weak};

  #[derive(Clone, Debug)]
  pub struct CancellationToken {
      inner: Arc<TokenInner>,
  }

  #[derive(Debug)]
  struct TokenInner {
      cancelled: AtomicBool,
      children: Mutex<Vec<Weak<TokenInner>>>,
  }

  impl CancellationToken {
      pub fn new() -> Self {
          Self {
              inner: Arc::new(TokenInner {
                  cancelled: AtomicBool::new(false),
                  children: Mutex::new(Vec::new()),
              }),
          }
      }

      pub fn child_token(&self) -> CancellationToken {
          let child = CancellationToken::new();
          // 如果 parent 已 cancelled，child 立即 cancelled
          if self.is_cancelled() {
              child.cancel();
              return child;
          }
          // 注册 child 的 weak ref 到 parent
          self.inner.children.lock().unwrap().push(Arc::downgrade(&child.inner));
          child
      }

      pub fn cancel(&self) {
          if self.inner.cancelled.swap(true, Ordering::SeqCst) {
              return; // 已经 cancelled，避免重复传播
          }
          // 递归传播到所有 children（不要先 store 再调 cancel，否则 swap 短路会跳过 grandchildren）
          self.propagate_to_children();
      }

      fn propagate_to_children(&self) {
          let children = self.inner.children.lock().unwrap();
          for weak_child in children.iter() {
              if let Some(child_inner) = weak_child.upgrade() {
                  // 统一走 cancel()——swap 会标记 cancelled，然后递归传播到 grandchildren
                  let child_token = CancellationToken { inner: child_inner };
                  child_token.cancel();
              }
          }
      }

      pub fn is_cancelled(&self) -> bool {
          self.inner.cancelled.load(Ordering::SeqCst)
      }
  }

  impl Default for CancellationToken {
      fn default() -> Self { Self::new() }
  }
  ```

  **为什么不用线程轮询**：对标 claude-code-best 的 `createChildAbortController`，cancel 传播是事件驱动的（parent abort → 同步传播到 children）。每个 child 起一条 OS 线程做轮询会把 tool-heavy session 变成资源泄漏点。上述方案用 `Weak<TokenInner>` + cancel 时遍历，零额外线程，abandoned children 被 GC。
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
- [ ] `chat_runtime_impl.rs` 中构造 `round_turn` 时，**显式创建 turn-scoped child token**：
  ```rust
  // cancel_token 是 session/agent 级别的 root token
  // turn 需要的是它的 child——turn cancel 不影响 session，session cancel 传播到 turn
  let turn_cancel = cancel_token.child_token();
  let round_turn = TurnState::new(round_mapping, run_id.clone(), String::new())
      .with_cancellation(turn_cancel);
  ```
  注意：**不是 `.clone()`，是 `.child_token()`**——这样才能建立 session → turn → tool_call 三级 cascade。如果用 clone 只是同一个 root token 的引用，不是真正的层级关系。
- [ ] `query_engine.rs` 中构造 `ToolExecutionContext` 时使用 `turn.build_execution_context(tool_call_id, capability)`——内部会自动调用 `self.cancellation.child_token()` 生成 tool-call-scoped token
- [ ] 编译验证

**完成标准**
- cancel token 形成 session → turn → tool_call 三级 cascade

---

## Task 7：消除生产路径 CancellationToken::new() — 接入真实 parent 或标记为未闭合

**文件**
- Modify: `src-tauri/src/plugin/registry.rs`（line 308, 352）

- [ ] `registry.execute()` 新增 `cancel_token: CancellationToken` 参数，调用方必须显式传入：
  ```rust
  pub async fn execute(
      &self,
      name: &str,
      ctx: &PluginContext,
      input: serde_json::Value,
      cancel_token: CancellationToken,  // 新增
  ) -> Result<ToolOutput, ToolError>
  ```
- [ ] 内部两处 `CancellationToken::new()` 改为使用传入的 `cancel_token`
- [ ] 修复所有调用方编译错误：
  - `sub_agent.rs`：需要从 sub-agent run context 传入 cancel token。如果当前无法获取真实 parent token，**保留 `CancellationToken::new()` 并在旁边加编译期标记**：
    ```rust
    // FIXME(S4): sub-agent cancel token 需要从 parent run 派生 child_token()
    // 当前是孤立 root token，cancel cascade 不生效
    let sub_cancel = CancellationToken::new();
    ```
    **不用 `default()` 做 cosmetic pass**——保持 `new()` 让 grep gate 能发现这个未闭合点。
  - `commands/chat.rs`（test support）：同样保留 `CancellationToken::new()` + FIXME 注释
- [ ] 更新 grep gate 的期望值：
  ```
  # CancellationToken::new() 白名单（精确匹配，每处必须有注释）：
  # 1. src-tauri/src/runtime/cancellation.rs — new() 方法定义本身
  # 2. src-tauri/src/runtime/state.rs — TurnState::new() 默认 token（被 with_cancellation 覆盖）
  # 3. src-tauri/src/llm/sub_agent.rs — FIXME(S4): 尚未接入 parent child_token
  # 4. src-tauri/src/commands/chat.rs — test support / non-production helper
  # 其他位置 = 架构回退，必须修复
  ```

**完成标准**
- 生产热路径（chat 主链路的 tool round）已接入真实 parent token
- 未闭合点有 FIXME 标记且在 grep gate 期望列表中，不是假绿

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

- [ ] CancellationToken 支持 child_token() cascade（事件驱动树形传播，零线程）
- [ ] **runtime-native tool 路径**（经 ToolDispatcher → RuntimeTool 的）形成 session → turn → tool_call 三级层级
- [ ] `CancellationToken::new()` 只允许出现在上述白名单位置；每个白名单位置必须有注释说明为什么尚未闭合
- [ ] cascade regression tests 通过（包含 grandchild 传播测试）
- [ ] **已知例外**：`execute_python` 仍走 legacy `ToolPlugin` 路径，S2 不要求它接入 cancel cascade；其 cancel 接入留到 S3/S4 迁移为 RuntimeTool 时解决
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

## Task 9：扩展 CapabilityContext + 重构 LoadFileRuntimeTool

**文件**
- Modify: `src-tauri/src/runtime/tools/capability.rs` — 新增 `FileOperations` trait 和受控 accessor
- Modify: `src-tauri/src/runtime/tools/builtin/file.rs` — LoadFileRuntimeTool 消费 capability.file_ops
- Create: `src-tauri/src/runtime/tools/capability/file_ops.rs` — FileOperations trait 定义 + infra 层实现
- Modify: `src-tauri/src/llm/tool_executor/file_load.rs`（如需拆出 runtime-friendly helper）

- [ ] 定义 `FileOperations` trait（S0 定义的受控 accessor）：
  ```rust
  #[async_trait]
  pub trait FileOperations: Send + Sync {
      async fn load_file(&self, file_id: &str, scope_id: &str) -> Result<LoadedFile>;
      fn is_loaded(&self, file_id: &str, scope_id: &str) -> bool;
      fn workspace_path(&self) -> &Path;
  }
  ```
- [ ] 在 `CapabilityContext` 中新增 `file_ops: Option<Arc<dyn FileOperations>>`
- [ ] 实现 `DefaultFileOperations`（infra 层），封装现有 `handle_load_file` 的核心逻辑：
  - 接收 `AppStorage`、`FileManager`、workspace_path 等依赖
  - 实现 `FileOperations` trait 的三个方法
  - 保持 loaded key 语义（`loaded:{scope_id}:{file_id}`）
  - 保持 parser / masking 行为
- [ ] `LoadFileRuntimeTool::execute()` 从 `ctx.capability.file_ops` 获取 accessor，调用 `file_ops.load_file()`
- [ ] 删除 `build_plugin_ctx()` 桥接函数
- [ ] 编译验证

**关键约束**
- `CapabilityContext` 暴露的是 **trait object accessor**，不是底层 `Arc<AppStorage>`
- 工具不能绕过 `FileOperations` 直接操作文件系统
- 如果需要更多能力，必须新增 trait method 而不是把原始依赖塞回 capability

**完成标准**
- LoadFileRuntimeTool 通过 `capability.file_ops` 访问文件能力
- 不再构造 PluginContext
- FileOperations trait 的实现封装在 infra 层

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

## Task 12：定义 execute_python 后续迁移边界（S3 不实施迁移）

**文件**
- `src-tauri/src/llm/tool_executor/python.rs`

- [ ] 梳理 execute_python 当前依赖 PluginContext 的最小字段集
- [ ] 产出明确结论：
  - 哪些字段可在 S4 迁成 ExecutionContext + CapabilityContext
  - 哪些仍需暂时保留（如 session_manager 需要特殊处理）
- [ ] 将结论写入 `docs/2026-04-15-execute-python-migration-boundary.md`

**完成标准**
- execute_python 在 S3 只完成 dependency inventory 和 boundary definition，不要求 runtime-native migration
- S3 的 canonical migration target 仅为 load_file + precompute auto-load

---

## S3 验收

- [ ] LoadFileRuntimeTool 不再回桥到 PluginContext
- [ ] precompute auto-load 不再构造 full PluginContext
- [ ] **运行时语义 gate**：
  - loaded/load_failed key 语义保持
  - file_meta/generatedFiles/degradation 透传到 TurnDriver（注意：需确认 `RuntimeToolCallOutcome` 和 `ToolResult` 已携带 `file_meta`、`is_degraded`、`degradation_notice` 字段——如果当前结果模型缺失 `generated_files` 聚合，需在 Task 9 中一并补齐，涉及 `src-tauri/src/runtime/chat/tool_round_types.rs` 和 `src-tauri/src/runtime/tools/executor.rs`）
  - cancellation 来自 turn cascade
  - 经过统一 permission pipeline
- [ ] execute_python 只完成迁移边界定义（dependency inventory + boundary doc），不计作 runtime-native 迁移完成
- [ ] S3 的 canonical migration target 仅为 load_file + precompute auto-load
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
