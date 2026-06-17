# Lotus-App 后端架构改造总蓝图

> 对标项目：`claude-code-best`（Bun + TypeScript monorepo CLI agent runtime）
> 改造策略：渐进重构，业务不停机
> 日期：2026-04-10

---

## 一、改造背景

### 1.1 当前架构形态

Lotus-App 是一个 Tauri 桌面应用，运行时为 `WebView(React/TS) + Tauri Host(Rust) + 子进程(Python / 外部 CLI)`。

当前后端核心链路：

```
前端 invoke('send_message')
  → src-tauri/src/commands/chat.rs:540 send_message()
    → 鉴权、读消息、装配上下文、技能激活、模型路由
    → LLM 流式调用、tool loop、sub-agent
    → app.emit() 发事件给前端
    → 持久化到 file_store
```

问题：**send_message 是 God function**，command 层同时承担了 transport adapter、session orchestrator、tool executor、event emitter、state manager 五重角色。

### 1.2 对标架构形态（claude-code-best）

```
CLI 入口 (src/entrypoints/cli.tsx)
  → main.tsx (bootstrapping + 服务装配)
    → QueryEngine.ts (会话编排，transport-neutral)
      → query.ts (单次 turn state + LLM loop + tool follow-up)
        → tools.ts (统一工具注册)
        → AgentTool (子代理调度)
        → tasks.ts / task/framework.ts (任务一等模型)
    → state/store.ts (薄共享状态)
    → permissions.ts (独立权限管线)
    → swarm/backends/registry.ts (可替换执行后端)
```

核心差异：**入口只做 bootstrapping，真正稳定的核心是长生命周期会话 + 可继续的查询循环 + 通用 task/tool 状态机。**

### 1.3 改造总目标

把 lotus-app 后端从"Tauri command 驱动业务"改造为"Session/Query Runtime 驱动业务，Tauri 只是 transport adapter"。

---

## 二、目标架构分层

改造完成后，后端分为 6 层：

### L1: Transport Adapter
- 职责：接收外部请求，转发给 Runtime，订阅 Runtime 事件并转发给调用方
- 当前实现：Tauri commands + app.emit()
- 目标：Tauri command 只做参数接收 → 调用 Runtime → 返回事件句柄
- 约束：**本层禁止包含业务逻辑**

### L2: Session / Query Runtime（核心层）
- 职责：驱动一次完整 agentic turn——消息流、turn state、tool follow-up、模型 fallback、预算控制、中断/恢复
- 对标：`query.ts` + `QueryEngine.ts`
- 约束：**禁止直接依赖 Tauri 类型**，通过 trait 注入宿主能力

### L3: Tool Runtime
- 职责：工具的 schema、routing、permission、execution、audit、lifecycle
- 对标：`tools.ts` + `permissions.ts`
- 约束：**单一工具注册表，单一执行管线**

### L4: Task / Agent Runtime
- 职责：task 生命周期、sub-agent 调度、workflow step 编排、后台运行
- 对标：`tasks.ts` + `task/framework.ts` + `AgentTool`
- 约束：**task 和 agent 是一等模型，不是 chat 过程的附属**

### L5: State Store
- 职责：统一建模和持久化——session/run/task/tool_call/agent_invocation
- 对标：`state/store.ts` + `sessionState.ts`
- 约束：**通过 repository trait 访问，底层实现可替换**

### L6: Infra Adapter
- 职责：LLM provider、Python bridge、file manager、auth、connector、外部 CLI 能力
- 约束：**只提供能力，不参与主流程编排**

---

## 三、ID 模型（第 1 期引入）

从第 1 期开始，系统内流转的核心标识：

| ID | 生命周期 | 说明 |
|----|---------|------|
| `SessionId` | 跨 turn 存活 | 对应一个用户会话（当前 conversation_id 的升级） |
| `RunId` | 单次 turn | 一次用户输入到完成响应的完整过程 |
| `AgentId` | 单次 agent 调用 | 子代理执行的唯一标识 |
| `ToolCallId` | 单次工具调用 | 一次工具执行的唯一标识 |
| `TaskId` | 任务生命周期 | 第 3 期引入，一等任务对象 |

规则：
- 所有 ID 在创建时生成，不可变
- `RunId` 挂在 `SessionId` 下，`AgentId` 挂在 `RunId` 下，`ToolCallId` 挂在 `RunId` 或 `AgentId` 下
- Python session 按 `RunId` 隔离，子 agent 创建独立 Python session

### 3.1 Identity Mapping

第 1~2 期采用“类型升级、物理值复用”的过渡策略：

- `SessionId` 在实现上暂时直接复用现有 `conversation_id` 的字符串值，但在类型系统中升级为新类型
- `RunId` 为每次 `send_message` 新生成的唯一标识
- `AgentId` 暂时只在 child run / sub-agent 场景生成
- `ToolCallId` 在 `ToolDispatcher` 入口生成

#### Old Identifier -> New Identifier

| 旧标识 | 新标识 | 第几期成为真相源 |
|--------|--------|------------------|
| `conversation_id` | `SessionId` | 第 1 期（类型层） |
| 无 | `RunId` | 第 1 期 |
| 无 | `AgentId` | 第 3 期 |
| 无 | `ToolCallId` | 第 2 期 |

#### Allowed Legacy Usage by Phase

| 场景 | 第 1 期 | 第 2 期 | 第 3 期后 |
|------|--------|--------|-----------|
| transport payload | 允许保留 `conversation_id` | 允许 | 逐步迁移 |
| 旧存储键 | 允许 | 允许 | 迁移到新 store 时清理 |
| 旧事件 payload | 允许 | 允许 | 由 adapter 托底 |
| `RunController` 内部状态 | 禁止 | 禁止 | 禁止 |
| `RuntimeEvent` | 禁止 | 禁止 | 禁止 |
| 新 store/repository | 禁止 | 禁止 | 禁止 |

规则：
- `conversation_id` 可以继续出现在 transport payload、旧存储键、旧事件 payload 中
- `conversation_id` 不允许继续作为运行态真相源存在于 `RunController`、`RuntimeEvent`、新 store 中
- 所有新增运行态逻辑必须优先使用 `SessionId / RunId / AgentId / ToolCallId`

---

## 四、事件协议

### 4.1 Current Event Contract

在开始重构前，必须以当前真实后端事件为准。前端兼容基线至少包括：

- `streaming:delta`
- `streaming:done`
- `tool:executing`
- `tool:completed`
- `message:updated`
- `agent:idle`

说明：
- 上述是当前前端协议层要兼容的 **legacy Tauri events**
- 如果未来 Runtime 内部需要 `StreamStarted`、`RunStarted` 等事件，必须明确标记为 **内部 runtime event**，不能冒充现状前端协议

### 4.2 Runtime 内部事件（新增）

```
RuntimeEvent {
    run_id: RunId,
    agent_id: Option<AgentId>,
    tool_call_id: Option<ToolCallId>,
    kind: RuntimeEventKind,
    timestamp: u64,
}

enum RuntimeEventKind {
    RunStarted,
    RunCompleted { finish_reason },
    RunCancelled,
    StreamStarted,
    StreamDelta { content },
    StreamDone,
    ToolCallExecuting { tool_name, input },
    ToolCallCompleted { output },
    ToolCallFailed { error },
    AgentIdle { summary },
    MessagePersisted { message_id },
    StateTransition { from, to },
}
```

### 4.3 RuntimeEvent -> Legacy Tauri Event Mapping

| RuntimeEventKind | Legacy Tauri Event | 说明 |
|------------------|-------------------|------|
| `StreamStarted` | 无 | 仅内部 runtime event |
| `StreamDelta` | `streaming:delta` | 前端兼容基线 |
| `StreamDone` | `streaming:done` | 前端兼容基线 |
| `ToolCallExecuting` | `tool:executing` | 前端兼容基线 |
| `ToolCallCompleted` | `tool:completed` | 前端兼容基线 |
| `MessagePersisted` / message flush | `message:updated` | 继续兼容旧消息刷新机制 |
| `AgentIdle` | `agent:idle` | 兼容当前子代理完成/空闲语义 |

### 4.4 事件兼容层

前两期前端不改事件协议。兼容方式：

```
SessionRuntime
  → 发 RuntimeEvent
    → TauriEventAdapter
      → 转成现有 streaming:delta / streaming:done / tool:executing / tool:completed / message:updated / agent:idle
        → app.emit(...)
```

`TauriEventAdapter` 是显式兼容层，负责：
- 把 RuntimeEvent 映射成前端期望的事件名和 payload 格式
- 保持事件顺序语义不变
- 在第 4 期前端迁移后可以移除
- 明确区分“内部 runtime event”和“legacy Tauri event”

---

## 五、10 个架构约束

这些约束贯穿所有分期，不可违背：

### C1: 身份模型第 1 期引入
SessionId / RunId / AgentId / ToolCallId 从第 1 期就存在于 TurnState 中。

### C2: LlmGateway 降级为 provider adapter
active_tasks / set_busy / cancel / clear 从 gateway 移入 Runtime。Gateway 只负责"给哪个 provider 发请求、拿流式响应"。

### C3: 事件兼容层显式实现
RuntimeEventBus + TauriEventAdapter 在第 1 期就落地。核心 runtime 只发结构化 RuntimeEvent。

### C4: PluginContext 按能力域约束
第 2 期将 PluginContext 替换为 ToolExecutionContext，按需传入能力，不再 service locator。

### C5: 工具接口 task-aware
第 2 期改 ToolPlugin::execute() 签名，加入 run_id / agent_id / CancellationToken / EventSink。

### C6: Python session 按 RunId 隔离
子 agent 创建独立 session，background run 也独立。conversation_id 不再是 session 复用键。

### C7: SubAgent 分 3 段演进
- A（第 3 期前半）：child run + 受限工具集 + cancel 真正生效
- B（第 3 期后半）：可后台 + 可发消息回主 run
- C（第 4 期）：可恢复 + worktree 隔离 + team 协作

### C8: Skill 是 QueryEngine 的策略插件
Skill 控制 prompt / tool filter / step 策略。Workflow step 由 Runtime 编排，Skill 不是 TaskRuntime 的 supervisor。

### C9: 最小持久化提前到第 2 期
第 2 期落 RunStore / TaskStore / ToolCallStore / AgentInvocationStore 最小版本（repository trait + file-based 底层）。

### C10: 核心 Runtime 禁止依赖 Tauri 类型
从第 1 期开始，`src-tauri/src/runtime/` 下的所有模块禁止 import `tauri::*`。需要 AppHandle 的地方通过 trait 注入。

---

## 六、5 个关键决策记录

| # | 决策问题 | 决策 | 理由 |
|---|---------|------|------|
| D1 | 前两期前端事件协议是否 100% 兼容 | **是** | 做兼容 adapter，避免前后端同时大改 |
| D2 | 是否引入新 ID 体系 | **是，第 1 期** | 没有 RunId 的 Runtime 只是搬家 |
| D3 | 物理存储方案 | **先包 repository trait，不换底层格式** | 第 4 期再替换底层 |
| D4 | SubAgent 第一目标 | **child run + cancel + 受限工具集** | 不做后台/恢复 |
| D5 | Python/browser/connector 定位 | **infra adapter** | 前 2 期不碰底层实现 |
| D6 | 切换方式 | **直接替换，不做灰度** | 每期明确 rollback，不设计灰度链路 |

---

## 七、分期概览

### 第 0 期：现状审计与迁移护栏
- 目标：画清 chat.rs 职责、状态拥有者、事件流，建立回归基线
- 产出：职责地图、状态矩阵、事件清单、golden trace 回放脚本
- 详见：[phase-0-baseline-audit.md](./phase-0-baseline-audit.md)

### 第 1 期：身份模型 + TurnState + SessionRuntime + 事件兼容层
- 目标：把 chat.rs 从 God function 变成薄 adapter + Runtime 调用
- 解决约束：C1, C2, C3, C10
- 详见：[phase-1-session-runtime.md](./phase-1-session-runtime.md)

### 第 2 期：Tool Runtime + Permission Pipeline + 最小持久化
- 目标：统一工具系统，独立权限管线，落最小 run/task/tool_call store
- 解决约束：C4, C5, C9
- 详见：[phase-2-tool-permission-store.md](./phase-2-tool-permission-store.md)

### 第 3 期：Task / Agent Runtime（SubAgent 阶段 A + B）
- 目标：task 一等化，sub-agent cancel 真正生效 + 可后台
- 解决约束：C6, C7(A/B), C8
- 详见：[phase-3-task-agent.md](./phase-3-task-agent.md)

### 第 4 期：Store 领域拆分 + Transport 解耦 + SubAgent 阶段 C
- 目标：存储按领域拆，Tauri 彻底降级为 adapter，subagent 可恢复/team
- 解决约束：C7(C)，完成全部分层
- 详见：[phase-4-store-transport-subagent-c.md](./phase-4-store-transport-subagent-c.md)

---

## 八、每期必须包含的结构

每份分期文档都包含以下节：

| 节 | 说明 |
|----|------|
| 本期目标 | 一句话 + 解决哪些约束 |
| 新增文件 | 文件级清单 |
| 迁移文件 | 从哪里移到哪里、改什么 |
| Compatibility Boundary | 哪些接口/事件不能变 |
| Kill List | 本期废掉哪些旧入口 |
| Truth Source | 每个状态谁说了算 |
| Golden Trace | 用什么事件序列验收 |
| Cutover Strategy | 本期如何直接替换旧路径 |
| Rollback Strategy | 切换失败如何回退 |
| Not Doing | 本期明确不做什么 |

---

## 九、不照搬的部分

以下 claude-code-best 的设计**不适用于 lotus-app**，明确排除：

| 不照搬 | 原因 |
|--------|------|
| 超大 CLI 外壳（多入口分发 / feature flag） | lotus-app 是单产品桌面端 |
| tmux/iTerm backend registry | lotus-app 不需要终端 backend |
| MCP server 多实例 | 当前不需要 |
| process.argv 解析链 | Tauri command 已覆盖 |
| Ink/React CLI 渲染 | 前端已是 WebView |

---

## 十、Python Session Scope Migration

将 Python REPL 从 conversation scope 切换到 `RunId` scope 时，不允许把“常驻 Python 进程”继续当作唯一状态真相源。

迁移策略：
- Python REPL 变为 `RunId` scoped
- 跨 run 依赖文件快照、analysis artifact、precompute cache、loaded file manifest 重建上下文
- 不再依赖 conversation 级常驻 Python 进程承载唯一状态

### Conversation-Scoped Python State -> New Recovery Source

| 旧状态 | 新恢复来源 |
|--------|-----------|
| `_df` / `_dfs` / `_text` | analysis artifact / 文件快照 / precompute cache |
| loaded file marker | loaded file manifest |
| checkpoint / restore | run-scoped checkpoint + analysis snapshot |
| precompute 结果 | analysis 目录产物 + precompute cache |
| `loaded:{conversation_id}:*` memory key | 迁移为 session/run 关联的 loaded manifest |

规则：
- step1 完成后，step2 不再依赖 conversation 级 Python 常驻内存，而是从文件快照/analysis snapshot 重建上下文
- `_df/_text/_dfs` 视为可恢复缓存，不是会话真相源
- precompute 结果继续写入 analysis 目录，并由后续 run 按 manifest 读回

---

## 十一、风险与缓解

| 风险 | 缓解 |
|------|------|
| 第 1 期 Runtime 和现有 chat.rs 双栈共存 | 明确 kill list，第 1 期末 chat.rs 的编排逻辑必须全部迁入 Runtime |
| 事件顺序语义漂移 | golden trace 回放验收，TauriEventAdapter 有完整映射测试 |
| Tool 双轨收敛时 regression | 第 2 期前建立 tool regression checklist |
| 前端 streaming 状态假设被打破 | 兼容 adapter 保证前两期事件格式不变 |
| SubAgent 改造范围失控 | 分 3 段演进，每段有明确边界 |
| 持久化迁移数据丢失 | repository trait 先包现有实现，不改底层格式 |
