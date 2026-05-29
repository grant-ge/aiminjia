# 2026-04-15 架构闭环分期设计

目的：基于 2026-04-15 当前代码状态，为“还没闭合的架构边界”设计一个**可分期、可独立落地、可验证**的收口方案。  
范围：本设计聚焦当前最值得继续推进的三期工作 `S1-S3`；完整的 LLM streaming ownership 回收与 runtime state 真相源统一，保留到后续阶段。  
结论口径：当前主链路**功能上已基本可用**，但仍不能说“runtime-first 架构已经 fully closed”。

---

## 一、当前基线

结合当前代码、review 文档和验证结果，可以确认：

- chat 主链路中的 **tool dispatch 已经走 runtime path**
  - `RuntimeChatTurnDriver` 已能驱动 `ToolRoundDriver`
  - `review_chat_tool_dispatch_runtime_test` 与 `send_message_production_path_test` 当前为绿色
- `P4-A` 主链路 cancel token 与 `PermissionStore` 原子写入已落地
- `StorePolicyPipeline` 已接入 chat 主链路 dispatcher
- `file_meta / is_degraded / degradation_notice` 的 runtime 透传已落地

但同时仍然成立的是：

1. **LLM streaming / orchestration ownership 仍在 `legacy_send_message_impl(...)`**
2. **非 chat / legacy fallback 入口的权限边界还没有完全统一**
3. **取消传播仍按入口点分裂，尚未形成单一 cancel source**
4. **`PluginContext` 仍在 hot path 承担过重桥接职责**
5. **runtime 事件可用，但 state 真相源仍有 compat / synthetic 痕迹**

所以接下来的收口原则应当是：

> **优先把边界收干净，而不是继续堆“看起来能跑”的局部能力。**

---

## 二、为什么不能一轮大重构

当前剩余问题里，风险最高但也最重的，是把 `agent_loop` / `legacy_send_message_impl(...)` 中的 streaming loop 完整回收到 runtime。

这一段不仅是 tool round，还包括：

- context decay
- masking
- retry / timeout / phase advance
- precompute
- checkpoint extraction
- assistant message 收尾
- 流式事件发射

直接一轮“把所有权一次性收回来”会带来两个问题：

1. **改动面过大**：容易把已转绿的主链路重新打红
2. **证明链变差**：很难区分“权限问题 / 取消问题 / ownership 问题”到底是哪一层回归

因此，本设计采用：

> **窄切面、分阶段、每期独立可验证** 的方案。

---

## 三、设计原则

### 1. 先收边界，再收 ownership

完整 ownership 回收是后续阶段的大任务；在那之前，先把：

- 权限边界
- 取消边界
- 高价值 legacy bridge

这些更容易腐化架构的点先收干净。

### 2. 每期都必须可单独合并

每一期都要满足：

- 可独立编译
- 有明确回归测试
- 不依赖“下一期做完才成立”

### 3. 不用“文档已关闭”替代“架构已关闭”

对当前架构的判断要保持克制：

- `tool dispatch` 已闭合，不等于 `streaming ownership` 已闭合
- `cancel` 主链路已修，不等于系统级 cancel model 已统一
- `event` 能驱动前端，不等于 runtime state model 已统一

---

## 四、分期设计

## S1：权限边界统一

### 目标

把**所有当前仍在用的工具入口**统一到同一条 permission boundary 上，消除“chat 主链路走 StorePolicyPipeline，但其他入口还能绕开”的双轨状态。

### 需要解决的现状问题

- `src-tauri/src/plugin/registry.rs` 的 `execute()` 在 legacy fallback 时仍用 `ToolDispatcher::allow_all()`
- `src-tauri/src/commands/chat.rs` 等非 chat 入口会复用 `ToolRegistry.execute()`，因此也会继承这条 bypass
- 当前 `PermissionStore + StorePolicyPipeline` 已经具备 allow / deny / persisted / unknown-scope fail-closed，但入口覆盖还不完整
- `ask` 交互尚未闭环，但这不应成为 bypass 合理化的理由

### 设计决策

- **S1 在后端层把权限契约升级为三态**（`Allow / Deny / Ask`），对标 S0 的 `PermissionDecision`
- 消除 legacy fallback 的 `allow_all()` bypass
- Ask 语义在后端完整保留——Dispatcher 返回 `AskRequired`，TurnDriver 按 mode 决定处理方式
- **S1 不做前端 ask UI**——Ask 在 S1 由 TurnDriver 暂转 deny，但后端接口已就位，S6 只需在 TurnDriver 层改 Ask 分支
- `unknown scope` 在 `StorePolicyPipeline` 下返回 Ask（而非直接 deny），给后续 ask 流程预留正确入口

### S1 完成后的状态

- “从哪个入口进来”不再决定权限是否生效
- 后端权限契约已是三态——后续 ask / revoke / visibility 可以建立在统一边界之上，不需要再重塑接口

### S1 非目标

- 不实现前端 Ask 对话框（UI 在 S6 做）
- 不做完整权限产品化，只做后端契约升级 + 入口统一

---

## S2：取消传播统一

### 目标

在**当前 ownership 结构不大改**的前提下，把取消传播从"各处自建孤立 token"改为**层级 cascade**：session root → turn child → tool-call child。每一层只能通过 `child_token()` 派生下一层，不允许 clone / default 伪装。

### 需要解决的现状问题

- `src-tauri/src/plugin/registry.rs` 的 runtime / legacy fallback 两条路径都在自己 `CancellationToken::new()`
- `src-tauri/src/llm/tool_executor/python.rs` 的旧执行路径仍未透传 cancel token
- `PluginContext` 仍是 legacy tool / request-scoped runtime tool 的桥接载体，但当前不携带 cancellation
- chat 主链路虽然已有 `cancel_token.cancel()` 调用，但其下游消费链仍未全部接上

### 设计决策

- **S2 不通过 `PluginContext` 加字段做取消透传**——那会继续加厚 service locator
- `CancellationToken` 升级为层级 cascade 模型：
  - Session root token
  - Turn child token
  - Tool-call child token
- 只允许通过 runtime 自有路径派生 child token，禁止用 `CancellationToken::default()` 或重命名来伪装闭环
- 未能接入真实 parent token 的 side path 必须保留 `CancellationToken::new()` 并加 `FIXME(S4)`，让 grep gate 能持续暴露未闭合点
- `child_token()` 实现必须是 shared-state / weak-child-registry 的事件驱动传播，禁止一 child 一线程轮询

### 关键边界说明

S2 做完后，**chat 主链路的 tool dispatch** 应形成：

```text
session/root cancel
  → turn child cancel
    → tool-call child cancel
```

但 S2 不假装所有 legacy / sub-agent / non-chat side path 都已经闭合。未闭合路径必须显式标记，不能 cosmetic pass。

### S2 非目标

- 不在 S2 里完全移除 `PluginContext`
- 不要求所有 legacy `ToolPlugin` 都支持 cancel（它们仍受旧接口限制）
- 不要求 background / child-run / sub-agent 全部 runtime-native 化

---

## S3：高价值 legacy bridge 收缩

### 目标

优先移除当前最“热”、最影响 runtime-first 纯度的桥接点，降低 `PluginContext` 在生产热路径上的存在感，为后续 ownership 回收做准备。

### 需要解决的现状问题

- `src-tauri/src/runtime/tools/builtin/file.rs` 的 `LoadFileRuntimeTool` 仍靠 `build_plugin_ctx()` 回桥
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` 的 precompute auto-load 仍构造 `PluginContext`
- `execute_python` 虽已部分改成 cancel-aware，但仍主要沿用 legacy handler 模式
- hot path 里继续构造 `PluginContext`，会让 capability / permission / cancel / event 边界都带着兼容层味道

### 设计决策

- **S3 不尝试回收完整 streaming ownership**
- S3 聚焦“高价值 hot path bridge”
- 第一优先级是：
  1. `load_file` runtime bridge
  2. precompute auto-load 路径
  3. `execute_python` 的 cancel-aware / request-scoped deps 收口

### S3 完成后的状态

- `PluginContext` 仍存在，但不再卡在最核心的 hot path 上
- ownership 回收时，需要迁的点会明显减少
- runtime / capability / request-scoped deps 的边界会更清晰

### S3 非目标

- 不关闭 P1 的“完整 streaming ownership”问题
- 不在本期把全部 legacy tools 一次性迁完
- 不在本期解决 runtime state synthetic marker 问题

---

## 五、S1-S3 之后的后续阶段

## S4：完整 ownership 回收

这才是把 `legacy_send_message_impl(...)` 中的 streaming/orchestration loop 真正迁入 runtime 的阶段。  
它对应当前 review 中保留的那个 P1 级 open finding。

S4 之前，不应把 P1/P1-A 写成 fully closed。

## S5：runtime state 真相源收口

在 ownership 回收后，再把：

- synthetic `message_persisted`
- compat payload
- 前端兼容事件消费

逐步统一成真正 runtime-owned state model。

## S6：权限 ask / revoke / visibility 产品闭环

在 S1 建立统一权限边界后，再把 ask / remember / revoke / UI 状态做完整。

---

## 六、阶段依赖关系

推荐依赖关系如下：

```text
S1 权限边界统一
  ↓
S2 取消传播统一
  ↓
S3 高价值 legacy bridge 收缩
  ↓
S4 完整 ownership 回收
  ↓
S5 runtime state 真相源收口
  ↘
   S6 权限 ask/UI 闭环
```

解释：

- **S1 最先做**：因为它最像架构安全边界问题，且切面最窄
- **S2 第二做**：因为很多 legacy 热路径仍要靠 `PluginContext` / registry 承接，先把 cancel 传通
- **S3 第三做**：把最热的桥点收掉，降低 S4 的重构面
- **S4** 才是大头：完整 ownership 回收

---

## 七、验收口径

### S1 验收口径

- `PermissionPipeline::authorize()` 返回三态 `PermissionDecision`（Allow/Deny/Ask），不是 `Result<()>`
- 生产代码中不再存在 `allow_all()` bypass
- `ToolDispatcher.dispatch()` 对 Ask 返回 `AskRequired`，不在 Dispatcher 层压扁
- `StorePolicyPipeline` 对 unknown scope 返回 Ask（不是 Deny）
- legacy fallback 与 runtime path 使用同一个 pipeline

### S2 验收口径

- `CancellationToken` 支持 `child_token()` cascade（shared-state 事件驱动，不是轮询线程）
- cancel 形成 session → turn → tool_call 三级层级
- chat 主链路的 tool round 已接入真实 parent token
- 未闭合 side path 保留 `CancellationToken::new()` + FIXME 标记，不用 `default()` 伪装闭环
- 禁止用 PluginContext 做 cancel 透传（不给 PluginContext 加 cancel_token 字段）

### S3 验收口径

- `LoadFileRuntimeTool` 通过 `CapabilityContext.file_ops`（`FileOperations` trait accessor）访问文件能力，不再构造 PluginContext
- precompute auto-load 路径使用 runtime-friendly helper，不再构造 full PluginContext
- **运行时语义 gate**（不只看代码形状）：
  - `loaded/load_failed` key 语义保持（`loaded:{scope_id}:{file_id}`）
  - `file_meta/generatedFiles/degradation` 透传到 TurnDriver 不回退
  - cancellation 来自 turn cascade（parent cancel 时 load_file 可观察到 cancelled）
  - 经过统一 permission pipeline（不是 allow_all）
- `execute_python` 的迁移边界已显式定义（本期只做边界收敛，不做完整迁移）

---

## 八、一句话结论

`S1-S3` 的设计目标不是“立刻把所有架构问题一次性做完”，而是：

> **先把权限边界、取消传播、高价值 legacy bridge 这三条最容易继续腐化系统的主线收紧，为后续 ownership 大迁移创造一个风险更低的地基。**
