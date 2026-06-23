# Subagent 对标改造综合设计（三轨：P / R / A）

**日期：** 2026-04-20  
**对标来源：** `claude-code-best` subagent 架构  
**接棒自：** Plan-H（H5/H6/H7 遗留）、Plan-U2（全 `[ ]`）、Plan-U5（已完成）  
**状态：** 设计确认，待落实施计划

---

## 背景与三大差距

通过两个项目的并行 agent 调研，确认 lotus-app 与 claude-code-best 在 subagent 架构上存在三大差距：

| 差距 | 当前状态 | 目标状态 |
|---|---|---|
| **Permission 隔离** | 子 agent 共享父 permission_store；AskRequired 被降级为 deny/error | 独立 permissionMode 上下文；Ask 结构化冒泡并在前端完整路由 |
| **声明式 Agent 注册** | 工具池白名单硬编码在 internal_system.rs；无注册机制 | AgentDefinition + AgentRegistry；browse_data/daily_assistant 迁移为声明式 |
| **Async 独立性** | background subagent cancel_token 有时为 None；lifecycle 状态机不完整 | background agent 独立 CancellationToken；lifecycle 完整状态机 + 事件 |

---

## 总体架构

### 分轨与依赖

```
P-1 (PermissionStore 三层)
  └── P-2 (WorkerRunConfig.permission_mode 独立上下文)
        └── P-3 (Ask 前端闭环 ← 接 H6)

R-1 (AgentDefinition + AgentRegistry)
  └── R-2 (browse_data + daily_assistant 迁移)
        └── R-3 (list_agents command)
              └── R-4 (前端 Agent 选择器)

A-1 (background cancel 独立 token ← 接 H5)
  └── A-2 (lifecycle 状态机收口)
        └── A-3 (回归测试 + review lock)
```

**并行启动点：P-1、R-1、A-1 三个无依赖，可同时开工。**  
R-2 依赖 P-1（注册的 agent 需要 permission 上下文结构存在）。

### 不纳入本期范围

- subagent 用户自定义（`.claude/agents/*.md` 前端 CRUD）
- subagent 独立 model 配置（本期全部 `inherit`）
- worktree 隔离、远程 worker、多模型编排
- 企业权限托管、云端规则同步

---

## P 轨：Permission 隔离 + Ask 闭环

### P-1：PermissionStore 三层规则

**当前问题：** `permission_store.rs` 只有扁平 `tool:scope -> PolicyDecision`，无规则来源分层。

**目标结构：**

```rust
pub enum PermissionSource {
    Session,    // 本次对话内生效，退出即清空
    Workspace,  // ~/.renlijia/workspace/permissions.json
    User,       // ~/.renlijia/user/permissions.json
}

pub struct PermissionRule {
    pub tool_name: String,
    pub scope: String,
    pub path_glob: Option<String>,        // 文件类工具路径匹配
    pub command_pattern: Option<String>,  // Bash/命令类工具匹配
    pub decision: PolicyDecision,
    pub source: PermissionSource,
}
```

优先级：`Session > Workspace > User`，未命中任何规则则走 Ask。

**迁移策略：** 旧的扁平格式通过 `fallback_read()` 兼容读取，新写入全部走分层结构。

**修改文件：**
- `src-tauri/src/runtime/store/permission_store.rs`
- `src-tauri/src/runtime/tools/permission.rs`（`StorePolicyPipeline` 适配新结构）

### P-2：Subagent 独立 permissionMode 上下文

**当前问题：** subagent 通过共享的 `ToolRegistry` 引用间接共享父的 `permission_store`，无独立 mode 上下文。

**目标：** `WorkerRunConfig` 新增 `permission_mode` 字段，subagent 执行时用自己的 mode 决策，不污染父 session。

```rust
pub struct WorkerRunConfig {
    pub allowed_tools: Vec<String>,
    pub conversation_id: String,
    pub parent_run_id: Option<RunId>,
    pub background: bool,
    pub app_handle: Option<tauri::AppHandle>,
    pub cancel_token: Option<CancellationToken>,
    pub permission_mode: PermissionMode,  // 新增
}
```

**Mode 赋值规则（代码决定，不暴露给用户配置）：**

| 场景 | permission_mode 值 |
|---|---|
| `background = false`（前台 subagent） | `inherit`（从父 session 取当前 mode） |
| `background = true`（后台 subagent） | `dontAsk`（不弹交互框，未知 scope 自动拒绝） |

**PermissionMode 完整定义（U2-2 遗留）：**

当前 `PermissionMode` 只有 `Default / DontAsk`，本期扩展为三种：

```rust
pub enum PermissionMode {
    Default,   // 正常流程：命中规则走规则，未命中走 Ask
    Plan,      // 只读模式：所有写操作自动 Deny，不弹 Ask
    DontAsk,   // 静默模式：未命中规则自动 Deny，不弹 Ask
}
```

`apply_permission_mode()` 改为规则表驱动，不再是单点 `dontAsk => deny`：

| mode | 命中 Allow 规则 | 命中 Deny 规则 | 未命中 | 写操作 |
|---|---|---|---|---|
| Default | Allow | Deny | Ask | Ask |
| Plan | Allow | Deny | Deny | Deny |
| DontAsk | Allow | Deny | Deny | Deny |

**修改文件：**
- `src-tauri/src/runtime/agent/worker_runtime.rs`（新增 permission_mode 字段 + 赋值逻辑）
- `src-tauri/src/llm/sub_agent.rs`（SubAgentConfig 传递 permission_mode）
- `src-tauri/src/llm/tool_executor/internal_system.rs`（构造 WorkerRunConfig 时按 background 赋值）
- `src-tauri/src/runtime/tools/permission.rs`（PermissionMode 扩展 + apply_permission_mode 规则表）

### P-3：Ask 前端闭环（接 H6）

**当前问题（H6 遗留）：**
- `worker_runtime.rs` 里 AskRequired 被 `annotate_subagent_ask_decision()` 标注后冒泡为 `LegacyToolError::AskRequired`
- 但父 run 的 `chat_turn_driver.rs` 处理 AskRequired 时缺少 `pending_permission_control_plane`（`FIXME(S6)` 注释）
- 最终降级成 deny/error 发回前端

**目标状态：**

```
子 agent 工具触发 AskRequired
  → worker_runtime.rs 捕获，annotate 后 break loop
  → LegacyToolError::AskRequired 冒泡到父 run
  → chat_turn_driver.rs 通过 pending_permission_control_plane 路由
  → RuntimeEvent::PermissionAskRequired 推到前端
  → 前端弹框：允许一次 / 记住到 Workspace / 记住到 User / 拒绝
  → approve_permission_request(remember, destination)
  → 写入 P-1 的对应规则层
  → 重放原始工具调用
```

**前端 PermissionAskDialog 扩展：**

当前只有"允许 / 拒绝 / 关闭"，扩展为：
- 允许一次（不记住）
- 记住到工作区（写 Workspace 层）
- 记住到用户级（写 User 层）
- 拒绝

**修改文件：**
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`（补全 pending_permission_control_plane）
- `src-tauri/src/runtime/tools/permission.rs`（approve 接口增加 remember + destination 参数）
- `src/components/common/PermissionAskDialog.tsx`（扩展操作选项）
- `src/stores/streamingStore.ts`（pending ask payload 增加 destination options）

### P-4：Bash / 文件 / MCP 匹配维度补齐（U2-4）

**当前遗漏：** 规则只能按 `tool_name + scope` 判定，无法表达“只允许某路径”或“只允许某命令模式”。

**目标：**
- Bash 工具支持 `command_pattern` 匹配（正则或前缀匹配，按安全策略选一）
- 文件类工具支持 `path_glob` 匹配（例如 `workspace/uploads/**`）
- MCP 工具继续走统一 permission pipeline，不加 special-case bypass

```rust
pub struct PermissionMatch {
    pub tool_name: String,
    pub scope: String,
    pub path_glob: Option<String>,
    pub command_pattern: Option<String>,
}
```

**修改文件：**
- `src-tauri/src/runtime/tools/permission.rs`（匹配器扩展）
- `src-tauri/src/runtime/tools/builtin/bash.rs`（把命令文本传入匹配器）
- `src-tauri/src/runtime/tools/builtin/file.rs`（把目标路径传入匹配器）
- `src-tauri/src/runtime/mcp/runtime_tool.rs`（确保 MCP tool call 走统一 authorize）

### P-5：P 轨回归测试 + review lock（U2-5）

**Rust 测试覆盖：**
- mode 变换（Default/Plan/DontAsk）
- 规则优先级（Session > Workspace > User）
- workspace/user 合并读取
- unknown scope Ask（Default）
- Bash `command_pattern` 匹配
- 文件 `path_glob` 匹配
- MCP tool 不绕过 pipeline

**前端测试覆盖：**
- PermissionAskDialog 的 remember 选项行为
- pending ask 清理
- mode 切换后 UI 文案

**review lock：**
- 新增 `review_permission_ask_semantics_test`，防止 Ask 再次退化为三按钮/字符串错误

---

## R 轨：声明式 Agent 注册

### 设计原则

- 内置 agent（browse_data、daily_assistant）和用户自定义 agent 共享同一套 `AgentDefinition` 结构
- 本期只做内置，用户自定义的加载器框架预留但不开放
- `model = "inherit"`（继承主 session 模型），本期不支持 per-agent 模型配置
- `permission_mode = "inherit"`，由 P-2 的 WorkerRunConfig 赋值逻辑控制

### R-1：AgentDefinition + AgentRegistry

**新建文件：** `src-tauri/src/runtime/agent/definition.rs`

```rust
#[derive(Clone, Debug)]
pub enum AgentModel {
    Inherit,
    Fixed(String),
}

#[derive(Clone, Debug)]
pub enum AgentPrompt {
    Inline(String),
    File(std::path::PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentSource {
    Builtin,
    User,
}

#[derive(Clone, Debug)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub max_iterations: usize,
    pub model: AgentModel,
    pub system_prompt: AgentPrompt,
    pub source: AgentSource,
}
```

**新建文件：** `src-tauri/src/runtime/agent/registry.rs`

```rust
pub struct AgentRegistry {
    agents: HashMap<String, AgentDefinition>,
}

impl AgentRegistry {
    pub fn with_builtins() -> Self         // 注册内置 agents
    pub fn get(&self, name: &str) -> Option<&AgentDefinition>
    pub fn list(&self) -> Vec<&AgentDefinition>   // 供前端 list_agents
    pub fn register(&mut self, def: AgentDefinition)
}
```

启动时在 `src-tauri/src/lib.rs` 中 `app.manage(Arc<AgentRegistry>)`，与 `McpServerManager` 同级。

### R-2：browse_data Agent 迁移

**新建：** `src-tauri/src/runtime/agent/builtin/browse_data_agent.rs`

```rust
pub fn browse_data_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "browse_data_agent".to_string(),
        description: "浏览器数据提取专家，从内部业务系统中提取表格数据".to_string(),
        allowed_tools: vec![
            "browse_and_extract".to_string(),
            "browse_navigate".to_string(),
            "read_page_content".to_string(),
            "page_execute_js".to_string(),
            "extract_table_data".to_string(),
            "extract_with_pagination".to_string(),
        ],
        max_iterations: 30,
        model: AgentModel::Inherit,
        source: AgentSource::Builtin,
    }
}
```

**修改：** `src-tauri/src/llm/tool_executor/internal_system.rs`

```rust
// 改造前（硬编码）
let config = SubAgentConfig {
    allowed_tools: vec![
        "browse_and_extract".to_string(),
        // ... 6 个硬编码字符串
    ],
    max_iterations: 30,
    ...
};

// 改造后（从 registry 读 definition）
let def = agent_registry
    .get("browse_data_agent")
    .expect("browse_data_agent must be registered");
let config = SubAgentConfig {
    allowed_tools: def.allowed_tools.clone(),
    max_iterations: def.max_iterations,
    ...
};
```

**注意：** `BrowseDataLauncher` 的前置逻辑（站点地图检测、多页选择、快速路径注入）**完全不动**，只有 `allowed_tools` 和 `max_iterations` 两个字段改为从 definition 读取。

### R-3：daily_assistant Agent 迁移

**新建：** `src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs`

```rust
pub fn daily_assistant_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "daily_assistant_agent".to_string(),
        description: "日常对话助手，限制工具集以保持安全边界".to_string(),
        allowed_tools: DAILY_ALLOWED_TOOLS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        max_iterations: 20,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(String::new()),  // daily 无专属 prompt
        source: AgentSource::Builtin,
    }
}
```

**修改：** `src-tauri/src/runtime/tools/catalog.rs`  
`DAILY_ALLOWED_TOOLS` 常量**保留**，作为 `daily_assistant_agent_definition()` 初始化时的数据源（避免重复定义）。但 `DailyAssistant` skill 和 `get_tool_defs` 改为通过 `AgentRegistry.get("daily_assistant_agent").allowed_tools` 取工具列表，不再直接引用常量——常量成为 registry 内部实现细节，外部调用者只感知 registry。

### R-4：list_agents Tauri Command + 前端选择器

**新增 command：** `src-tauri/src/transport/tauri_commands/agents.rs`

```rust
#[derive(Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
    pub source: String,  // "builtin" | "user"
}

#[tauri::command]
pub async fn list_agents(
    registry: State<'_, Arc<AgentRegistry>>,
) -> Result<Vec<AgentInfo>, String> {
    Ok(registry.list().iter().map(|def| AgentInfo {
        name: def.name.clone(),
        description: def.description.clone(),
        source: match def.source {
            AgentSource::Builtin => "builtin".to_string(),
            AgentSource::User => "user".to_string(),
        },
    }).collect())
}
```

**前端：** 在 `src/lib/tauri.ts` 新增 `listAgents()` 封装，在 `send_message` 时携带可选的 `agent_name?: string` 参数，`SessionRuntime` 收到后按 definition 约束工具池。

前端 UI 仅新增下拉选择器（chat 输入区附近），列出已注册 agent，默认"自动"（不指定 agent，走原有流程）。

---

## A 轨：Async Subagent 独立性

### A-1：background cancel 独立 token（接 H5 四步）

**当前问题：**
- `internal_system.rs` 构造 `SubAgentConfig` 时 `cancel_token` 有时传 `None`（H5-2 遗留）
- background subagent 的 cancel token 链接到父 session，导致用户按 ESC 时后台任务也被取消

**目标：**

```rust
// background = false：child token，父 cancel 级联
let cancel_token = if config.background {
    Some(CancellationToken::new())  // 完全独立，不链接父
} else {
    parent_cancel.map(|t| t.child_token())  // 单向级联
};
```

**修改文件：**
- `src-tauri/src/llm/tool_executor/internal_system.rs`（确保 cancel_token 不为 None）
- `src-tauri/src/runtime/agent/worker_runtime.rs`（按 background 标志选择 token 来源）
- 新建 `src-tauri/tests/subagent_legacy_cancel_reachability_test.rs`（H5-1 测试）

### A-2：lifecycle 状态机收口

**目标状态机：**

```
Pending
  → Running（spawn_child_run 成功）
  → Completed（run 正常返回）
  → Cancelled（cancel token 触发）
  → Failed（LLM error / 迭代耗尽且 output 为空）
```

每个状态转换通过 `RuntimeEventBus` 发对应事件：

| 转换 | 事件 |
|---|---|
| → Running | `TaskStatusChanged { status: Running }` |
| → Completed | `AgentIdle` + `TaskStatusChanged { status: Completed }` |
| → Cancelled | `TaskStatusChanged { status: Cancelled }` |
| → Failed | `TaskStatusChanged { status: Failed }` |

**当前缺口：** `worker_runtime.rs` 只在 `complete_run` / `cancel_run` 时更新 AgentRuntime 状态，但 Failed 状态没有独立路径，与 Cancelled 混用。

**修改文件：**
- `src-tauri/src/runtime/agent/invocation.rs`（AgentStatus 增加 Failed 变体）
- `src-tauri/src/runtime/agent/agent_runtime.rs`（fail_run 方法）
- `src-tauri/src/runtime/agent/worker_runtime.rs`（迭代耗尽 / LLM error 路径走 fail_run）

### A-3：回归测试 + review lock

**测试覆盖：**
- background agent 的 CancellationToken 与父 session token 不是同一个对象（`!Arc::ptr_eq`）
- 父 session cancel 不波及 background agent
- background agent 完成后前端能收到 `AgentIdle` 事件
- AskRequired 在 background agent 中走 `dontAsk` 路径（自动拒绝，不冒泡弹框）

**review lock 新增：**
- `review_background_subagent_cancel_is_independent`：background SubAgentConfig 的 cancel_token 不得为父 session token 的 child_token
- `review_subagent_cancel_token_not_none`：SubAgentConfig.cancel_token 不得为 None

---

## 执行顺序建议

### 第一批（可并行）
- **P-1**：PermissionStore 三层规则
- **R-1**：AgentDefinition + AgentRegistry
- **A-1**：background cancel 独立 token（H5 四步）

### 第二批（依赖第一批）
- **P-2**：WorkerRunConfig.permission_mode + PermissionMode 三种 mode + apply_permission_mode 规则表（依赖 P-1）
- **P-4**：Bash/文件/MCP 匹配维度补齐（依赖 P-1，可与 P-2 并行）
- **R-2**：browse_data 迁移（依赖 R-1）
- **A-2**：lifecycle 状态机（依赖 A-1）

### 第三批（依赖第二批）
- **P-3**：Ask 前端闭环（依赖 P-2）
- **P-5**：P 轨回归测试 + review lock（依赖 P-3/P-4）
- **R-3**：daily_assistant 迁移（依赖 R-2）
- **A-3**：回归测试 + review lock（依赖 A-2）

### 第四批（依赖第三批）
- **R-4**：前端 Agent 选择器（依赖 R-3 + list_agents command）

---

## 文件修改清单

| 文件 | 轨 | 操作 |
|---|---|---|
| `runtime/store/permission_store.rs` | P-1 | 修改：三层规则结构 + PermissionRule 扩展字段 |
| `runtime/tools/permission.rs` | P-1/P-2/P-4 | 修改：StorePolicyPipeline + PermissionMode 三种 + apply_permission_mode 规则表 + 匹配器 |
| `runtime/tools/builtin/bash.rs` | P-4 | 修改：命令文本传入匹配器 |
| `runtime/tools/builtin/file.rs` | P-4 | 修改：目标路径传入匹配器 |
| `runtime/mcp/runtime_tool.rs` | P-4 | 修改：确认走统一 authorize，无 bypass |
| `runtime/agent/worker_runtime.rs` | P-2/A-1/A-2 | 修改：permission_mode 字段 + cancel token 逻辑 + lifecycle |
| `llm/sub_agent.rs` | P-2 | 修改：SubAgentConfig 传递 permission_mode |
| `llm/tool_executor/internal_system.rs` | P-2/R-2/A-1 | 修改：cancel_token 不为 None + 从 registry 读 definition |
| `runtime/chat/chat_turn_driver.rs` | P-3 | 修改：补全 pending_permission_control_plane |
| `runtime/agent/definition.rs` | R-1 | 新建 |
| `runtime/agent/registry.rs` | R-1 | 新建 |
| `runtime/agent/builtin/browse_data_agent.rs` | R-2 | 新建 |
| `runtime/agent/builtin/daily_assistant_agent.rs` | R-3 | 新建 |
| `runtime/tools/catalog.rs` | R-3 | 修改：daily 通过 registry 读工具列表 |
| `transport/tauri_commands/agents.rs` | R-4 | 新建 |
| `runtime/agent/invocation.rs` | A-2 | 修改：AgentStatus 增加 Failed |
| `runtime/agent/agent_runtime.rs` | A-2 | 修改：fail_run 方法 |
| `src/lib/tauri.ts` | R-4 | 修改：listAgents + agent_name 参数 |
| `src/components/common/PermissionAskDialog.tsx` | P-3 | 修改：扩展操作选项 |
| `src/stores/streamingStore.ts` | P-3 | 修改：pending ask payload 扩展 |
| `tests/subagent_legacy_cancel_reachability_test.rs` | A-1 | 新建 |
| `tests/subagent_background_lifecycle_test.rs` | A-3 | 新建 |
| `tests/permission_rule_matching_test.rs` | P-5 | 新建 |

---

## 验收标准

**P 轨：**
- 子 agent 触发的 AskRequired 能完整路由到前端弹框，不再降级为 error
- 用户在弹框选择"记住到工作区"后，同一工具下次不再弹框
- background subagent 的权限未知 scope 自动拒绝，不弹交互框
- Bash 工具调用时 `command_pattern` 规则生效
- 文件工具调用时 `path_glob` 规则生效
- MCP tool 调用经过统一 `authorize()` 无 bypass
- `review_permission_ask_semantics_test` 通过

**R 轨：**
- `browse_data` 启动时工具池从 `AgentRegistry` 读取，删除 `internal_system.rs` 中硬编码的工具字符串列表
- `list_agents` command 返回正确的 agent 列表
- 前端可通过选择器指定 agent，send_message 携带 `agent_name`

**A 轨：**
- background subagent 的 CancellationToken 与父 session token 不链接（单元测试覆盖）
- 父 session 按 ESC 取消，background agent 继续运行
- agent 完成/取消/失败均有对应事件到达前端
