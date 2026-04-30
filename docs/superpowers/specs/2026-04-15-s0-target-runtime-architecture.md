# S0：lotus-app 目标 runtime 架构定义

**目的**：在动手修任何边界之前，先定义 canonical runtime path、state ownership model、permission result model、cancel model 和 tool execution context 的目标形态。后续 S1-S3 的每一步都是"让现实向这个定义靠拢"，而不是"修观察到的问题"。

**对标**：`claude-code-best`（`/Users/a20250311/github/claude-code-best`）

---

## 一、Canonical Runtime Path

### 目标主路径

```
Transport (Tauri IPC / REST / CLI)
  ↓  只做参数接收、序列化/反序列化，不含业务逻辑
Session (SessionRuntime)
  ↓  owns: messages, file_cache, usage, permission_denials, abort_controller
  ↓  per-turn: execute_turn()
Turn (TurnDriver)
  ↓  owns: turn_state (messages snapshot, turn_count, transition)
  ↓  iterative: stream LLM → collect tool_calls → dispatch → feed results → loop
Tool Dispatch (ToolDispatcher)
  ↓  partition: concurrent-safe vs serial
  ↓  per-call: build ExecutionContext → permission check → execute → collect result
Permission Pipeline
  ↓  single pipeline for ALL tool calls, regardless of entry point
  ↓  decision: Allow / Deny / Ask (三态，不是二态)
Tool.execute(input, ctx: ExecutionContext) → ToolResult
  ↓
State / Persistence (ConversationStore, PermissionStore)
```

### 唯一 owner 定义

| 层 | Owner | 职责边界 |
|---|---|---|
| Transport | `TauriChatCommandAdapter` | 参数接收 → 转发 Session，不持有业务状态 |
| Session | `SessionRuntime` | messages、abort_controller、usage、permission_denials、file_cache |
| Turn | `TurnDriver` | 单轮 LLM ↔ Tool 迭代循环、turn_state（immutable update per iteration）|
| Tool Dispatch | `ToolDispatcher` | 工具查找、权限裁决、并发编排 |
| Permission | `PermissionPipeline` | 唯一权限裁决入口——所有路径都经过这里 |
| Tool | `RuntimeTool` | 单工具执行，只接收 `ExecutionContext`（per-call 窄上下文）|
| State | `ConversationStore` + `PermissionStore` | 持久化真相源 |

### 不应存在的路径

- `PluginContext` 作为工具的上下文载体（应用 `ExecutionContext` 替代）
- `registry.execute()` 作为独立入口绕过 Session/Turn（应通过 ToolDispatcher）
- `legacy_send_message_impl` 内部自建 QueryEngine/EventBus/TurnState（应由 Session 提供）
- 任何入口的 `allow_all()` 权限 bypass

---

## 二、State Ownership Model

对标 claude-code-best 的三层 state ownership：

### Session State（SessionRuntime owns）

```rust
pub struct SessionState {
    messages: Vec<Message>,              // 持久跨 turn
    abort_controller: CancellationToken, // session 生命周期
    total_usage: Usage,                  // 累计
    permission_denials: Vec<PermissionDenial>,
    read_file_cache: FileStateCache,
    discovered_skills: HashSet<String>,  // per-turn 重置
}
```

### Turn State（TurnDriver owns，per-iteration immutable update）

```rust
pub struct TurnState {
    identity: RuntimeIdentity,
    agent_id: Option<AgentId>,           // sub-agent identity（一等运行时对象）
    messages: Vec<Message>,              // snapshot from session + this turn
    turn_count: u32,
    transition: Option<Transition>,      // 上一 iteration 的 continue 原因
    cancellation: CancellationToken,     // child of session abort_controller
    pending_tool_calls: Vec<ToolCallState>,
}
```

TurnState 持有的是 **turn-scoped** 的信息。per-call 的 `ExecutionContext` 由 ToolDispatcher 在 dispatch 时按需构造（见第五节），不存储在 TurnState 上。

TurnState 提供 context builder 以便 ToolDispatcher 构造 per-call context：

```rust
impl TurnState {
    /// 为单次工具调用构造 ExecutionContext
    pub fn build_execution_context(
        &self,
        tool_call_id: ToolCallId,
        capability: CapabilityContext,
    ) -> ExecutionContext {
        ExecutionContext {
            session_id: self.identity.session_id().clone(),
            run_id: self.identity.run_id().clone(),
            agent_id: self.agent_id.clone(),  // 透传 agent identity
            tool_call_id,
            cancellation: self.cancellation.child_token(),
            capability,
            event_sink: EventSink::default(),
        }
    }
}
```

**`build_execution_context()` 约束**：
- 只构造 call-independent defaults + call-scoped child token + agent identity 透传
- 禁止在 builder 中塞入 message snapshot、app_state handle、storage raw deps
- 如果工具需要这些信息，必须通过 `CapabilityContext` 上的受控 trait accessor 暴露

### App-wide State（thin reactive store）

```rust
pub struct AppState {
    permission_context: PermissionContext,
    settings: AppSettings,
    model: Option<String>,
    tasks: HashMap<String, TaskState>,
    // ... 其他跨 session 共享状态
}

// Store 是薄壳——getState/setState/subscribe
// domain logic 在外部，不在 store 实现里
```

### 关键约束

- **Subagent 隔离**：子 agent 的 `set_app_state` 是 no-op；file_cache 共享读、隔离写
- **Turn state 不可变更新**：每次 iteration 创建新 TurnState，不 in-place 修改
- **State ownership 不是后置优化**：permission_denial、in-flight tool_call、cancel status、message payload、generatedFiles 这些状态从一开始就归到上述三层中的明确 owner

---

## 三、Permission Result Model（含 Ask）

对标 claude-code-best，permission 是**三态**，不是二态：

### 决策类型

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

### 决策原因

```rust
pub enum PermissionReason {
    Rule(PermissionRule),
    Mode(PermissionMode),
    Hook { name: String, source: Option<String> },
    Classifier { name: String, reason: String },
    SafetyCheck { reason: String, classifier_approvable: bool },
    StoredPolicy,
    UnknownScope,
    Other(String),
}
```

### Pipeline（单一入口）

```rust
pub trait PermissionPipeline: Send + Sync {
    /// 所有工具调用的唯一权限裁决入口
    async fn authorize(
        &self,
        tool: &dyn RuntimeTool,
        input: &Value,
        ctx: &ExecutionContext,
    ) -> PermissionDecision;
}
```

**关键**：`authorize()` 返回 `PermissionDecision`（三态），不是 `Result<()>`。调用方必须处理 `Ask` 变体——不能把 Ask 当成 Deny。

### Ask 流程（后端契约，UI 可延后实现）

1. Pipeline 返回 `Ask { message, suggestions }`
2. TurnDriver 收到 Ask 后：
   - 如果处于 `auto` mode：运行 classifier 或 auto-deny
   - 如果处于 `interactive` mode：通过 `RuntimeEvent::PermissionAsk` 发给前端
   - 如果处于 `dontAsk` mode：转为 Deny
3. 前端（S6 实现）展示对话框，用户选择 allow/deny/always-allow/always-deny
4. 决策回传后端，持久化到 `PermissionStore`

**S1 就应该用三态接口**，即使 S1 暂时把所有 Ask 转为 Deny。这样后续加 Ask UI 时不需要重塑接口。

---

## 四、Cancel Model

对标 claude-code-best 的 hierarchical AbortController：

### 层级 CancellationToken

```rust
impl CancellationToken {
    pub fn new() -> Self { ... }

    /// 创建 child token——parent cancel 传播到 child，child cancel 不影响 parent
    pub fn child_token(&self) -> CancellationToken { ... }

    pub fn cancel(&self) { ... }
    pub fn is_cancelled(&self) -> bool { ... }
}
```

### 传播路径

```
Session.abort_controller （用户取消整个会话）
  ├── Turn.cancellation = session.abort_controller.child_token()
  │     ├── LLM stream 检查 cancellation
  │     ├── Tool dispatch 检查 cancellation
  │     │     ├── tool_call_1.cancellation = turn.cancellation.child_token()
  │     │     ├── tool_call_2.cancellation = turn.cancellation.child_token()
  │     │     └── ...
  │     └── Python subprocess 接收 cancellation
  └── (其他 session-scoped background task)
```

### 关键约束

- **禁止在生产路径 `CancellationToken::new()`**：所有 token 必须来自 parent token 的 `child_token()`
- **Session cancel → 所有 turn/tool/subprocess 都中断**
- **单个 tool cancel → 不影响同 turn 的其他 tool**
- 取消后的状态清理：pending tool_call 标记为 cancelled，synthetic tool_result 注入 messages
- **cancel cascade 必须是事件驱动的 shared-state 树形传播**（shared inner + weak child registry），禁止"一 child 一 OS 线程轮询"实现

---

## 五、Tool Execution Context（per-call 窄上下文）

对标 claude-code-best 的 `ToolUseContext`，但精简为 Rust 所需：

### ExecutionContext（替代 PluginContext）

```rust
pub struct ExecutionContext {
    // Identity
    pub session_id: SessionId,
    pub run_id: RunId,
    pub agent_id: Option<AgentId>,
    pub tool_call_id: ToolCallId,

    // Cancellation（call-scoped child token，由 TurnState 派生）
    pub cancellation: CancellationToken,

    // Capability（narrow service access——工具只能通过这里访问系统能力）
    pub capability: CapabilityContext,

    // Event sink（per-call 事件收集）
    pub event_sink: EventSink,
}
```

**关键约束**：

- ExecutionContext **不暴露** `Arc<RwLock<AppState>>`、`Arc<Vec<Message>>` 或 `Arc<FileStateCache>>`
- 工具如需读取 messages 或 file cache，通过 `CapabilityContext` 上的受控 accessor：
  - `capability.read_file(path)` — 不是直接拿到整个 cache
  - `capability.workspace_path()` — 不是直接拿到 storage 对象
- 工具如需写状态（如生成文件），通过 `ToolResult` 返回值声明（`file_meta`、`generated_files`），由 TurnDriver 统一处理
- 这确保工具无法随手读写全局状态，消除 service locator 的核心问题

### CapabilityContext（受控能力访问，不是原始依赖暴露）

```rust
/// 工具通过 CapabilityContext 访问系统能力。
/// 每个 accessor 暴露的是受控操作，不是底层对象。
/// 新增 accessor 必须论证为什么工具需要这个能力。
pub struct CapabilityContext {
    // Workspace / storage
    pub storage: Option<StorageCapability>,
    pub workspace_id: Option<String>,

    // File operations（受控 accessor，不是 Arc<FileManager>）
    pub file_ops: Option<Arc<dyn FileOperations>>,

    // Python session
    pub python_available: bool,
    pub python_session: Option<Arc<dyn PythonExecution>>,

    // Browser
    pub browser_available: bool,
}

/// 受控文件操作 trait——工具只能通过这个接口访问文件系统
#[async_trait]
pub trait FileOperations: Send + Sync {
    /// 加载文件内容（负责 loaded key 管理、parser、masking）
    async fn load_file(&self, file_id: &str, scope_id: &str) -> Result<LoadedFile>;
    /// 检查文件是否已加载
    fn is_loaded(&self, file_id: &str, scope_id: &str) -> bool;
    /// 获取 workspace 路径
    fn workspace_path(&self) -> &Path;
}

/// 受控 Python 执行 trait
#[async_trait]
pub trait PythonExecution: Send + Sync {
    async fn execute_code(
        &self,
        code: &str,
        run_id: &RunId,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<String>;
}
```

**关键约束**：
- CapabilityContext 暴露的是 **trait object accessor**，不是底层 `Arc<AppStorage>` 或 `Arc<FileManager>`
- 工具不能绕过 accessor 直接操作文件系统或全局状态
- 新增 accessor 必须有明确的 capability scope 对应关系
- `FileOperations` 和 `PythonExecution` 的具体实现放在 infra 层，不在 runtime 层

### RuntimeTool trait（已有，确认接口）

```rust
#[async_trait]
pub trait RuntimeTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    async fn execute(
        &self,
        input: Value,
        ctx: ExecutionContext,
    ) -> Result<ToolResult, ToolError>;

    fn is_concurrency_safe(&self, input: &Value) -> bool { false }
    fn is_read_only(&self, input: &Value) -> bool { false }
}
```

### 关键约束

- **ExecutionContext 是 per-call 的**：每次工具调用构造新的，不复用
- **不允许工具直接访问 PluginContext**：所有需要的能力通过 CapabilityContext 暴露
- **Subagent 的 app_state 写入是 no-op**
- **LegacyToolAdapter 是过渡层**：它把 ExecutionContext 桥接回 PluginContext，但新工具不应走这条路

---

## 六、当前架构到目标架构的差距矩阵

| 维度 | 目标 | 当前 | 差距 |
|------|------|------|------|
| Canonical path | Session → Turn → ToolDispatcher → Permission → Tool | Transport 层的 `legacy_send_message_impl` 仍是编排 owner | **大** |
| Permission model | 三态（Allow/Deny/Ask），单一入口 | 二态（Allow/Deny），多入口（部分 allow_all bypass） | **中** |
| Cancel model | 层级 child_token cascade | 多处 `CancellationToken::new()`，fire-and-forget | **中** |
| Tool context | per-call ExecutionContext | PluginContext service locator（36 文件、42 签名） | **大** |
| State ownership | 三层明确 owner | TurnState 薄壳、file_meta/generatedFiles 在局部变量、synthetic event | **中** |
| Turn state | immutable update per iteration | agent_loop 3900 行共享可变状态 | **大** |

---

## 七、S1-S3 重新定义（为 canonical path 服务）

基于以上目标架构，S1-S3 的每一步都应该是"让某个维度向 canonical model 靠拢"：

### S1：权限模型升级到三态 + 单一入口

- 把 `PermissionPipeline::authorize()` 返回值从 `Result<()>` 改为 `PermissionDecision`（三态）
- `Allow` 变体必须携带 `updated_input: Option<Value>`——允许 pipeline 在放行时修改工具入参
- 消除 `allow_all()` bypass
- 所有入口统一到同一个 pipeline
- `ToolDispatcher.dispatch()` 对 Ask 返回 `ToolDispatchOutcome::AskRequired`，Ask 语义由 TurnDriver 按 mode 决定处理，不在 Dispatcher 压扁
- Ask 在 S1 由 TurnDriver 暂转 Deny（后端契约已就位，UI 在 S6 做）

### S2：Cancel model 改为 child_token cascade

- `CancellationToken` 新增 `child_token()` 方法
- Session 持有 root token
- Turn 使用 `session.cancel.child_token()`
- Tool dispatch 使用 `turn.cancel.child_token()`
- 禁止生产路径 `CancellationToken::new()`

### S3：高价值工具迁到 ExecutionContext

- **S3 canonical migration target**：`load_file` + precompute auto-load 改为接收 `ExecutionContext`
- `execute_python` 在 S3 只做 dependency inventory + boundary definition，不做 runtime-native migration（留到 S4）
- 验收：不仅验代码形状，还验运行时语义（loaded key、file_meta/generatedFiles/degradation 透传、cancel cascade、permission pipeline 一致性）

---

## 八、一句话结论

> **先定义目标架构的五个维度（canonical path / state ownership / permission 三态 / cancel cascade / per-call context），然后让每一期改动都是"缩小现实与目标的差距"，而不是"修观察到的问题"。**
