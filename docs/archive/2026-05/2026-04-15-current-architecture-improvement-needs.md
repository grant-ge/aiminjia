# 2026-04-15 当前架构下仍需推进的改进需求

目的：基于 2026-04-15 当前代码状态，重新梳理“还有哪些需求改进值得继续做”。  
口径：这是一份**架构改进 backlog**，不是 bug 复盘，也不是要求本轮一次性做完的修复单。  
适用范围：以当前 `pzc` 分支工作树为准；若与更早的泛化 worklist 冲突，以本文为准。

---

## 结论先行

当前系统已经不是“legacy chat + legacy tool”原地踏步的状态了：

- chat 主链路的 tool dispatch 已经走到 runtime 路径；
- runtime event bus、`send_message` 生产 gating tests、`review_` 回归、权限持久化原子写入、P4-A 主链路 cancel token 都已有实质落地；
- 因此，**当前剩余问题的重点不再是“修功能红灯”，而是“把架构边界和真相源收干净”。**

换句话说：

> 现在最需要推进的，不是再补几个点状功能，而是把 ownership、权限边界、取消边界、工具边界、状态真相源真正闭合。

---

## 已不应再反复作为主问题推进的项

这些问题按当前代码状态看，已经不应继续作为“主要残留问题”重复推进：

- `chat_runtime_impl.rs` 本地假 `CancellationToken` 完全不触发：**已修**，主链路上已真实 `cancel()`
- Python LRU eviction 永远固定传 `None`：**主链路已修**，但旧 `PluginContext` Python 路径仍有 follow-up
- tool dispatch 完全绕过 runtime dispatcher：**chat 主链路已不成立**
- 双 `RunId`：**已不再是当前 blocker**
- `PermissionStore` 仅原地覆写：**已改为 temp + rename**
- `review_` 假绿：**已修**

因此，后续 backlog 不应再把这些项当成“当前最大问题”的主叙事。

---

## A. 必做的架构闭环项

这些项不一定都是“马上会出错”的 correctness bug，但如果不做，就不能说 runtime-first 架构已经真正闭合。

### A1. LLM streaming / orchestration ownership 仍未真正回收到 runtime

**现状**

- `src-tauri/src/transport/tauri_commands/chat.rs` 中，`TauriLegacyTurnExecutor` 仍通过 `run_chat_turn()` 直接委托 `legacy_send_message_impl(...)`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` 虽然已经具备 iterative tool-round contract，但 executor-backed 路径仍不是完整 streaming owner
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` 里仍保留约一整段主循环：context decay、masking、retry、phase 推进、precompute、tool-call message 拼装、assistant message 收尾等

**为什么它仍是需求**

- 当前功能是通的，但 runtime 还不是聊天生命周期的唯一 owner
- 现在的 runtime 更像“包住 tool round + 兼容事件发射”，不是完整 streaming/orchestration 的单一真相源
- 后续任何和 chat lifecycle 相关的能力，仍容易被迫回到 legacy transport 层上做

**目标**

- `RuntimeChatTurnDriver` 成为真实聊天 turn 的 owner
- `legacy_send_message_impl(...)` 退化为窄 helper，而不是继续持有整轮编排
- `message_persisted / stream_done / agent_idle` 对应真实生命周期，不再依赖 executor-backed synthetic marker

**建议口径**

- 这是 **Tasks 3/4 级别的大重构**，不是一个“小修”
- 当前可继续保持 P1/P1-A open，而不是误记为 fully closed

---

### A2. `PluginContext` 仍未退出热路径，legacy bridge 还偏重

**现状**

- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` 在 precompute 自动 load_file 和 tool round 入口仍会构造 `PluginContext`
- `src-tauri/src/runtime/tools/builtin/file.rs` 里的 `LoadFileRuntimeTool` 仍需回桥到 `PluginContext`
- `src-tauri/src/runtime/tools/legacy_adapter.rs` 仍是大量 legacy tool 接入 runtime dispatcher 的核心桥
- 大量 `plugin/builtin/tools/*` 与 `llm/tool_executor/*` 仍直接接受 `&PluginContext`

**为什么它仍是需求**

- 这会让 service locator 语义继续泄漏进 runtime hot path
- 权限、取消、事件、能力边界虽然“能跑”，但不是窄上下文主导，而是靠桥接维持
- 后续若继续叠加功能，`PluginContext` 很容易重新膨胀

**目标**

- 新主路径统一消费 `ToolExecutionContext + CapabilityContext + request-scoped deps`
- `PluginContext` 降级为兼容层，而不是继续承担主要生产流量
- 先优先收掉高价值桥点：`load_file`、`execute_python`、文件导出/图表生成相关 handler

---

### A3. 取消模型仍未完全统一到单一 cancel source

**现状**

- `src-tauri/src/runtime/state.rs` 已有 `TurnState.cancellation()`
- `src-tauri/src/runtime/query_engine.rs` 在 runtime tool path 会使用 `turn.cancellation()`
- 但其他路径仍存在“各自 new 一个 token”：
  - `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` 仍创建本地 turn token
  - `src-tauri/src/plugin/registry.rs` 在 `execute()` 的 runtime / legacy fallback 中都直接 `CancellationToken::new()`
  - `src-tauri/src/llm/tool_executor/python.rs` 旧路径仍调用 `execute_for_run()`，没有透传 cancel token

**为什么它仍是需求**

- 当前 chat 主链路 cancel 基本可用，但系统级取消语义仍按入口点分裂
- 一旦涉及非 chat 工具调用、workspace helper、legacy ToolPlugin、子任务/子 agent，就很难保证“同一个 run 的取消语义一致”

**目标**

- 统一以 `TurnState` / `RunRegistry` 作为 cancel source
- 禁止在生产路径随手 `CancellationToken::new()`
- Python、tool dispatcher、sub-agent、background task 都接受同一条 run-scoped token

---

### A4. 权限边界还没有覆盖所有入口点，`ask` 交互也未闭环

**现状**

- chat 主链路通过 `to_runtime_dispatcher()` 已能吃到 `StorePolicyPipeline`
- 但 `src-tauri/src/plugin/registry.rs` 的 `execute()` 在 fallback 到 legacy tool 时，仍走 `ToolDispatcher::allow_all()`
- `src-tauri/src/commands/chat.rs` 里还有直接 `tool_registry.execute(...)` 的路径
- 当前 `PermissionStore + StorePolicyPipeline` 已覆盖 allow/deny/persisted/fail-closed unknown scope，但 **ask/UI 流程尚未闭环**

**为什么它仍是需求**

- 只要存在“某些入口走 policy engine，某些入口还能绕开”的情况，权限系统就还不是唯一裁决边界
- 即使后端决策逻辑正确，如果没有 ask / remember / revoke / 可见性闭环，产品能力也不完整

**目标**

- 所有工具入口统一经过同一条 policy boundary
- legacy fallback 不允许再用 `AllowAllPermissionPipeline` 兜底
- 前后端补齐 ask / remember / revoke / 状态展示

---

### A5. runtime 事件已可用，但状态真相源仍有 synthetic / compat 痕迹

**现状**

- `src-tauri/src/runtime/chat/chat_turn_driver.rs` 在 executor-backed 路径会发出 `message_persisted / stream_done / agent_idle`
- 其中 `message_persisted` 仍是 synthetic payload：`exec-msg-<run_id>` + `{"executor_owned": true}`
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` 虽然已经开始聚合 `file_meta` / `generatedFiles`，但仍主要是 compatibility 式承接，而不是完整 runtime-owned state model

**为什么它仍是需求**

- 当前事件“能驱动前端”，不代表它已经是可信的状态真相源
- 只要 persisted message payload、tool result payload、frontend state 仍混有 compat marker，就很难做一致的运行态审计和后续前端状态建模

**目标**

- 运行态事件尽量来自真实 runtime store / lifecycle，而不是 synthetic marker
- `message / run / tool_call / permission` 逐步对齐为一套 runtime state model

---

## B. 建议尽快推进，但不一定阻塞主链路的项

### B1. 统一所有工具入口，减少“chat path”和“non-chat path”双轨

**现状**

- chat 主链路：`ToolRoundDriver -> QueryEngine -> ToolDispatcher`
- 部分非 chat helper / command path：仍直接 `tool_registry.execute(...)`

**建议**

- 收口到统一的 runtime tool invocation surface
- 让 workspace-first helper、命令入口、调试入口尽可能复用同一 dispatch / permission / event 模型

---

### B2. 补齐 `file_meta / generatedFiles / degradationNotice` 的端到端验证

**现状**

- `ToolResult -> RuntimeToolCallOutcome -> chat_runtime_impl` 的透传已补齐
- 但还缺真正的 e2e 证明：文件型工具在真实聊天链路里生成的 `generatedFiles / isDegraded / degradationNotice` 最终能稳定进入 assistant message / UI 消费层

**建议**

- 增加从真实文件型 tool 到 assistant payload 的端到端测试
- 补齐 degraded/export/chart/report 这类高价值文件工具场景

---

### B3. 继续把高频高价值工具迁到 runtime-native contract

**优先建议顺序**

1. `load_file`
2. `execute_python`
3. 文件导出 / 图表 / 报告生成
4. 浏览器类 request-scoped tools

**原因**

- 这些工具和权限、取消、文件元信息、workspace 能力、generatedFiles 展示都有直接耦合
- 越晚迁，legacy bridge 越难收

---

### B4. 前端状态模型还需要从“兼容事件消费”升级到“runtime 语义消费”

**现状**

- 当前关键 happy path 已接通
- 但前端大体仍是消费 legacy/compat 事件名，而不是完整 runtime state model

**建议**

- 先补权限 UI 闭环
- 再逐步把 message/run/tool_call/task 状态消费迁到 runtime 语义

---

## C. 证明链和测试层面仍建议补的项

这些更偏“证明架构真的闭合”，不是新增功能。

### C1. ownership 迁移完成的最终 gating test

当前缺的是：

- 一条真正能证明 `TauriLegacyTurnExecutor` 不再把整轮 turn 留在 `legacy_send_message_impl(...)` 的测试

当前不建议为了这条测试去假装关闭 P1；应等真正做完 A1 再补。

---

### C2. 非 chat 入口的权限一致性测试

当前建议补一条直接覆盖：

- `ToolRegistry.execute()` 在 legacy fallback 下也必须经过统一权限裁决

这条测试能直接验证 A4 是否闭合。

---

### C3. 多会话 / 多任务 / cancel / sub-agent 并发验证

建议补更系统的联调证明：

- 多 conversation 并行 cancel
- Python session eviction 与 turn cancel 并发
- child agent / background task 的取消和收尾
- tool_call / task terminal state 在前后端是否一致

---

## 建议执行顺序

如果继续拆任务推进，建议顺序如下：

1. **A4 权限边界全入口统一 + ask 闭环**
2. **A3 取消模型统一到单一 cancel source**
3. **A2 `PluginContext` 退出热路径，缩减 legacy bridge**
4. **A1 LLM streaming / orchestration ownership 回收（Tasks 3/4 级别）**
5. **A5 runtime state 真相源对齐**
6. **B1-B4 入口统一 / 工具迁移 / file_meta e2e / 前端状态模型**
7. **C1-C3 证明链补齐**

---

## 一句话结论

当前最值得继续推进的改进需求，不是“再补几个功能点”，而是：

> **把 runtime ownership、权限边界、取消边界、legacy bridge、状态真相源这五条主线真正收成一套系统。**

在这五条线没闭合前，lotus-app 可以说“主链路已基本可用”，但还不能说“runtime-first 架构已经完全收口”。
