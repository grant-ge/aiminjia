# TodoWrite 与 AskUserQuestion 工具架构补充设计

日期：2026-04-24

## 背景

当前项目已完成一轮对标 `claude-code-best` 的 runtime 架构收口，但后端 agent 仍缺少两类 Claude Code 新版关键交互工具：

1. `TodoWrite`：让模型在执行复杂任务时维护 session/agent 级任务清单。
2. `AskUserQuestion`：让模型在执行过程中主动向用户提出结构化问题，并等待用户回答后继续。

本次排查结论明确：这两个工具在 lotus-app 中不是“前端未展示”，而是**后端工具、catalog schema、前端展示/交互均完全缺失**。

## 参考信息

### lotus-app 当前状态

- 工具 catalog 权威入口：`src-tauri/src/runtime/tools/catalog.rs`
  - `build_default_catalog()` 未注册 `TodoWrite` / `AskUserQuestion`。
  - `DAILY_ALLOWED_TOOLS` 只包含 `bash/read_workspace_file/write_file/edit_file/list_directory/search_files/get_file_info/grep_content/write_memory/search_memory`。
- RuntimeTool 模块入口：`src-tauri/src/runtime/tools/builtin/mod.rs`
  - 当前仅有 `bash/browse_data/browser/chart/file/grep/memory/network/python/report/switch_skill/workspace` 等模块。
- RuntimeTool trait：`src-tauri/src/runtime/tools/dispatcher.rs`
  - 已支持 `check_permissions()` 返回 `PermissionDecision::Ask`。
  - `ToolDispatchOutcome::AskRequired` 已存在。
- 现有权限 ask 流水线：`src-tauri/src/runtime/chat/chat_turn_driver.rs`
  - `resolve_permission_asks()` 会把 `AskRequired` 写入 pending permission control plane。
  - 同时发出 `RuntimeEventKind::PermissionAskRequired`。
- 事件协议：`src-tauri/src/runtime/events.rs`
  - 目前只有 `PermissionAskRequired`，用于工具权限确认。
- Tauri 事件适配：`src-tauri/src/transport/tauri_event_adapter.rs`
  - `PermissionAskRequired` 被映射为前端 legacy event `permission:ask`。
- 前端工具渲染：`src/hooks/useTurnRenderModel.ts`
  - 当前只把 tool calls/tool results 投影为通用 `RenderToolStep`，展示 `toolName/inputJson/output`。
  - 没有 todo 专属状态，也没有 AskUserQuestion 的交互 UI。
- 前端权限弹窗：`src/components/common/PermissionAskDialog.tsx`
  - 用于“是否允许执行工具”，不是“模型向用户提问并等待回答”。

### claude-code-best 对标信息

- `TodoWriteTool`
  - 参考文件：`/Users/a20250311/github/claude-code-best/src/tools/TodoWriteTool/TodoWriteTool.ts`
  - 工具名：`TodoWrite`
  - 输入：`{ todos: TodoListSchema }`
  - 输出：`{ oldTodos, newTodos, verificationNudgeNeeded? }`
  - 权限：直接 allow。
  - 状态归属：写入 app state 的 `todos[todoKey]`，`todoKey = agentId ?? sessionId`。
- `AskUserQuestionTool`
  - 参考文件：`/Users/a20250311/github/claude-code-best/src/tools/AskUserQuestionTool/AskUserQuestionTool.tsx`
  - 工具名：`AskUserQuestion`
  - 输入：`{ questions: Question[], metadata? }`
  - 每个 question 包含 `question/header/options/multiSelect`，option 可带 `label/description/preview`。
  - `requiresUserInteraction() = true`。
  - `checkPermissions()` 返回 ask，UI 收集答案后，tool result 注入给模型继续推理。

## 目标

补齐两类交互工具，并保持 lotus-app 现有架构约束：

1. 新工具必须走 `RuntimeTool`，不得新增 `ToolPlugin`。
2. 工具元数据与 schema 进入 `ToolCatalog`，不在 prompt 或 legacy tools 中旁路定义。
3. Runtime 层不得依赖 `tauri::*`，所有 UI 交互通过 `RuntimeEvent` + transport adapter + command control plane 完成。
4. 状态归属要清晰：Todo 状态属于 session/agent 运行态；AskUserQuestion 的 pending 交互属于 turn 运行态。
5. 前端显示与交互必须从事件/消息数据驱动，不把业务逻辑塞进通用消息渲染组件。

## 非目标

- 不在本阶段实现 Claude Code 完整 TaskCreate/TaskUpdate/TaskList 多工具体系。
- 不引入新的 prompt-only 修复。
- 不把 AskUserQuestion 伪装成权限确认；它是用户交互工具，不是安全权限工具。
- 不重构所有现有 permission pipeline，只在必要边界上抽象出可复用的 pending interaction 模型。

---

## 方案一：最小增量接入

### 概述

方案一以最小改动补齐两个工具，尽量复用现有 `PermissionAskRequired`/pending permission control plane 流水线。

适合目标：快速让 agent 可调用 `TodoWrite` 与 `AskUserQuestion`，用于 Codex 分步实现与短期产品验证。

### 后端设计

#### TodoWrite

新增 `src-tauri/src/runtime/tools/builtin/todo.rs`，实现 `RuntimeTool`：

- `definition().id = "TodoWrite"`
- `is_read_only = false`
- `is_concurrency_safe = false`
- `check_permissions()` 返回 `PermissionDecision::Allow`
- `execute()`：
  - 解析输入 `{ todos: TodoItem[] }`
  - `TodoItem` 字段建议最小集：
    - `content: string`
    - `status: "pending" | "in_progress" | "completed"`
    - `activeForm?: string`
  - 根据 `ctx.agent_id.unwrap_or(ctx.session_id)` 作为 todo key。
  - 写入 session/agent 级 todo state。
  - 返回 `ToolResult`：
    - content: `Todos have been modified successfully...`
    - data: `{ oldTodos, newTodos }`

状态存储可以先采用 session runtime 内存态：

- 在 session 运行态中新增 `TodoStateStore` 或 `SessionTodoStore`。
- 以 `SessionId + AgentId?` 为 key。
- 本阶段不强制持久化到 `~/.renlijia/`，因为 Claude Code 的 todo 本质是当前执行上下文的工作清单，不是长期任务管理。

同时扩展事件：

```rust
RuntimeEventKind::TodoListUpdated {
    owner_agent_id: Option<AgentId>,
    todos: Vec<TodoItem>,
}
```

transport adapter 映射为：

```text
todo:updated
```

前端用它更新当前会话的 todo 展示。

#### AskUserQuestion

最小方案复用现有 `PermissionDecision::Ask` 机制，但增加一种 ask payload。

新增工具：`src-tauri/src/runtime/tools/builtin/ask_user_question.rs`

- `definition().id = "AskUserQuestion"`
- `is_read_only = true`
- `is_concurrency_safe = true`
- `check_permissions()` 返回 `PermissionDecision::Ask`
- ask 的 message 可固定为 `Answer questions?`
- `execute()` 在用户回答后执行，返回 data：`{ questions, answers, annotations? }`

为了传递结构化问题，需要扩展 `PermissionDecision::Ask` 或新增字段：

```rust
interaction_payload: Option<serde_json::Value>
```

当工具是 `AskUserQuestion` 时，payload 放入完整 questions：

```json
{
  "kind": "askUserQuestion",
  "questions": [...],
  "metadata": {...}
}
```

现有 `RuntimeEventKind::PermissionAskRequired` 也增加该 payload 字段，然后 `permission:ask` 事件携带给前端。

用户提交答案仍复用现有 permission resolution command，但需要允许 resolution 携带 `updated_input` 或 `answers`：

```rust
PendingPermissionResolution::Allow {
    updated_input: Option<Value>,
    remember: bool,
    destination: Option<PermissionDestination>,
}
```

TurnDriver replay 原工具时，把 `answers` 合并进原始 input，然后设置 `permission_override=Allow`，避免再次 ask。

### 前端设计

#### Todo 展示

新增轻量 store：

- `src/stores/todoStore.ts`
- key: `conversationId + ownerAgentId?`
- value: `TodoItem[]`

新增 UI：

- `src/components/chat-scene/TodoListCard.tsx`
- 显示当前 turn/session 的任务清单。
- 状态映射：
  - pending：未开始
  - in_progress：进行中
  - completed：完成

事件接入：

- 在现有 Tauri event hook 中订阅 `todo:updated`。
- 收到后更新 todo store。

#### AskUserQuestion UI

复用现有 `PermissionAskDialog` 的弹窗宿主，但拆出一种“交互问答模式”：

- 如果 `permission:ask` payload 中 `interactionPayload.kind === "askUserQuestion"`：
  - 渲染 `AskUserQuestionDialog`
  - 每个 question 渲染 2-4 个 options
  - 支持 `multiSelect`
  - 默认提供“Other”输入
  - 用户提交后调用 approve command，附带 answers
- 否则继续走原权限确认 UI。

### 优点

- 改动小，利用现有 pending permission control plane。
- Codex 可按小步实现：先 TodoWrite，再 AskUserQuestion。
- 不需要先重构整个交互系统。

### 缺点

- AskUserQuestion 与权限确认共享 `permission:ask`，语义混杂。
- `PermissionDecision::Ask` 会被扩展成同时承载“安全权限”和“用户问答”，长期看边界不干净。
- 前端需要在权限弹窗入口里做 kind 分流。

### 适用判断

如果当前目标是快速补齐 agent 能力，并接受后续再做架构收口，推荐先执行方案一。

---

## 方案二：Interaction Runtime 一等抽象

### 概述

方案二将“需要用户参与的工具执行”从 permission pipeline 中抽出来，建立独立的 Interaction Runtime。

适合目标：继续严格对标 claude-code-best 的架构哲学，把 AskUserQuestion 作为一等交互工具，而不是挂靠权限确认。

### 核心设计

新增 runtime interaction 层：

```text
ToolDispatcher
  → RuntimeTool.execute/check_interaction
  → ToolDispatchOutcome::InteractionRequired
  → ChatTurnDriver pending interaction control plane
  → RuntimeEventKind::UserInteractionRequired
  → TauriEventAdapter
  → frontend dialog/form
  → resolve_user_interaction command
  → replay/continue tool call
```

### 后端设计

#### 新增类型

新增 `src-tauri/src/runtime/interaction/`：

- `types.rs`
  - `InteractionId`
  - `InteractionKind`
  - `InteractionRequest`
  - `InteractionResolution`
- `control_plane.rs`
  - `PendingInteractionControlPlane`
  - `insert_pending_interaction()`
  - `resolve_pending_interaction()`
- `mod.rs`

`InteractionKind` 初始支持：

```rust
pub enum InteractionKind {
    AskUserQuestion,
}
```

`InteractionRequest`：

```rust
pub struct InteractionRequest {
    pub interaction_id: InteractionId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub kind: InteractionKind,
    pub payload: Value,
    pub original_request: RuntimeToolCallRequest,
}
```

`InteractionResolution`：

```rust
pub enum InteractionResolution {
    Submit { value: Value },
    Cancel { message: String },
}
```

#### ToolDispatcher 扩展

新增 outcome：

```rust
ToolDispatchOutcome::InteractionRequired(InteractionRequest)
```

或者在 `ToolError` 中新增：

```rust
ToolError::InteractionRequired(InteractionRequest)
```

推荐 outcome 方式，和现有 `AskRequired` 平级，避免把正常控制流表达为 error。

#### RuntimeTool 接口扩展

最小扩展：不改 trait，只允许 `AskUserQuestionTool.execute()` 返回 `ToolError::InteractionRequired`。

更干净扩展：给 trait 增加可选方法：

```rust
fn requires_user_interaction(&self, input: &Value) -> Option<InteractionRequestSpec> {
    None
}
```

推荐先用最小扩展，减少破坏面。

#### AskUserQuestion 实现

`AskUserQuestionTool` 首次执行时：

- 校验 questions schema。
- 构造 `InteractionRequest`，payload 为 `{ questions, metadata }`。
- 返回 `InteractionRequired`。

TurnDriver 收到后：

- 写入 `PendingInteractionControlPlane`。
- 发 `RuntimeEventKind::UserInteractionRequired`。
- 等待用户 resolution。
- 如果 Submit：把 `answers/annotations` 合并回 input，再 replay 原 tool call。
- 如果 Cancel：生成 tool_result，告诉模型用户取消回答。

replay 时通过 `ToolExecutionContext` 增加：

```rust
pub interaction_resolution: Option<Value>
```

AskUserQuestionTool 检测到 resolution 后返回最终 ToolResult：

```json
{
  "questions": [...],
  "answers": {...},
  "annotations": {...}
}
```

#### TodoWrite 实现

TodoWrite 不需要 interaction runtime，和方案一相同。

但方案二建议将 todo 状态也纳入明确 state ownership：

- `runtime/todo/types.rs`
- `runtime/todo/store.rs`
- `TodoOwner = Session(SessionId) | Agent(SessionId, AgentId)`
- `SessionTodoStore` 由 `QueryEngine` 或 `SessionRuntime` 持有。

TodoWriteTool 通过 `ToolExecutionContext` 获取一个窄能力：

```rust
pub todo_store: Option<Arc<dyn TodoStore>>
```

如果不想扩大 `CapabilityContext`，可把它放在 `ToolExecutionContext`，因为它是运行态 per-call context，不是外部系统能力。

#### 事件协议

新增 runtime event：

```rust
RuntimeEventKind::TodoListUpdated {
    owner_agent_id: Option<AgentId>,
    todos: Vec<TodoItem>,
}

RuntimeEventKind::UserInteractionRequired {
    interaction_id: InteractionId,
    tool_call_id: ToolCallId,
    tool_name: String,
    kind: InteractionKind,
    payload: Value,
}

RuntimeEventKind::UserInteractionResolved {
    interaction_id: InteractionId,
}
```

Tauri legacy event：

```text
todo:updated
interaction:required
interaction:resolved
```

新增 commands：

```rust
resolve_user_interaction(interaction_id, value)
cancel_user_interaction(interaction_id, message?)
```

### 前端设计

#### 交互 store

新增：

- `src/stores/interactionStore.ts`

状态：

```ts
type PendingInteraction = {
  interactionId: string
  conversationId: string
  runId: string
  toolCallId: string
  toolName: string
  kind: 'askUserQuestion'
  payload: unknown
}
```

事件处理：

- `interaction:required` → push pending interaction
- `interaction:resolved` → remove

#### AskUserQuestionDialog

新增：

- `src/components/interactions/AskUserQuestionDialog.tsx`

职责：

- 只处理 `kind=askUserQuestion`。
- 渲染 questions。
- 支持单选、多选、Other。
- 可选 preview 字段先按纯文本/markdown 预览，不执行 HTML。
- submit 调 `resolve_user_interaction`。
- cancel 调 `cancel_user_interaction`。

#### Todo UI

同方案一，但状态 store 建议放到：

- `src/stores/todoStore.ts`
- `src/components/chat-scene/TodoListCard.tsx`

### 优点

- 架构语义最清晰：权限确认与用户问答彻底分离。
- 后续可扩展更多交互工具，例如文件选择、表单填写、确认计划、人工审批等。
- 更符合 `claude-code-best` 中 AskUserQuestion “requires user interaction”的建模。
- 不污染 permission pipeline。

### 缺点

- 改动更大。
- 需要新增 pending interaction control plane、事件、commands、前端 store。
- Codex 执行时需要更严格分阶段，否则容易把 interaction 与 permission 两套机制混在一起。

### 适用判断

如果目标是做一次长期架构补齐，推荐方案二。

---

## 推荐

推荐采用**方案二：Interaction Runtime 一等抽象**。

原因：

1. 当前项目已经有明确架构约束：runtime 层 transport-neutral、工具走 RuntimeTool、事件协议桥接前端。AskUserQuestion 本质是工具执行中的用户交互，不是权限安全决策，应该有独立边界。
2. 现有 permission pipeline 已承担安全边界职责。如果把业务问答塞进 `PermissionDecision::Ask`，短期快，但会让“是否允许执行工具”和“用户如何回答模型问题”共享同一套类型、事件和 UI，后续维护成本高。
3. `TodoWrite` 与 `AskUserQuestion` 的 state ownership 不同。Todo 是 session/agent 状态，Ask 是 turn 内 pending interaction。分开建模可以避免继续扩大 `CapabilityContext` 或前端通用工具渲染组件。
4. 方案二仍可分阶段执行：先实现 TodoWrite，再实现 Interaction Runtime，再接 AskUserQuestion。

如果需要压缩执行范围，可以采用“方案二架构，方案一节奏”：

- Phase 1：只做 TodoWrite 后端 + todo:updated + 前端展示。
- Phase 2：新增 Interaction Runtime 空框架和事件/commands。
- Phase 3：接入 AskUserQuestion。
- Phase 4：补 tests 与前端交互回归。

## Codex 执行建议

给 Codex 的执行顺序建议如下：

1. 阅读本文档与以下文件：
   - `src-tauri/src/runtime/tools/catalog.rs`
   - `src-tauri/src/runtime/tools/dispatcher.rs`
   - `src-tauri/src/runtime/chat/chat_turn_driver.rs`
   - `src-tauri/src/runtime/events.rs`
   - `src-tauri/src/transport/tauri_event_adapter.rs`
   - `src/hooks/useTurnRenderModel.ts`
   - `src/components/common/PermissionAskDialog.tsx`
2. 实现 `TodoWrite`，不要先碰 AskUserQuestion。
3. 为 TodoWrite 添加 Rust tests：
   - schema 注册存在。
   - DAILY_ALLOWED_TOOLS 包含工具。
   - execute 后按 session/agent key 更新 todos。
   - all completed 时按设计清空或保留展示状态，需要在实现前二选一并保持一致。
4. 实现 todo 前端事件与展示。
5. 新增 Interaction Runtime，不复用 `permission:ask`。
6. 实现 AskUserQuestion 后端 pending/resolve/replay。
7. 实现 AskUserQuestion 前端 dialog。
8. 跑验证：
   - `pnpm test`
   - `pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts`
   - `cd src-tauri && cargo test review_ --tests --no-fail-fast`
   - 相关新增 Rust test 串行执行，避免 cargo artifact lock 阻塞。

## 设计自查

- 无 TBD/TODO 占位。
- Todo 与 AskUserQuestion 分属不同状态域：session/agent todo state 与 turn pending interaction。
- 方案一与方案二边界清晰，推荐方案明确。
- 没有要求通过 prompt 修复能力缺失。
- 没有新增 legacy ToolPlugin。
- 没有让 runtime 层依赖 Tauri。
