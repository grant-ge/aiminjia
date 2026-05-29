# 2026-04-15 计划维度实现复审（第四轮）

状态：**⚠️ 本轮新增的 P4-A / 权限持久化修复已验证；但按计划关闭口径，仍建议保留 1 个 P1 级未闭合项**

评审对象：

- `docs/superpowers/plans/2026-04-14-p0-immediate-fixes-plan.md`
- `docs/superpowers/plans/2026-04-14-p1-chat-runtime-first-final-closure-plan.md`
- `docs/superpowers/plans/2026-04-13-chat-runtime-first-closure-plan.md`
- `docs/superpowers/plans/2026-04-14-chat-runtime-closure-red-lights.md`
- `docs/superpowers/plans/2026-04-14-p1-a-chat-tool-dispatch-runtime-plan.md`
- `docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md`
- `docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md`
- `docs/superpowers/plans/2026-04-14-prompt-slimming-plan.md`
- `docs/superpowers/plans/2026-04-14-p4a-plugin-context-cancellation-plan.md`
- `docs/superpowers/plans/2026-04-14-p4b-storage-facade-plan.md`
- `docs/superpowers/plans/2026-04-14-p4c-policy-engine-python-sandbox-plan.md`

评审分支：`pzc`

新增复核基线：

- `e71db2c feat(runtime): extend RuntimeTurnExecutor with run_llm_step, rewrite driver as iterative loop`
- `3aa0df8 test(P1-A): T1-T3 green — MockLlmExecutor routes tool calls through runtime dispatcher`
- 未提交修复：Python LRU eviction cancel token 真实级联、`PermissionStore` 原子写入

---

## 本轮结论先行

用户之前列出的 5 个 findings，目前复核结论如下：

- **F1 已修复**：README 与 closure review 状态已回调，不再沿用先前的提前关闭口径。
- **F2 已修复**：`review_` 测试命名已生效。2026-04-15 本地执行 `cargo test --manifest-path src-tauri/Cargo.toml review_ --tests --no-fail-fast` 时，当前 checkout 实际匹配到 **26** 条 `review_` 测试，全部通过。
- **F3 已修复**：`StorePolicyPipeline` 已接入 `ToolRegistry` 生产路径，`unknown scope` 仍保持 fail-closed。
- **F4 按本轮修复目标可关闭**：`chat_runtime_impl.rs` 主链路与 precompute 路径都已经接入真实 `CancellationToken`，Python LRU eviction 不再固定走 `None`；但仍保留 1 个后续迁移项：`src-tauri/src/llm/tool_executor/python.rs` 旧 `PluginContext` 路径仍调用 `execute_for_run()`。
- **F5 已修复**：`file_meta / is_degraded / degradation_notice` 的 runtime 透传链路已补齐。
- **P4-B 残余风险已修复**：`PermissionStore::flush_persistent()` 已改为 temp file + rename 的原子写入，并补了持久化回读测试。

因此，本轮最准确的判断是：

- 文档状态回调、review 测试命名、权限管线接入、file metadata 透传、Python LRU eviction cancel token、权限持久化原子写入，这些项都已落地；
- `cargo check`、`permission_store` 单测、`review_` 回归，以及 `send_message_production_path_test` 当前均为绿色；
- **但如果严格按 P1/P1-A 的原始关闭条件理解为“runtime 真正拿回 send_message 的 LLM/tool round ownership，而不是 legacy transport 内部继续拥有整轮执行权”，则仍建议保留 1 个 P1 级 open finding。**

---

## Findings

### Finding 1（P1，仍 open）

**P1 / P1-A 的 runtime-first ownership 仍未按最严格口径闭合：`RuntimeChatTurnDriver` 的 iterative contract 已可工作，但真实 Tauri executor 仍然把整轮 turn 委托给 `legacy_send_message_impl(...)`。**

#### 代码证据

- `src-tauri/src/runtime/chat/chat_turn_driver.rs`
  - `RuntimeTurnExecutor` 已新增 `run_llm_step()` / `feed_tool_results()`；
  - executor-backed 分支也已经改成 iterative loop，driver 可以在 runtime 边界内统一调度 tool round。
- 但 `src-tauri/src/transport/tauri_commands/chat.rs`
  - `TauriLegacyTurnExecutor` 目前仍只实现 `run_chat_turn()`；
  - 其实现仍直接调用 `legacy_send_message_impl(...)`；
  - 并没有让真实 Tauri executor 改为返回 tool calls，再由 `RuntimeChatTurnDriver` 继续驱动后续轮次。
- 这意味着：
  - runtime contract 已经具备；
  - 但真实 production adapter 是否已经完成“owner 从 legacy transport 迁回 runtime”的最后一跳，当前代码仍没有直接证明。

#### 测试证据

本轮新增/复测结果：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test review_chat_tool_dispatch_runtime_test -- --nocapture
```

- 6/6 绿

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test send_message_production_path_test -- --nocapture
```

- 4/4 绿

```bash
cargo test --manifest-path src-tauri/Cargo.toml review_ --tests --no-fail-fast
```

- 当前 checkout 下匹配到 26 条 `review_` 测试，全部通过

这些结果证明了三件事：

1. `SessionRuntime + RuntimeChatTurnDriver` 的 iterative contract 现在确实能跑通；
2. executor-backed 路径现在能够通过 runtime event bus 对外发出 `message:updated` / `streaming:done` / tool events；
3. 先前用来卡 P1/P1-A 的 review 红灯，目前在 contract / gating test 维度已经转绿。

**但这些绿灯仍主要证明“runtime contract 可工作”，而不是直接证明“真实 `TauriChatCommandAdapter -> TauriLegacyTurnExecutor` 已完全不再由 `legacy_send_message_impl` 拥有整轮控制权”。**

#### 计划维度判断

- 对 `2026-04-14-p1-chat-runtime-first-final-closure-plan.md`
  - 现在已经不能再说“红灯未修”或“主链路没有进展”；
  - 但如果关闭条件是“runtime 真正拿回 send_message ownership”，那仍建议维持**未完全关闭**口径。
- 对 `2026-04-14-p1-a-chat-tool-dispatch-runtime-plan.md`
  - runtime tool dispatch contract、event bus、gating tests 基本已到位；
  - 剩余 gap 更偏向**架构关闭口径**，而不是功能红灯。

#### 风险

- 如果此时直接把 P1/P1-A 在计划文档中记为“已关闭”，容易把“contract 已具备”和“生产 owner 已迁移”混为一谈；
- 后续如果还要继续做 runtime-first 收口，文档会丢失这条关键的架构边界信息。

---

## 已关闭的旧 finding

### 已关闭 1：P4-A / Python LRU eviction cancel token

本轮这项可以按“已修复”记账，但建议备注 1 个非阻塞 follow-up。

#### 关闭依据

- `src-tauri/src/python/session.rs`
  - 新增 `execute_with_cancel()` 与 `execute_for_run_with_cancel()`；
  - `get_or_create_with_token()` 现在能接收真实 `CancellationToken`；
  - LRU eviction 的后台任务在 token 已取消时会跳过昂贵的 checkpoint 写盘，但仍执行 `kill()`，避免泄漏进程。
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
  - 为 turn 创建了 `CancellationToken`；
  - 在 early cancellation、timeout、`cancel_rx` 触发、tool batch 后检测取消等路径显式调用 `cancel()`；
  - 通过 `AgentContext.cancel_token` 传进 agent loop；
  - precompute Python 路径已改用 `execute_for_run_with_cancel(..., Some(cancel_token.clone()))`。

#### 当前口径

- **主链路已闭合**：本轮 review 指向的“evicted session 在 turn 已取消后仍无意义写盘”的问题，当前在 chat runtime 主链路上已经修掉；
- **保留 1 个 follow-up**：`src-tauri/src/llm/tool_executor/python.rs` 仍走旧 `PluginContext.session_manager.execute_for_run()`，没有把 token 继续传下去；这更像后续 migration 项，而不是本轮 blocker。

### 已关闭 2：P4-B / 权限持久化原子写入

这项残余风险本轮可以关闭。

#### 关闭依据

- `src-tauri/src/runtime/store/permission_store.rs`
  - `flush_persistent()` 已从直接 `std::fs::write()` 改为：
    - 先写 `permissions.json.tmp`
    - 再 `std::fs::rename()` 覆盖正式文件
    - rename 失败时清理孤儿 tmp 文件
- 新增测试：
  - `test_flush_persistent_atomic_write`
  - `test_flush_persistent_reload`

#### 风险变化

- 先前“进程崩溃可能把权限 JSON 写坏”的风险，现在已经明显下降；
- 在同文件系统前提下，当前实现满足这轮 review 对持久化原子性的要求。

---

## 按计划维度复核

| 计划 | 本轮判断 | 说明 |
|---|---|---|
| P0 immediate fixes | 基本符合 | 本轮未见新回退 |
| P1 final closure | **仍不建议直接关闭** | 功能红灯已大体转绿，但 ownership 口径仍建议保留谨慎判断 |
| 2026-04-13 chat runtime first closure | **接近闭合** | runtime contract、event bus、send_message gating tests 均已转绿 |
| chat runtime closure red lights | 已基本转绿 | review / production-path 红灯测试当前均为绿色 |
| P1-A tool dispatch runtime | **大部分完成** | dispatch、event bus、回归测试都已到位；剩余是架构关闭口径 |
| Workspace-First | 符合 | 本轮未见回退 |
| Atomic Tool | 符合 | 本轮未见回退 |
| Prompt Slimming | 符合 | 本轮未见回退 |
| P4-A PluginContext / Cancellation | **按本轮范围已完成** | chat runtime 主链路已接真实 token；旧 `PluginContext` Python 路径留作 follow-up |
| P4-B Storage Facade | **已补齐持久化原子写盘** | 原 residual risk 可关闭 |
| P4-C Policy Engine / Python sandbox | 基本符合当前关闭口径 | `StorePolicyPipeline` 已在生产 dispatcher 生效 |

---

## 本轮验证记录

### 通过

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- 编译通过，当前仅有 warnings，未见 errors

```bash
cargo test --manifest-path src-tauri/Cargo.toml permission_store -- --nocapture
```

- 5 passed
- 包含：`test_flush_persistent_atomic_write`、`test_flush_persistent_reload`

```bash
cargo test --manifest-path src-tauri/Cargo.toml review_ --tests --no-fail-fast
```

- 2026-04-15 本地实测：当前 checkout 下匹配到 26 条 `review_` 测试，全部通过

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test send_message_production_path_test -- --nocapture
```

- 4 passed

### 代码复核确认

- `src-tauri/src/python/session.rs` 已把 cancel token 真正穿到 LRU eviction 判断点；
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` 已在 turn 生命周期中真实调用 `cancel()`，并把 token 传入 precompute Python 路径；
- `src-tauri/src/runtime/store/permission_store.rs` 已改为 temp file + rename 的原子写入；
- `src-tauri/src/transport/tauri_commands/chat.rs` 中 `TauriLegacyTurnExecutor` 仍维持 `run_chat_turn() -> legacy_send_message_impl(...)` 的 ownership 形态。

---

## 测试缺口与残余风险

这些目前不足以上升为新的 blocking finding，但仍建议记录：

1. **P1/P1-A 仍缺对“真实 ownership 迁移完成”的直接证明**
   - 现在可以证明 runtime contract 工作正常；
   - 但还缺一条能直接证明 `TauriLegacyTurnExecutor` 不再把整轮 turn 留在 `legacy_send_message_impl(...)` 内部的测试或代码收口。

2. **旧 `PluginContext` Python 路径仍未透传 cancel token**
   - `src-tauri/src/llm/tool_executor/python.rs` 仍调用 `execute_for_run()`；
   - 当前被标注为后续 migration 项，暂不构成本轮 blocker。

3. **`file_meta` 仍缺真正的端到端回归**
   - 当前已补齐 runtime 结构体与收集逻辑；
   - 但还缺一条从真实文件型 tool 到 assistant message / generated files 展示层的 e2e 验证。

---

## 最终判断

截至 **2026-04-15** 这轮复核：

- 本轮新增的 **P4-A Python LRU eviction cancel token** 与 **权限持久化原子写入** 已经落到代码与测试；
- `cargo check`、`permission_store` 单测、`review_` 回归、`send_message_production_path_test` 当前均为绿色；
- **F1 / F2 / F3 / F4 / F5 均可按“已处理”记账**；
- 但若严格按计划原意追问“runtime 是否已经真正拿回 send_message 的 ownership”，则**仍建议保留 1 个 P1 级 open finding，不直接把 P1/P1-A 标成 fully closed。**
