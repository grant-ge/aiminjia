# 第 2 期：Tool Runtime + Permission Pipeline + 最小持久化

> 目标：统一工具系统，剥离权限判定，建立最小 run/task/tool_call store
> 关键原则：本期不仅解决注册表双轨，更要解决 `PluginContext` 过宽和工具接口不具备 run/task 语义的问题

---

## 一、本期目标

完成以下六件事：

1. 建立统一 `ToolDefinition` / `ToolDispatcher` / `ToolExecutor`
2. 将权限判定抽到独立 `PermissionPipeline`
3. 用 `ToolExecutionContext` 替代全量 `PluginContext`
4. 将 `ToolPlugin::execute()` 改为 task-aware 接口
5. 落地最小 `RunStore / TaskStore / ToolCallStore / AgentInvocationStore`
6. 引入 `LegacyToolAdapter` 作为旧工具迁移桥

### 本期解决的挑战
- C4：PluginContext 按能力域约束
- C5：工具接口必须 task-aware
- C9：最小持久化提前到第 2 期

---

## 二、核心设计

### 2.1 统一工具模型

新增统一抽象：

```rust
pub struct ToolDefinition {
    pub id: String,
    pub display_name: String,
    pub schema: ToolInputSchema,
    pub capabilities: Vec<ToolCapability>,
    pub permission_policy: PermissionPolicy,
    pub executor_kind: ToolExecutorKind,
}
```

要求：
- 所有工具只能通过 `ToolRegistry` 注册
- `llm/tools.rs` 中的静态工具定义逐步迁移为兼容桥
- `plugin/builtin/tools/*` 仍可保留实现，但必须生成统一 `ToolDefinition`

### 2.2 统一执行管线

执行路径固定为：

```text
QueryEngine
  → ToolDispatcher
    → PermissionPipeline
      → ToolExecutor
        → ToolExecutionContext
          → ToolPlugin::execute(...)
            → ToolResult
```

禁止：
- chat/query/runtime 直接调用 tool impl
- tool 自己绕过 dispatcher 做权限判断
- tool 直接回写前端事件

### 2.3 Permission Pipeline

权限管线独立成模块，参考 claude-code-best `permissions.ts`。

```rust
pub struct PermissionRequest {
    pub run_id: RunId,
    pub agent_id: Option<AgentId>,
    pub tool_id: String,
    pub input_summary: String,
    pub capability_scope: Vec<ToolCapability>,
}
```

输出：
- allowed
- denied
- requires_user_confirmation

本期要求：
- 权限判定不再散落在 command/chat/tool 代码里
- 审批结果可记录到 `ToolCallStore`

### 2.4 ToolExecutionContext

用 `ToolExecutionContext` 取代 `PluginContext`：

```rust
pub struct ToolExecutionContext {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub agent_id: Option<AgentId>,
    pub tool_call_id: ToolCallId,
    pub cancellation: CancellationToken,
    pub event_sink: Arc<dyn ToolEventSink>,
    pub capabilities: ToolCapabilities,
}
```

核心思想：
- 工具只能拿到它声明需要的能力
- storage/auth/file/python/browser 不再默认全部可见
- 逐工具收缩权限边界

### 2.5 task-aware 工具接口

`ToolPlugin::execute()` 改造目标：

```rust
async fn execute(
    &self,
    input: serde_json::Value,
    ctx: ToolExecutionContext,
) -> Result<ToolResult, ToolError>
```

必须支持：
- `run_id`
- `agent_id`
- `tool_call_id`
- `CancellationToken`
- 事件上报接口

这样后续才能支持：
- 工具取消
- 工具审计
- agent 内工具链路跟踪
- 背景工具执行

### 2.7 Legacy Tool Migration Strategy

由于现有大量工具依赖旧 `PluginContext`，第 2 期不能要求一次性全部改成新接口，必须引入过渡适配层：

- 新工具：必须实现新接口，直接接入 `ToolDispatcher`
- 旧工具：通过 `LegacyToolAdapter` 包装后接入 `ToolDispatcher`

桥接规则：
- `ToolExecutionContext` 降级映射到旧 `PluginContext`
- 先保持兼容暴露的能力：storage、file、auth、python、browser 的必要子集
- 在旧接口下不可用或受限的能力：run-scoped audit、细粒度 cancellation、agent-aware event sink

#### Tool Interface Deprecation Timeline

- 第 2 期结束：禁止新增旧接口工具
- 第 3 期：持续迁移内建工具到新接口
- 第 4 期结束：移除旧接口 shim 与 `LegacyToolAdapter`


本期落地 repository trait，但底层继续使用现有 file-based store。

新增 trait：

```rust
pub trait RunStore {
    fn create_run(&self, run: RunRecord) -> Result<()>;
    fn update_run_status(&self, run_id: &RunId, status: RunStatus) -> Result<()>;
}

pub trait TaskStore {
    fn create_task(&self, task: TaskRecord) -> Result<()>;
    fn update_task_status(&self, task_id: &TaskId, status: TaskStatus) -> Result<()>;
}

pub trait ToolCallStore {
    fn create_tool_call(&self, call: ToolCallRecord) -> Result<()>;
    fn update_tool_call(&self, tool_call_id: &ToolCallId, status: ToolCallStatus) -> Result<()>;
}

pub trait AgentInvocationStore {
    fn create_invocation(&self, record: AgentInvocationRecord) -> Result<()>;
    fn update_invocation_status(&self, agent_id: &AgentId, status: AgentStatus) -> Result<()>;
}
```

#### AgentInvocationRecord 最小字段

```rust
pub struct AgentInvocationRecord {
    pub agent_id: AgentId,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub status: AgentStatus,
    pub background: bool,
    pub summary_or_output_ref: Option<String>,
}
```

#### Why AgentInvocation Needs Persistence Before Background Mode

只要第 3 期要支持 background sub-agent，就不能让 agent 生命周期只存在于内存里。否则：
- 主 run 无法可靠读取后台 agent 状态
- agent 完成/失败后无法持久化 summary
- background 模式一旦进程抖动就丢失执行状态

因此第 2 期先落最小 `AgentInvocationStore` 占位版，第 3 期再把它提升为正式真相源。

第 2 期只要求：
- run/tool_call 至少能记录开始/结束/失败/取消
- task store 支持最小占位
- agent invocation store 能记录 background 预备字段与最小状态变化

---

## 三、新增文件（建议）

```text
src-tauri/src/runtime/tools/
├── mod.rs
├── definition.rs              # ToolDefinition
├── dispatcher.rs              # ToolDispatcher
├── executor.rs                # ToolExecutor
├── permission.rs              # PermissionPipeline
├── context.rs                 # ToolExecutionContext
├── result.rs                  # ToolResult / ToolError
├── legacy_adapter.rs          # LegacyToolAdapter
└── registry.rs                # 统一 ToolRegistry（或桥接现有 registry）

src-tauri/src/runtime/store/
├── mod.rs
├── run_store.rs               # RunStore trait + file-based impl bridge
├── task_store.rs              # TaskStore trait + minimal impl
├── tool_call_store.rs         # ToolCallStore trait + file-based impl bridge
└── agent_invocation_store.rs  # AgentInvocationStore trait + minimal impl
```

迁移涉及的旧文件：

```text
src-tauri/src/llm/tools.rs
src-tauri/src/plugin/registry.rs
src-tauri/src/plugin/context.rs
src-tauri/src/plugin/tool_trait.rs
src-tauri/src/plugin/builtin/tools/*
src-tauri/src/storage/file_store/mod.rs
```

---

## 四、迁移方式（文件级）

### 4.1 llm/tools.rs
角色变更：
- 从工具定义主入口，降为兼容层 / 导出桥
- 新工具定义统一落到 runtime/tools/definition.rs + registry.rs

目标：
- 第 2 期末不再允许新增工具继续写到 `llm/tools.rs`

### 4.2 plugin/registry.rs
角色收敛：
- 保留 plugin/builtin 工具发现能力
- 但最终输出统一 `ToolDefinition`
- 注册结果进入唯一 `ToolRegistry`

### 4.3 plugin/context.rs
核心改造：
- 不再向工具暴露“全量上下文”
- 改造成 capability injection
- 原 `PluginContext` 先保留桥接层，但新工具接口禁止使用

### 4.4 plugin/tool_trait.rs
修改 trait：
- `execute()` 接收 `ToolExecutionContext`
- 必须可取消
- 必须支持事件上报

### 4.5 file_store/mod.rs
不改底层格式，但抽出：
- run/tool_call/task 对应 repository bridge
- 所有 Runtime 不直接碰 file_store 细节方法

---

## 五、Compatibility Boundary

本期必须保持：
- 前端 legacy Tauri 事件协议仍兼容：`streaming:delta` / `streaming:done` / `tool:executing` / `tool:completed` / `message:updated` / `agent:idle`
- 现有工具功能对用户表现不变
- 工具输入输出 schema 不强制对前端改形
- file-based 底层格式暂不替换
- 旧 plugin/builtin 工具仍能运行

允许变化：
- 工具内部执行路径统一走 dispatcher
- tool 运行日志/trace 变更
- 权限判断逻辑位置变化
- 旧工具通过 `LegacyToolAdapter` 接入新主链路

---

## 六、Kill List

本期末必须废掉：

1. `chat.rs` / `QueryEngine` 直接调用具体工具实现的路径
2. `PluginContext` 默认暴露全量依赖的用法（新路径禁止）
3. 工具内部自行处理权限判定的散落逻辑
4. `llm/tools.rs` 作为新增工具主注册点的角色

允许保留的兼容层：
- 旧工具定义到新 `ToolDefinition` 的桥接
- `LegacyToolAdapter`
- 旧 PluginContext 到新 ToolExecutionContext 的短期 shim

强约束：
- 第 2 期结束后，禁止新增旧接口工具

---

## 七、Truth Source

第 2 期拍板：

| 状态 | 真相源 |
|------|-------|
| 当前工具是否运行中 | `ToolCallStore` + `TurnState.active_tool_call` |
| 工具权限结果 | `PermissionPipeline` 输出 + `ToolCallStore` |
| 单次工具执行日志 | `ToolCallStore` |
| task 占位状态 | `TaskStore` |
| 工具取消状态 | `CancellationToken` + `ToolCallStore` |
| agent invocation 占位状态 | `AgentInvocationStore` |

注意：
- 工具运行态不再依赖前端推测
- Runtime 与 store 对 tool_call 状态必须一致
- background agent 的状态预留从 `AgentInvocationStore` 读取

---

## 八、Golden Trace 验收

### Trace D：需要权限确认的工具
要求：
1. 生成 PermissionRequest
2. 记录审批结果
3. tool_call 状态更新一致
4. 前端事件顺序保持兼容

### Trace E：长时工具执行
要求：
- `tool:executing` → 中间进度/消息刷新 → `tool:completed` 顺序稳定
- tool_call_store 有完整记录

### Trace F：工具取消
要求：
- `CancellationToken` 真正传到工具内部
- `ToolCallStore` 最终状态为 cancelled
- UI 不留悬空 loading

---

## 九、Cutover Strategy

本期采用**直接替换**：
- Tool 主执行链路直接切到 `ToolDispatcher -> PermissionPipeline -> ToolExecutor`
- 旧工具通过 `LegacyToolAdapter` 进入新链路
- 新 store 直接作为主记录面，不做双写灰度

切换前提：
- 工具回归清单通过
- 3 条 golden trace 回放通过
- `LegacyToolAdapter` 能覆盖当前存量旧工具

## 十、Rollback Strategy

若第 2 期切换失败：
- 回退到旧工具分发路径
- 暂停 `ToolDispatcher` 作为生产主入口
- 保留新 runtime/tools 模块代码，但不挂主路径
- `AgentInvocationStore` 占位实现可保留但不作为读路径

回滚判据：
- 工具无法执行
- 权限判定出现大面积误判
- 前端工具事件序列异常

---

## 十一、Not Doing

本期明确不做：
- 不重构 sub-agent 主循环
- 不引入完整 task framework
- 不改变 Python/browser/connector 底层实现
- 不替换 file-based 持久化格式
- 不重做前端工具交互协议
- 不引入远程 transport

---

## 十二、本期完成定义

第 2 期完成的标志：

1. 所有工具统一经过 ToolDispatcher
2. 权限判定统一经过 PermissionPipeline
3. 新 `ToolExecutionContext` 已生效
4. `ToolPlugin::execute()` 已具备 run/agent/tool_call/cancel/event 语义
5. `RunStore / TaskStore / ToolCallStore / AgentInvocationStore` 最小版本已落地
6. `LegacyToolAdapter` 已接住存量旧工具
7. `llm/tools.rs` 不再是新增工具主入口
8. 3 条 golden trace 回放通过
