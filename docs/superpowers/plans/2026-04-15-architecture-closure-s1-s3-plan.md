# 2026-04-15 架构闭环 S1-S3 实施计划

> 设计文档：`docs/superpowers/specs/2026-04-15-architecture-closure-phased-design.md`

Goal：按窄切面分三期推进架构闭环：

- `S1` 权限边界统一
- `S2` 取消传播统一
- `S3` 高价值 legacy bridge 收缩

本计划刻意**不**把完整 LLM streaming ownership 回收塞进来；那是后续 `S4` / Tasks 3/4 级别的大重构。

---

## 总体原则

1. **每期都能独立合并**
   - 可单独编译
   - 有明确测试
   - 不依赖“下一期做完才成立”

2. **先收边界，再收 ownership**
   - S1 先把权限边界统一
   - S2 再把取消传播断点补齐
   - S3 再缩减 hot path legacy bridge

3. **不假装关闭 P1**
   - 即使 S1-S3 全做完，也仍然**不直接关闭**“完整 streaming ownership”那条 P1 open finding

---

## 文件范围

| 文件 | S1 | S2 | S3 | 说明 |
|---|---|---|---|---|
| `src-tauri/src/plugin/registry.rs` | ✅ | ✅ |  | 统一权限边界、透传 cancel token |
| `src-tauri/src/plugin/context.rs` |  | ✅ |  | 过渡性新增 `cancel_token` |
| `src-tauri/src/commands/chat.rs` | 观察 |  |  | 复核非 chat 入口是否还存在额外 bypass |
| `src-tauri/src/llm/tool_executor/python.rs` |  | ✅ | 部分 | 改为 cancel-aware 路径；后续继续去桥 |
| `src-tauri/src/llm/sub_agent.rs` |  | ✅ |  | sub-agent 透传 cancel token |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` |  | ✅ | ✅ | hot path 传 token、减少 precompute bridge |
| `src-tauri/src/runtime/tools/dispatcher.rs` | ✅ |  |  | `allow_all()` 约束为 test-only |
| `src-tauri/src/runtime/tools/builtin/file.rs` |  |  | ✅ | 移除 `load_file` 的 PluginContext 回桥 |
| `src-tauri/src/llm/tool_executor/file_load.rs` |  |  | ✅ | 提取 runtime-friendly helper |
| `src-tauri/tests/` | ✅ | ✅ | ✅ | 各期补 regression tests |

---

# S1：权限边界统一

## 目标

让当前仍在使用的所有工具入口都遵守同一条 permission boundary，消除 legacy fallback / non-chat 路径上的 bypass。

## 非目标

- 不在 S1 引入 ask UI
- 不在 S1 调整 `PolicyDecision` 类型
- 不在 S1 重构完整工具系统

---

## Task 1：统一 `ToolRegistry.execute()` 的 legacy fallback 权限裁决

**文件**

- `src-tauri/src/plugin/registry.rs`

- [ ] 把 `execute()` 中 legacy fallback 路径的 `ToolDispatcher::allow_all()` 替换为与 runtime path 同级别的 permission pipeline
- [ ] 优先复用当前文件中已存在的模式：
  - 有 `permission_store` 时用 `StorePolicyPipeline`
  - 没有 `permission_store` 时退回 `CapabilityPermissionPipeline`
- [ ] 确保 runtime path 与 legacy fallback path 在 unknown-scope 行为上保持一致（deny by default）
- [ ] 确认 fallback 路径的报错语义仍会被归一化成当前调用方可接受的 `ToolError`

**完成标准**

- `execute()` 不再存在 `allow_all()` bypass

---

## Task 2：把 `allow_all()` 收缩为 test-only helper

**文件**

- `src-tauri/src/runtime/tools/dispatcher.rs`
- `src-tauri/src/runtime/tools/testing.rs`
- 相关 tests

- [ ] 将 `ToolDispatcher::allow_all()` 明确约束为测试辅助能力
- [ ] 若生产代码仍依赖它，先补替代路径，再收缩
- [ ] 保证测试辅助代码（如 `runtime/tools/testing.rs`）仍可正常构造 allow-all dispatcher

**完成标准**

- 生产代码中不再依赖 `allow_all()`
- allow-all 仅作为测试辅助存在

---

## Task 3：补一条“非 chat / legacy fallback 权限一致性”回归测试

**建议测试目标**

- 一个 legacy tool 通过 `ToolRegistry.execute()` 调用时：
  - 若声明 unknown scope，应 fail-closed
  - 若 permission store 中已有 persisted allow/deny，应遵守 persisted decision

**优先实现方式**

- 优先写在 `src-tauri/tests/` 的 integration test
- 如果构造完整 `PluginContext` 成本过高，可在 `plugin/registry.rs` 写模块内单元测试

**完成标准**

- 能直接证明 `ToolRegistry.execute()` 不再绕开统一权限边界

---

## S1 验收

- [ ] `rg "allow_all\\(" src-tauri/src` 的生产路径调用归零
- [ ] legacy fallback 的 unknown-scope regression test 通过
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml review_ --tests --no-fail-fast` 通过

---

# S2：取消传播统一

## 目标

在当前 ownership 结构不大改的前提下，把 legacy / non-chat / Python 热路径的 cancel 传播断点补齐，让下游消费同一份 turn-scoped token。

## 非目标

- 不要求 S2 直接把 `TurnState` 变成系统唯一 cancel root
- 不要求 S2 完整移除 `PluginContext`
- 不把 background / child-run / sub-agent 全部 runtime-native 化

---

## Task 4：在 `PluginContext` 上增加过渡性 `cancel_token`

**文件**

- `src-tauri/src/plugin/context.rs`

- [ ] 在 `PluginContext` 中新增：
  - `cancel_token: Option<CancellationToken>`
- [ ] 在注释里明确说明：
  - 这是 legacy bridge 的**过渡字段**
  - 目的仅是把 cancel 传到底层旧路径
  - 不是鼓励继续扩张 `PluginContext`
- [ ] 补齐所有 `PluginContext` 构造点的默认值
  - 大多数填 `None`
  - 只有当前真正持有 turn token 的 hot path 填 `Some(...)`

**完成标准**

- 所有 `PluginContext` 构造点可编译

---

## Task 5：chat hot path / sub-agent 把真实 token 填进 `PluginContext`

**文件**

- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- `src-tauri/src/llm/sub_agent.rs`

- [ ] 在 precompute auto-load 路径构造 `PluginContext` 时填入当前 turn token
- [ ] 在 tool round 构造 `PluginContext` 时填入当前 turn token
- [ ] 在 sub-agent 派生的 `PluginContext` 上 clone 父 token
- [ ] 保持没有上游 token 的路径仍可安全为 `None`

**完成标准**

- chat / sub-agent 热路径不再默默丢失 cancel token

---

## Task 6：`ToolRegistry.execute()` 与 Python 旧路径透传 token

**文件**

- `src-tauri/src/plugin/registry.rs`
- `src-tauri/src/llm/tool_executor/python.rs`

- [ ] `registry.execute()` 中 runtime path 构造 `ToolExecutionContext` 时，不再 `CancellationToken::new()`
- [ ] `registry.execute()` 中 legacy fallback 构造 `ToolExecutionContext` 时，也消费 `ctx.cancel_token`
- [ ] `python.rs` 的 run-scoped 执行切换到 `execute_for_run_with_cancel(...)`
- [ ] 若仍有非 run-scoped 路径暂时没有 cancel-aware 变体，明确写 TODO 注释，不要静默制造新的断点

**完成标准**

- registry / python 旧路径不再自己造 ad-hoc token

---

## Task 7：补一条 cancel propagation regression test

**建议测试目标**

- 给 `ToolRegistry.execute()` 一个带 `cancelled` token 的 `PluginContext`
- 由一个 spy `RuntimeTool` 或 legacy-adapted tool 断言：
  - `ToolExecutionContext.cancellation.is_cancelled()` 为 true

**实现建议**

- 优先做一个最小 spy `RuntimeTool`
- 不依赖复杂 storage/file manager helper；dummy 依赖只保证能构造上下文即可

**完成标准**

- 有一条直接证明“token 从 PluginContext 传到 ToolExecutionContext”的测试

---

## S2 验收

- [ ] 生产热路径里不再出现新的 ad-hoc `CancellationToken::new()`
- [ ] registry / python / sub-agent 至少已经消费上传下来的 token
- [ ] cancel propagation regression test 通过
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` 通过

---

# S3：高价值 legacy bridge 收缩

## 目标

优先移除当前最热、最影响 runtime-first 纯度的 `PluginContext` 回桥点，为后续 ownership 大迁移减负。

## 非目标

- 不在 S3 里回收完整 LLM streaming ownership
- 不要求一次性迁完所有 legacy tools
- 不在 S3 里解决 synthetic `message_persisted`

---

## Task 8：重构 `load_file`，移除 runtime -> PluginContext 回桥

**文件**

- `src-tauri/src/runtime/tools/builtin/file.rs`
- `src-tauri/src/llm/tool_executor/file_load.rs`

- [ ] 把 `handle_load_file(ctx: &PluginContext, ...)` 依赖拆成 runtime-friendly helper
- [ ] 目标是让 `LoadFileRuntimeTool` 直接消费 `LoadFileDeps + ToolExecutionContext`
- [ ] 删除 `build_plugin_ctx()` 这类仅为过渡桥接存在的 helper
- [ ] 保持 `load_file` 的现有行为不回退：
  - loaded cache
  - uploaded file resolve
  - parser / masking
  - memory key 语义

**完成标准**

- `src-tauri/src/runtime/tools/builtin/file.rs` 不再为 `load_file` 构造 `PluginContext`

---

## Task 9：precompute auto-load 改为 runtime-friendly helper，不再构造 full `PluginContext`

**文件**

- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- 如有需要，配套 `src-tauri/src/llm/tool_executor/file_load.rs`

- [ ] 把 precompute auto-load 依赖的“file key / loaded key / load failed key / load file”逻辑抽成更窄的 helper
- [ ] 在 chat hot path 里只传：
  - request-scoped deps
  - conversation/run identity
  - cancel token / workspace context
- [ ] 避免在 precompute 阶段重新构造 full `PluginContext`

**完成标准**

- precompute auto-load 路径不再依赖 full `PluginContext`

---

## Task 10：评估并收口 `execute_python` 的下一步迁移边界

**文件**

- `src-tauri/src/llm/tool_executor/python.rs`
- 如有需要，相关 builtin tool file

- [ ] 先不强求本期把 `execute_python` 全量 runtime-native 化
- [ ] 本期至少做两件事：
  - 让 cancel-aware 执行路径成为默认路径
  - 梳理它还依赖 `PluginContext` 的最小字段集
- [ ] 产出明确结论：
  - 哪些字段可下期迁成 request-scoped deps
  - 哪些仍需暂时保留

**完成标准**

- `execute_python` 后续迁移边界被显式定义，而不是继续隐含在 `PluginContext` 里

---

## Task 11：补一条 load_file / precompute bridge reduction regression test

**建议测试目标**

- 证明 runtime `load_file` 路径已经不再依赖 `PluginContext` bridge
- 或证明 precompute auto-load 在不构造 full `PluginContext` 的情况下仍能工作

**完成标准**

- S3 至少有一条测试能直接证明“高价值 bridge 确实被去掉了”

---

## S3 验收

- [ ] `LoadFileRuntimeTool` 不再回桥到 `PluginContext`
- [ ] precompute auto-load 不再构造 full `PluginContext`
- [ ] `execute_python` 的下一步迁移边界已明确
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml review_ --tests --no-fail-fast` 通过

---

## 最终文档回写

在 S1-S3 任一期完成后，按实际完成情况更新：

- `docs/2026-04-15-current-architecture-improvement-needs.md`
- `docs/reviews/2026-04-15-plan-implementation-review.md`
- `docs/superpowers/plans/README.md`

注意：

- S1 / S2 / S3 完成后，仍然**不要**直接把 P1/P1-A 记为 fully closed
- 只有后续完整 ownership 回收做完，才讨论关闭那条 P1 open

---

## 一句话执行顺序

按顺序执行：

1. **S1 先堵住权限 bypass**
2. **S2 再打通 cancel 传播**
3. **S3 再缩减高价值 legacy bridge**

等这三条线收紧以后，再进入完整 ownership 回收的大阶段。
