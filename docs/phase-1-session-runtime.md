# 第 1 期：身份模型 + TurnState + SessionRuntime + 事件兼容层

> 目标：把 `chat.rs` 从 God function 拆成薄 command adapter + 独立 SessionRuntime
> 关键原则：第 1 期就建立身份模型、RuntimeEvent、Tauri 兼容层，并禁止 Runtime 直接依赖 Tauri 类型

---

## 一、本期目标

完成以下四件事：

1. 引入 `SessionId / RunId / AgentId / ToolCallId` 身份模型
2. 抽离 `TurnState`、`SessionRuntime`、`QueryEngine` 雏形
3. 建立 `RuntimeEventBus` → `TauriEventAdapter` 兼容层
4. 将 `LlmGateway` 从运行态控制器降级为 provider adapter

### 本期解决的挑战
- C1：身份模型不能拖后
- C2：LlmGateway 必须降级成 provider adapter
- C3：事件兼容层必须显式做
- C10：核心 runtime 禁止依赖 Tauri 类型

---

## 二、核心设计

### 2.1 身份模型

新增类型：

```rust
pub struct SessionId(String);
pub struct RunId(String);
pub struct AgentId(String);
pub struct ToolCallId(String);
```

使用规则：
- `SessionId`：从 conversation/session 语义升级而来，跨 turn 持久
- `RunId`：单次用户请求唯一标识
- `AgentId`：单次子代理执行唯一标识
- `ToolCallId`：单次工具调用唯一标识

所有 RuntimeEvent、日志、持久化、trace 必须带 `RunId`。

#### Identity Mapping

第 1~2 期规则明确如下：
- `SessionId` 在实现上暂时直接复用 `conversation_id` 的字符串值，但在类型系统中升级为新类型
- `RunId` 为每次 `send_message` 新生成
- `AgentId` 暂时只在 child run 出现
- `ToolCallId` 在 `ToolDispatcher` 入口生成（第 2 期进入生产主链路）

##### Old Identifier -> New Identifier

| 旧标识 | 新标识 | 规则 |
|--------|--------|------|
| `conversation_id` | `SessionId` | 物理值复用，类型升级 |
| 无 | `RunId` | 每次 send_message 新生成 |
| 无 | `AgentId` | child run / sub-agent 时生成 |
| 无 | `ToolCallId` | ToolDispatcher 入口生成 |

##### Allowed Legacy Usage by Phase

| 场景 | 第 1 期 | 第 2 期 |
|------|--------|--------|
| transport payload | 允许 `conversation_id` | 允许 |
| 旧存储键 | 允许 | 允许 |
| 旧事件 payload | 允许 | 允许 |
| `RunController` | 禁止使用 `conversation_id` 作为真相源 | 禁止 |
| `RuntimeEvent` | 禁止 | 禁止 |
| 新 store / repository | 禁止 | 禁止 |

### 2.2 TurnState

新增 `TurnState`，作为一次 run 的单一内存真相源：

```rust
pub struct TurnState {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub agent_id: Option<AgentId>,
    pub selected_model: String,
    pub user_input: String,
    pub conversation_id: String,
    pub cancellation: CancellationToken,
    pub phase: RunPhase,
    pub active_tool_call: Option<ToolCallId>,
    pub pending_assistant_output: String,
    pub auth_snapshot: AuthSnapshot,
    pub tool_budget: Option<ToolBudget>,
    pub metadata: TurnMetadata,
}
```

`TurnState` 是第 1 期唯一权威内存状态对象，禁止在 Runtime 内再散落 busy/tool/current_output 等局部 mutable 状态。

### 2.3 SessionRuntime / QueryEngine

新增层次：

```text
Tauri command (adapter)
  → SessionRuntime::run_turn(input)
    → QueryEngine::execute_turn(turn_state)
      → ProviderAdapter / SkillPolicy / StorageRepository / EventSink
```

职责：
- `SessionRuntime`：装配依赖、创建 `TurnState`、启动 run、做外层异常边界
- `QueryEngine`：执行一次完整 turn，包括上下文组装、调用 provider、收流式结果、触发后续 follow-up

第 1 期范围内，Tool Runtime 仍可通过旧路径桥接，但必须从 `QueryEngine` 调用，不再由 `chat.rs` 直接编排。

### 2.4 RuntimeEventBus + TauriEventAdapter

#### Current Event Contract

本期前端兼容基线必须以真实 legacy Tauri events 为准：

- `streaming:delta`
- `streaming:done`
- `tool:executing`
- `tool:completed`
- `message:updated`
- `agent:idle`

这些事件名和 payload shape 在第 1 期必须保持兼容。

#### Runtime 内部事件

Runtime 内部统一发：

```rust
pub enum RuntimeEventKind {
    RunStarted,
    StreamStarted,
    StreamDelta { content: String },
    StreamDone,
    ToolCallExecuting { tool_call_id: ToolCallId, tool_name: String },
    ToolCallCompleted { tool_call_id: ToolCallId },
    AgentIdle { agent_id: AgentId },
    MessagePersisted { message_id: String },
    RunFailed { error: String },
    RunCancelled,
    RunCompleted,
}
```

#### RuntimeEvent -> Legacy Tauri Event Mapping

| RuntimeEventKind | Legacy Tauri Event | 说明 |
|------------------|-------------------|------|
| `StreamStarted` | 无 | 仅内部 runtime event |
| `StreamDelta` | `streaming:delta` | 前端兼容基线 |
| `StreamDone` | `streaming:done` | 前端兼容基线 |
| `ToolCallExecuting` | `tool:executing` | 前端兼容基线 |
| `ToolCallCompleted` | `tool:completed` | 前端兼容基线 |
| `MessagePersisted` / flush | `message:updated` | 消息更新兼容事件 |
| `AgentIdle` | `agent:idle` | 子代理空闲/完成兼容事件 |

兼容层：
- `TauriEventAdapter` 订阅 `RuntimeEventBus`
- 仅由 adapter 将内部事件映射成 legacy Tauri events
- **保证前两期 payload 与顺序兼容**

### 2.5 LlmGateway 降级

本期要求：
- 移除 Gateway 对 `active_tasks / busy / cancel state` 的真相源角色
- Gateway 只保留：
  - 选择 provider
  - 发送请求
  - 返回流式 token / 完整响应

`cancel` / `busy` / `clear` 转移到 `RunController`（Runtime 内）。

---

## 三、新增文件（建议）

```text
src-tauri/src/runtime/
├── mod.rs
├── ids.rs                  # SessionId / RunId / AgentId / ToolCallId
├── state.rs                # TurnState / RunPhase / metadata
├── session_runtime.rs      # SessionRuntime
├── query_engine.rs         # QueryEngine
├── events.rs               # RuntimeEvent / RuntimeEventBus
├── run_controller.rs       # busy/cancel/control
├── traits.rs               # Runtime 依赖抽象（EventSink、ProviderAdapter 等）
└── tauri_adapter.rs        # TauriEventAdapter（注意：可放 transport/ 目录，避免 runtime 依赖 tauri）
```

推荐更严格目录：

```text
src-tauri/src/runtime/...
src-tauri/src/transport/tauri_event_adapter.rs
```

### 迁移涉及的旧文件

```text
src-tauri/src/commands/chat.rs
src-tauri/src/llm/gateway.rs
src-tauri/src/lib.rs
src-tauri/src/llm/orchestrator.rs
```

---

## 四、迁移方式（文件级）

### 4.1 chat.rs
**保留** command 入口，但剥离以下职责：
- run id 生成
- busy 状态管理
- provider 调用主循环
- streaming 事件发射
- 主流程编排

`send_message()` 最终只做：
1. 参数校验
2. 读取必要的 AppState/Repository/Adapter
3. 调 `SessionRuntime::run_turn()`
4. 返回 command 结果

### 4.2 gateway.rs
迁移掉：
- active_tasks
- set_busy / clear_busy
- cancel / clear state

保留：
- provider 选择
- 请求发送
- 流式结果桥接

必要时将文件名调整为：
- `provider_adapter.rs`
- 或保留 `gateway.rs` 但职责重定义

### 4.3 orchestrator.rs
第 1 期不做最终命名收敛，但要明确：
- 旧 orchestrator 中属于 turn loop 的部分迁入 `query_engine.rs`
- 旧 orchestrator 保留兼容桥接，成为临时 wrapper

### 4.4 lib.rs
保留服务装配作用，但新增 Runtime 依赖装配：
- ProviderAdapter
- StorageRepository（暂时桥接现有 AppStorage）
- EventSink / TauriEventAdapter
- RunController

---

## 五、Compatibility Boundary

本期必须保持：
- Tauri command 名称不变
- command payload 不变
- 前端订阅的 legacy Tauri 事件名不变：`streaming:delta` / `streaming:done` / `tool:executing` / `tool:completed` / `message:updated` / `agent:idle`
- 前端事件 payload 结构不变
- file store 格式不变
- tool 行为对用户可见表现不变

可接受的内部变化：
- Rust 代码路径变更
- chat.rs 大幅变薄
- gateway 失去运行态控制职责
- 新增内部 runtime event，但不得直接暴露替代 legacy 事件

---

## 六、Kill List

本期末必须废掉：

1. `chat.rs` 内直接控制 busy 的逻辑
2. `chat.rs` 内直接 `app.emit(...)` 的主流程路径
3. `gateway.rs` 作为 busy/cancel 真相源的角色
4. `chat.rs` 内散落的 turn 局部 mutable 状态

本期末允许保留的兼容桥：
- 旧 orchestrator wrapper
- 旧 tool path bridge（仅供第 2 期前临时复用）

---

## 七、Truth Source

第 1 期拍板：

| 状态 | 真相源 |
|------|-------|
| 当前 run 是否 active | `RunController` |
| 当前 turn 内阶段/流式状态 | `TurnState` |
| 当前 assistant 输出缓冲 | `TurnState.pending_assistant_output` |
| 当前工具调用 ID | `TurnState.active_tool_call` |
| provider 请求是否仍在进行 | `RunController` |

注意：
- `LlmGateway` 不再拥有 busy 真相源
- 前端 busy 只作为派生状态，不是权威状态

---

## 八、Golden Trace 验收

本期至少验证 3 条关键链路：

### Trace A：普通聊天
要求事件序列仍兼容：
1. 多个 `streaming:delta`
2. `message:updated`
3. `streaming:done`

### Trace B：单工具调用
要求：
- `tool:executing` / `tool:completed` 顺序不变
- tool 结果仍能回流 assistant message
- 必要时伴随 `message:updated`

### Trace C：取消中断
要求：
- cancel 由 `RunController` 生效
- `streaming:done` 最终可达或明确有兼容收尾事件
- 事件最终一致，不留悬空 busy

额外要求：所有 trace 必须带 `RunId`（可只在日志中，前端不必暴露）。

---

## 九、Cutover Strategy

本期采用**直接替换**：
- `send_message` 主编排路径直接切到 `SessionRuntime -> QueryEngine`
- `chat.rs` 保留为薄 adapter
- 旧 `app.emit(...)` 主流程路径由 `TauriEventAdapter` 统一接管

切换前提：
- legacy Tauri events 映射测试通过
- 3 条 golden trace 回放通过

## 十、Rollback Strategy

若第 1 期切换失败：
- 回退到旧 `chat.rs` 主编排路径
- 暂停使用 `SessionRuntime` 作为生产入口
- 保留新建 runtime 模块代码，但不挂到 command 主路径
- 事件发射退回旧 `app.emit(...)` 实现

回滚判据：
- 前端事件顺序错乱
- busy 状态悬空
- tool 流程明显回归

---

## 十一、Not Doing

本期明确不做：
- 不统一 ToolDefinition
- 不重做 PluginContext
- 不改 ToolPlugin trait
- 不引入 TaskStore / ToolCallStore
- 不修改 Python session 作用域实现
- 不重构 sub-agent
- 不改前端 useChat / useStreaming 行为

---

## 十二、本期完成定义

第 1 期完成的标志：

1. 新 ID 模型已进入生产运行链路
2. `TurnState` 成为一次 run 的单一内存真相源
3. `SessionRuntime` / `QueryEngine` 承接主编排逻辑
4. `chat.rs` 变成薄 adapter
5. `gateway.rs` 降级成 provider adapter
6. Runtime 内不直接 import `tauri::*`
7. 前端事件协议保持兼容
8. 3 条 golden trace 回放通过
