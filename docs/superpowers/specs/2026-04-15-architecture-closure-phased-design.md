# 2026-04-15 架构闭环分期设计

## 背景

基于对 `docs/2026-04-15-current-architecture-improvement-needs.md` 五项核心断言（A1-A5）的代码验证，全部确认成立。当前系统主链路功能可用，但 runtime ownership、权限边界、取消边界、legacy bridge、状态真相源五条主线尚未闭合。

本设计采用**窄切面 x 多期**策略（方案 A），每期可独立编译、测试、合并，按风险优先排序。

## 代码验证摘要

| 断言 | 结论 | 关键证据 |
|------|------|---------|
| A1: LLM streaming ownership 未回收 | TRUE | `chat_runtime_impl.rs` 3900 行主循环；`RuntimeChatTurnDriver` 是 passthrough |
| A2: PluginContext 仍在热路径 | TRUE | 36 个文件、42 个函数签名接受 `&PluginContext` |
| A3: 取消模型分裂 | TRUE | `registry.rs` 两处 `new()`、`chat_runtime_impl` 独立 token、python 无透传 |
| A4: 权限边界未全覆盖 | TRUE | `registry.execute()` legacy 回退用 `allow_all()`；无 Ask 变体 |
| A5: 事件真相源有合成痕迹 | TRUE | `exec-msg-<run_id>` + `{"executor_owned": true}` synthetic payload |

## 分期路线图

```
S1 取消模型统一
  ↓
S2 权限 bypass 消除
  ↓          ↘
S3 P1 收尾    S6 权限 ask 前端闭环
  ↓
S4 PluginContext 高价值迁移
  ↓
S5 事件真相源
  ↓
S7 剩余迁移 + 证明链
```

依赖关系：
- S1 → S2：权限修复需要 cancel token 传播已统一
- S2 → S6：ask UI 需要 StorePolicyPipeline 已在生产路径生效
- S3 → S4 → S5：PluginContext 退出依赖 ownership 回收；状态模型依赖工具已迁移
- S7 收尾依赖前面所有期

可并行：S3 与 S2 无强依赖；S6 与 S5 可并行。

---

## S1：取消模型统一

**目标**：消除生产路径中所有孤立的 `CancellationToken::new()`，统一到 `TurnState` 作为唯一 cancel source。

**改动范围**：

| 文件 | 改动 |
|------|------|
| `src-tauri/src/plugin/registry.rs` | `execute()` 签名新增 `cancel: CancellationToken`；line 308 和 352 替换为传入 token |
| `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` | line 1081 本地 `CancellationToken::new()` 替换为从 `RuntimeRunRegistry` 获取的 run-scoped token |
| `src-tauri/src/llm/tool_executor/python.rs` | `execute_for_run()` 调用处透传 cancel token |
| `src-tauri/src/python/session.rs` | LRU eviction spawn 内部检查 `token.is_cancelled()` |

**不改**：`TurnState` 本身、`query_engine.rs`（已正确）、工具实现内部。

**验收**：
1. `grep -r "CancellationToken::new()" src-tauri/src/` 生产代码归零（test 除外）
2. 全量 `review_` 测试绿
3. 新增测试：`registry.execute()` 透传 token cancelled 时工具执行被中断

---

## S2：权限 bypass 消除

**目标**：消除 `allow_all()` 在生产路径的使用，所有工具入口统一经过 `StorePolicyPipeline`。

**改动范围**：

| 文件 | 改动 |
|------|------|
| `src-tauri/src/plugin/registry.rs` | `execute()` legacy 回退路径 line 340 从 `allow_all()` 改为 `StorePolicyPipeline` |
| `src-tauri/src/commands/chat.rs` | line 213 直接调用改为走统一 dispatcher |
| `src-tauri/src/runtime/tools/dispatcher.rs` | `allow_all()` 标记 `#[cfg(test)]` |

**不改**：`PolicyDecision` 枚举（Ask 是 S6）、前端、`StorePolicyPipeline` 逻辑。

**验收**：
1. `grep -r "allow_all" src-tauri/src/` 非 test 代码归零
2. 新增回归测试：legacy tool 经 `registry.execute()` 调用时 unknown scope 被 deny
3. 全量测试绿

---

## S3：P1 收尾 — tool round ownership

**目标**：执行已有 P1 计划 Task 4，`TauriLegacyTurnExecutor` 实现 `run_llm_step`，tool dispatch 经过 session-level runtime。

**设计**：沿用 `docs/superpowers/plans/2026-04-15-p1-tool-round-ownership-plan.md` Task 4，不重新设计。

**验收**：
1. T1-T6 review 测试在 production path 全绿
2. `legacy_send_message_impl` 不再持有 tool dispatch 逻辑
3. P1 标记已关闭

---

## S4：PluginContext 高价值工具迁移

**目标**：`load_file` 和 `execute_python` 转为 runtime-native contract，不再构造 `PluginContext`。

**改动范围**：
- `runtime/tools/builtin/file.rs`：`LoadFileRuntimeTool` 直接使用 `CapabilityContext`，移除 `build_plugin_ctx()` 桥接
- `llm/tool_executor/python.rs`：改为接受 `ToolExecutionContext`
- `chat_runtime_impl.rs`：line 1812（precompute）和 line 2768（tool round）两处 `PluginContext` 构造移除

**验收**：两个工具路径无 `PluginContext` 构造。

---

## S5：事件真相源

**目标**：runtime 事件携带真实数据，消除 synthetic marker。

**改动范围**：
- `runtime/chat/chat_turn_driver.rs`：`message_persisted` 改为真实 message ID 和内容
- `runtime/state.rs`：`TurnState` 扩展 `messages` / `active_tool_calls` 集合

**验收**：`grep "exec-msg-" src-tauri/src/` 归零。

---

## S6：权限 ask 前端闭环

**目标**：完整的权限 ask 交互闭环。

**改动范围**：
- `PolicyDecision` 新增 `Ask` 变体
- 后端通过 runtime event 发起 ask，等待前端响应
- 前端权限对话框 + 记忆/撤销 UI

**验收**：端到端 ask 流程可演示。

---

## S7：剩余迁移 + 证明链

**目标**：收尾。

**改动范围**：
- 批量迁移剩余高频工具的 `&PluginContext` 接受点
- C1 ownership gating test
- C2 非 chat 权限一致性测试
- C3 并发验证（多会话 cancel、Python eviction、子 agent）

**验收**：全量证明测试套件绿。

---

## 不做的事

- 不重新设计已关闭的 P0-P3 专项
- 不动前端 store 架构（S6 只加权限 UI，不重构 chatStore）
- 不做 Workspace-First / Atomic Tool / Prompt Slimming / Skill 导入统一（这些已独立关闭或是独立专项）
- S4-S7 到执行前再出详细实施计划，本轮只为 S1-S3 出完整计划
