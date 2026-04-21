# 声明式 Agent 注册（Plan-R）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 AgentDefinition + AgentRegistry 机制，把 browse_data 和 daily_assistant 的硬编码工具白名单迁移为声明式注册，并通过 list_agents command 在前端暴露 agent 选择器。

**Architecture:** 新建 `runtime/agent/definition.rs` 和 `runtime/agent/registry.rs`，在 `lib.rs` 的 `app.manage` 里注册 `Arc<AgentRegistry>`。`internal_system.rs` 从 registry 读 `allowed_tools`/`max_iterations` 替代硬编码字符串；`catalog.rs` 的 `DAILY_ALLOWED_TOOLS` 保留为 registry 的数据源，外部通过 registry 查询。前端 `send_message` 增加可选 `agent_name` 参数，`SessionRuntime` 收到后用 definition 的 allowed_tools 覆盖本次 turn 的 ToolFilter。

**Tech Stack:** Rust, Tauri v2, React/TypeScript

**Worktree branch:** pzc

---

## 文件地图

| 文件 | 操作 |
|---|---|
| `src-tauri/src/runtime/agent/definition.rs` | 新建：AgentDefinition, AgentModel, AgentPrompt, AgentSource |
| `src-tauri/src/runtime/agent/registry.rs` | 新建：AgentRegistry |
| `src-tauri/src/runtime/agent/builtin/mod.rs` | 新建或修改：pub mod browse_data_agent; pub mod daily_assistant_agent |
| `src-tauri/src/runtime/agent/builtin/browse_data_agent.rs` | 新建：browse_data_agent_definition() |
| `src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs` | 新建：daily_assistant_agent_definition() |
| `src-tauri/src/runtime/agent/mod.rs` | 修改：pub mod definition; pub mod registry; pub mod builtin |
| `src-tauri/src/lib.rs` | 修改：构造并 manage AgentRegistry；注册 list_agents command |
| `src-tauri/src/llm/tool_executor/internal_system.rs` | 修改：从 registry 读 allowed_tools / max_iterations |
| `src-tauri/src/plugin/builtin/skills/daily_assistant.rs` | 修改：通过 registry 取工具列表 |
| `src-tauri/src/transport/tauri_commands/agents.rs` | 新建：list_agents command |
| `src-tauri/src/transport/tauri_commands/mod.rs` | 修改：pub mod agents |
| `src/lib/tauri.ts` | 修改：listAgents() + send_message 增加 agent_name |
| `src/components/chat/AgentSelector.tsx` | 新建：agent 下拉选择器组件 |

---

## Task 1：定义 AgentDefinition 和 AgentRegistry

**文件：**
- 新建：`src-tauri/src/runtime/agent/definition.rs`
- 新建：`src-tauri/src/runtime/agent/registry.rs`
- 修改：`src-tauri/src/runtime/agent/mod.rs`

- [x] **Step 1：写失败测试**

新建 `src-tauri/tests/agent_registry_test.rs`：

```rust
use app_lib::runtime::agent::definition::{AgentDefinition, AgentModel, AgentPrompt, AgentSource};
use app_lib::runtime::agent::registry::AgentRegistry;

#[test]
fn registry_with_builtins_has_browse_data_agent() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent");
    assert!(def.is_some(), "browse_data_agent must be registered");
}

#[test]
fn registry_with_builtins_has_daily_assistant_agent() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent");
    assert!(def.is_some(), "daily_assistant_agent must be registered");
}

#[test]
fn browse_data_agent_has_six_browser_tools() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    assert_eq!(def.allowed_tools.len(), 6);
    assert!(def.allowed_tools.contains(&"browse_and_extract".to_string()));
    assert!(def.allowed_tools.contains(&"browse_navigate".to_string()));
    assert!(def.allowed_tools.contains(&"read_page_content".to_string()));
    assert!(def.allowed_tools.contains(&"page_execute_js".to_string()));
    assert!(def.allowed_tools.contains(&"extract_table_data".to_string()));
    assert!(def.allowed_tools.contains(&"extract_with_pagination".to_string()));
}

#[test]
fn browse_data_agent_max_iterations_is_30() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    assert_eq!(def.max_iterations, 30);
}

#[test]
fn daily_assistant_agent_has_eight_tools() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent").unwrap();
    assert_eq!(def.allowed_tools.len(), 8);
    assert!(def.allowed_tools.contains(&"bash".to_string()));
}

#[test]
fn registry_list_returns_all_builtins() {
    let registry = AgentRegistry::with_builtins();
    let list = registry.list();
    assert!(list.len() >= 2);
}
```

- [x] **Step 2：运行确认失败**

```bash
cd src-tauri && cargo test --test agent_registry_test -- --nocapture 2>&1 | head -20
```

期望：编译错误，`definition` 和 `registry` 模块不存在

- [x] **Step 3：新建 definition.rs**

```rust
// src-tauri/src/runtime/agent/definition.rs

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

#[derive(Clone, Debug, PartialEq, Eq)]
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

- [x] **Step 4：新建 builtin/browse_data_agent.rs**

```rust
// src-tauri/src/runtime/agent/builtin/browse_data_agent.rs
use crate::runtime::agent::definition::{AgentDefinition, AgentModel, AgentPrompt, AgentSource};

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
        system_prompt: AgentPrompt::Inline(String::new()), // prompt 由 launcher 加载
        source: AgentSource::Builtin,
    }
}
```

- [x] **Step 5：新建 builtin/daily_assistant_agent.rs**

```rust
// src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs
use crate::runtime::agent::definition::{AgentDefinition, AgentModel, AgentPrompt, AgentSource};
use crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;

pub fn daily_assistant_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "daily_assistant_agent".to_string(),
        description: "日常对话助手，受限工具集保持安全边界".to_string(),
        allowed_tools: DAILY_ALLOWED_TOOLS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        max_iterations: 20,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(String::new()),
        source: AgentSource::Builtin,
    }
}
```

- [x] **Step 6：新建 builtin/mod.rs**

```rust
// src-tauri/src/runtime/agent/builtin/mod.rs
pub mod browse_data_agent;
pub mod daily_assistant_agent;
```

- [x] **Step 7：新建 registry.rs**

```rust
// src-tauri/src/runtime/agent/registry.rs
use std::collections::HashMap;
use crate::runtime::agent::definition::AgentDefinition;
use crate::runtime::agent::builtin::{
    browse_data_agent::browse_data_agent_definition,
    daily_assistant_agent::daily_assistant_agent_definition,
};

pub struct AgentRegistry {
    agents: HashMap<String, AgentDefinition>,
}

impl AgentRegistry {
    pub fn with_builtins() -> Self {
        let mut registry = Self {
            agents: HashMap::new(),
        };
        registry.register(browse_data_agent_definition());
        registry.register(daily_assistant_agent_definition());
        registry
    }

    pub fn register(&mut self, def: AgentDefinition) {
        self.agents.insert(def.name.clone(), def);
    }

    pub fn get(&self, name: &str) -> Option<&AgentDefinition> {
        self.agents.get(name)
    }

    pub fn list(&self) -> Vec<&AgentDefinition> {
        let mut list: Vec<&AgentDefinition> = self.agents.values().collect();
        list.sort_by_key(|d| &d.name);
        list
    }
}
```

- [x] **Step 8：修改 runtime/agent/mod.rs，暴露新模块**

在 `src-tauri/src/runtime/agent/mod.rs` 末尾追加：

```rust
pub mod builtin;
pub mod definition;
pub mod registry;
```

- [x] **Step 9：运行确认通过**

```bash
cd src-tauri && cargo test --test agent_registry_test -- --nocapture
```

期望：全部 `PASSED`

- [x] **Step 10：Commit**

```bash
git add \
  src-tauri/src/runtime/agent/definition.rs \
  src-tauri/src/runtime/agent/registry.rs \
  src-tauri/src/runtime/agent/builtin/mod.rs \
  src-tauri/src/runtime/agent/builtin/browse_data_agent.rs \
  src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs \
  src-tauri/src/runtime/agent/mod.rs \
  src-tauri/tests/agent_registry_test.rs
git commit -m "feat(agent): AgentDefinition + AgentRegistry + builtin browse_data/daily_assistant"
```

---

## Task 2：lib.rs 注册 AgentRegistry

**文件：**
- 修改：`src-tauri/src/lib.rs`

- [x] **Step 1：在 lib.rs 找到 app.manage 区块（约行 292-347）**

```bash
grep -n "app.manage\|agent_runtime\|mcp_server_manager" src-tauri/src/lib.rs | head -15
```

- [x] **Step 2：构造并注册 AgentRegistry**

在 `app.manage(agent_runtime);` 之后，`app.manage(chat_adapter);` 之前插入：

```rust
// Agent registry — 声明式 agent 注册中心
let agent_registry = std::sync::Arc::new(
    crate::runtime::agent::registry::AgentRegistry::with_builtins()
);
app.manage(agent_registry);
```

- [x] **Step 3：编译确认无错误**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

期望：无错误

- [x] **Step 4：Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(lib): manage AgentRegistry as global app state"
```

---

## Task 3：internal_system.rs 从 registry 读 browse_data 配置

**当前问题：** `internal_system.rs` 约行 356-374 的 `SubAgentConfig` 构造中，`allowed_tools` 和 `max_iterations` 是硬编码字符串。

**文件：**
- 修改：`src-tauri/src/llm/tool_executor/internal_system.rs`

- [ ] **Step 1：写测试确认迁移后行为不变**

在 `agent_registry_test.rs` 追加：

```rust
#[test]
fn browse_data_agent_tools_match_legacy_hardcoded_list() {
    // 确保 registry 里的工具列表与原来 internal_system.rs 的硬编码完全一致
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    let expected = vec![
        "browse_and_extract",
        "browse_navigate",
        "read_page_content",
        "page_execute_js",
        "extract_table_data",
        "extract_with_pagination",
    ];
    for tool in &expected {
        assert!(
            def.allowed_tools.contains(&tool.to_string()),
            "browse_data_agent must contain tool: {}", tool
        );
    }
    assert_eq!(def.allowed_tools.len(), expected.len());
}
```

- [ ] **Step 2：运行确认通过**

```bash
cd src-tauri && cargo test --test agent_registry_test browse_data_agent_tools_match -- --nocapture
```

期望：`PASSED`

- [ ] **Step 3：修改 launch_browse_data_with_runtime_deps，接收 AgentRegistry 参数**

`internal_system.rs` 的 `launch_browse_data_with_runtime_deps` 函数签名增加 `agent_registry` 参数：

```rust
async fn launch_browse_data_with_runtime_deps(
    ctx: &RequestScopedRuntimeDeps,
    request: BrowseDataLaunchRequest,
    cancel_token: Option<CancellationToken>,
    sub_agent_background: bool,
    agent_registry: Option<&crate::runtime::agent::registry::AgentRegistry>,
) -> Result<BrowseDataLaunchResult> {
```

在 `SubAgentConfig` 构造处，改为从 registry 读取（如果 registry 可用），否则 fallback 到原硬编码：

```rust
let (allowed_tools, max_iterations) = if let Some(reg) = agent_registry {
    if let Some(def) = reg.get("browse_data_agent") {
        (def.allowed_tools.clone(), def.max_iterations)
    } else {
        // fallback
        (vec![
            "browse_and_extract".to_string(),
            "browse_navigate".to_string(),
            "read_page_content".to_string(),
            "page_execute_js".to_string(),
            "extract_table_data".to_string(),
            "extract_with_pagination".to_string(),
        ], 30)
    }
} else {
    (vec![
        "browse_and_extract".to_string(),
        "browse_navigate".to_string(),
        "read_page_content".to_string(),
        "page_execute_js".to_string(),
        "extract_table_data".to_string(),
        "extract_with_pagination".to_string(),
    ], 30)
};

let config = crate::llm::sub_agent::SubAgentConfig {
    task: task_msg,
    system_prompt,
    allowed_tools,
    max_iterations,
    dynamic_context,
    conversation_id: ctx.conversation_id.clone(),
    parent_run_id: ctx.run_id.clone(),
    background: sub_agent_background,
    app_handle: ctx.app_handle.clone(),
    cancel_token,
    permission_mode: if sub_agent_background {
        crate::runtime::tools::permission::PermissionMode::DontAsk
    } else {
        crate::runtime::tools::permission::PermissionMode::Default
    },
};
```

- [ ] **Step 4：更新所有调用 launch_browse_data_with_runtime_deps 的地方，传入 registry**

```bash
grep -n "launch_browse_data_with_runtime_deps" src-tauri/src/llm/tool_executor/internal_system.rs
```

对每处调用追加 `agent_registry: None`（后续 Task 会改为真正传入）。

- [ ] **Step 5：编译确认无错误**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/llm/tool_executor/internal_system.rs \
        src-tauri/tests/agent_registry_test.rs
git commit -m "refactor(browse_data): read allowed_tools/max_iterations from AgentRegistry"
```

---

## Task 4：daily_assistant.rs 从 registry 取工具列表

**当前问题：** `daily_assistant.rs:47` 直接引用 `DAILY_ALLOWED_TOOLS` 常量。

**文件：**
- 修改：`src-tauri/src/plugin/builtin/skills/daily_assistant.rs`

- [ ] **Step 1：写测试**

在 `agent_registry_test.rs` 追加：

```rust
use app_lib::plugin::builtin::skills::daily_assistant::DailyAssistantSkill;
use app_lib::plugin::skill_trait::{Skill, ToolFilter};
use app_lib::runtime::agent::registry::AgentRegistry;

#[test]
fn daily_assistant_tool_filter_matches_registry_definition() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("daily_assistant_agent").unwrap();
    // DailyAssistantSkill 的 tool_filter 应该与 registry 里的 allowed_tools 一致
    let skill = DailyAssistantSkill::new_with_registry(&registry);
    let filter = skill.tool_filter();
    match filter {
        ToolFilter::Only(tools) => {
            assert_eq!(tools.len(), def.allowed_tools.len());
            for tool in &def.allowed_tools {
                assert!(tools.contains(tool), "filter must include {}", tool);
            }
        }
        _ => panic!("DailyAssistantSkill must use ToolFilter::Only"),
    }
}
```

- [ ] **Step 2：运行确认失败**

```bash
cd src-tauri && cargo test --test agent_registry_test daily_assistant_tool_filter -- --nocapture 2>&1 | head -15
```

期望：编译错误，`new_with_registry` 不存在

- [ ] **Step 3：修改 DailyAssistantSkill**

`src-tauri/src/plugin/builtin/skills/daily_assistant.rs`：

```rust
use crate::runtime::agent::registry::AgentRegistry;

pub struct DailyAssistantSkill {
    allowed_tools: Vec<String>,
}

impl DailyAssistantSkill {
    /// 生产路径：从 AgentRegistry 读取工具列表
    pub fn new_with_registry(registry: &AgentRegistry) -> Self {
        let tools = registry
            .get("daily_assistant_agent")
            .map(|def| def.allowed_tools.clone())
            .unwrap_or_else(|| {
                // fallback 到常量，确保向后兼容
                crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });
        Self { allowed_tools: tools }
    }

    /// 测试/兼容路径：直接用常量
    pub fn new() -> Self {
        Self {
            allowed_tools: crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl Skill for DailyAssistantSkill {
    fn tool_filter(&self) -> ToolFilter {
        ToolFilter::Only(self.allowed_tools.clone())
    }
    // ... 其余 Skill 方法保持不变
}
```

- [ ] **Step 4：运行确认通过**

```bash
cd src-tauri && cargo test --test agent_registry_test daily_assistant_tool_filter -- --nocapture
```

期望：`PASSED`

- [ ] **Step 5：更新 DailyAssistantSkill 的构造调用处**

```bash
grep -rn "DailyAssistantSkill::new\b" src-tauri/src/
```

对所有调用处，如果能拿到 `AgentRegistry`（通过 State 或参数），改为 `DailyAssistantSkill::new_with_registry(®istry)`，否则保持 `DailyAssistantSkill::new()`。

- [ ] **Step 6：编译确认无错误**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

- [ ] **Step 7：Commit**

```bash
git add src-tauri/src/plugin/builtin/skills/daily_assistant.rs \
        src-tauri/tests/agent_registry_test.rs
git commit -m "refactor(daily_assistant): read tool list from AgentRegistry, keep DAILY_ALLOWED_TOOLS as data source"
```

---

## Task 5：list_agents Tauri Command

**文件：**
- 新建：`src-tauri/src/transport/tauri_commands/agents.rs`
- 修改：`src-tauri/src/transport/tauri_commands/mod.rs`
- 修改：`src-tauri/src/lib.rs`（invoke_handler 注册）

- [ ] **Step 1：新建 agents.rs**

```rust
// src-tauri/src/transport/tauri_commands/agents.rs
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use crate::runtime::agent::registry::AgentRegistry;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
    pub source: String, // "builtin" | "user"
}

#[tauri::command]
pub async fn list_agents(
    registry: State<'_, Arc<AgentRegistry>>,
) -> Result<Vec<AgentInfo>, String> {
    let list = registry
        .list()
        .into_iter()
        .map(|def| AgentInfo {
            name: def.name.clone(),
            description: def.description.clone(),
            source: match def.source {
                crate::runtime::agent::definition::AgentSource::Builtin => "builtin".to_string(),
                crate::runtime::agent::definition::AgentSource::User => "user".to_string(),
            },
        })
        .collect();
    Ok(list)
}
```

- [ ] **Step 2：修改 mod.rs 暴露 agents 模块**

在 `src-tauri/src/transport/tauri_commands/mod.rs` 追加：

```rust
pub mod agents;
```

- [ ] **Step 3：在 lib.rs invoke_handler 注册**

找到约行 409-420 的 MCP 相关 command 注册，在其后追加：

```rust
// Agent registry commands
transport::tauri_commands::agents::list_agents,
```

- [ ] **Step 4：编译确认**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

期望：无错误

- [ ] **Step 5：Commit**

```bash
git add \
  src-tauri/src/transport/tauri_commands/agents.rs \
  src-tauri/src/transport/tauri_commands/mod.rs \
  src-tauri/src/lib.rs
git commit -m "feat(api): list_agents Tauri command returns registered agent definitions"
```

---

## Task 6：前端 listAgents + send_message 增加 agent_name

**文件：**
- 修改：`src/lib/tauri.ts`
- 修改：`src/stores/chatStore.ts`（发送时携带 agent_name）

- [ ] **Step 1：修改 tauri.ts 新增 listAgents**

找到 `tauri.ts` 里 `invoke` 调用集中处，追加：

```typescript
export interface AgentInfo {
  name: string;
  description: string;
  source: 'builtin' | 'user';
}

export async function listAgents(): Promise<AgentInfo[]> {
  return await invoke<AgentInfo[]>('list_agents');
}
```

同时找到 `sendMessage` 函数，增加可选 `agentName` 参数：

```typescript
export async function sendMessage(params: {
  conversationId: string;
  message: string;
  agentName?: string;  // 新增
  // ... 其余现有参数
}): Promise<void> {
  return await invoke('send_message', {
    conversationId: params.conversationId,
    message: params.message,
    agentName: params.agentName ?? null,
    // ... 其余现有参数
  });
}
```

- [ ] **Step 2：修改 Rust send_message command 接收 agent_name**

```bash
grep -n "pub async fn send_message\|agent_name" src-tauri/src/transport/tauri_commands/chat.rs | head -10
```

在 `send_message` command 的参数列表增加 `agent_name: Option<String>`，并传递给 `SessionRuntime`：

```rust
#[tauri::command]
pub async fn send_message(
    // ... 现有参数
    agent_name: Option<String>,
    // ...
) -> Result<...> {
    // 传给 session_runtime 的 run_chat_request
}
```

- [ ] **Step 3：SessionRuntime 用 agent_name 约束工具池**

在 `session_runtime.rs` 的 `run_chat_request` 方法里，若收到 `agent_name`，从 `AgentRegistry` 查出 definition，用 `definition.allowed_tools` 覆盖本次 turn 的 `TurnConfig.allowed_tools`（`turn_config.rs:56` 已有 `Option<HashSet<String>>`）：

```rust
// session_runtime.rs run_chat_request 入口处
if let Some(ref agent_name) = request.agent_name {
    if let Some(registry) = self.agent_registry.as_ref() {
        if let Some(def) = registry.get(agent_name) {
            turn_config.allowed_tools = Some(
                def.allowed_tools.iter().cloned().collect()
            );
        }
    }
}
```

`SessionRuntime` 需要持有 `agent_registry: Option<Arc<AgentRegistry>>`，在 `lib.rs` 构造时注入。

- [ ] **Step 4：前端构建确认**

```bash
pnpm build 2>&1 | grep -i "error\|Error" | grep -v "warning" | head -10
```

期望：无错误

- [ ] **Step 5：Commit**

```bash
git add src/lib/tauri.ts \
        src-tauri/src/transport/tauri_commands/chat.rs \
        src-tauri/src/runtime/session_runtime.rs
git commit -m "feat(frontend): listAgents() + send_message agent_name constrains tool pool"
```

---

## Task 7：前端 Agent 选择器组件

**文件：**
- 新建：`src/components/chat/AgentSelector.tsx`
- 修改：chat 输入区父组件（引入 AgentSelector）

- [ ] **Step 1：新建 AgentSelector.tsx**

```tsx
// src/components/chat/AgentSelector.tsx
import { useEffect, useState } from 'react';
import { listAgents, AgentInfo } from '../../lib/tauri';

interface AgentSelectorProps {
  value: string | null;
  onChange: (agentName: string | null) => void;
}

export function AgentSelector({ value, onChange }: AgentSelectorProps) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);

  useEffect(() => {
    listAgents().then(setAgents).catch(console.error);
  }, []);

  return (
    <select
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value || null)}
      className="agent-selector"
    >
      <option value="">自动（默认）</option>
      {agents.map((agent) => (
        <option key={agent.name} value={agent.name}>
          {agent.description || agent.name}
        </option>
      ))}
    </select>
  );
}
```

- [ ] **Step 2：在 chat 输入区使用 AgentSelector**

找到发送消息的输入组件（通常是 `ChatInput.tsx` 或类似名称）：

```bash
find src/components -name "*Input*" -o -name "*Chat*" | grep -v node_modules | head -10
```

在该组件里引入并使用 `AgentSelector`，并在 `sendMessage` 调用时携带 `agentName`：

```tsx
import { AgentSelector } from './AgentSelector';

// 在组件 state 里：
const [selectedAgent, setSelectedAgent] = useState<string | null>(null);

// 在 JSX 里：
<AgentSelector value={selectedAgent} onChange={setSelectedAgent} />

// 在发送时：
await sendMessage({
  conversationId,
  message,
  agentName: selectedAgent ?? undefined,
});
```

- [ ] **Step 3：前端构建确认**

```bash
pnpm build 2>&1 | grep -i "^error\|Error:" | head -10
```

期望：无错误

- [ ] **Step 4：Commit**

```bash
git add src/components/chat/AgentSelector.tsx
git commit -m "feat(ui): AgentSelector dropdown for registered agents"
```

---

## Task 8：review lock

**文件：**
- 新建：`src-tauri/tests/review_agent_registry_test.rs`

- [ ] **Step 1：新建 review test**

```rust
//! review_agent_registry — 防止 browse_data 工具硬编码退化的架构约束测试。

use app_lib::runtime::agent::registry::AgentRegistry;

/// browse_data_agent 必须在 registry 中注册，且工具列表完整。
#[test]
fn review_browse_data_agent_must_be_registered_with_six_tools() {
    let registry = AgentRegistry::with_builtins();
    let def = registry
        .get("browse_data_agent")
        .expect("browse_data_agent must be registered in AgentRegistry::with_builtins()");

    assert_eq!(
        def.allowed_tools.len(),
        6,
        "browse_data_agent must have exactly 6 browser tools"
    );
}

/// daily_assistant_agent 必须在 registry 中注册。
#[test]
fn review_daily_assistant_agent_must_be_registered() {
    let registry = AgentRegistry::with_builtins();
    let def = registry
        .get("daily_assistant_agent")
        .expect("daily_assistant_agent must be registered in AgentRegistry::with_builtins()");

    assert!(
        def.allowed_tools.len() >= 8,
        "daily_assistant_agent must have at least 8 tools"
    );
}

/// browse_data_agent max_iterations 必须 >= 20。
#[test]
fn review_browse_data_agent_max_iterations_reasonable() {
    let registry = AgentRegistry::with_builtins();
    let def = registry.get("browse_data_agent").unwrap();
    assert!(
        def.max_iterations >= 20,
        "browse_data_agent max_iterations must be at least 20, got {}",
        def.max_iterations
    );
}
```

- [ ] **Step 2：运行**

```bash
cd src-tauri && cargo test review_agent_registry -- --nocapture
```

期望：全部 `PASSED`

- [ ] **Step 3：Commit**

```bash
git add src-tauri/tests/review_agent_registry_test.rs
git commit -m "test(review): lock AgentRegistry builtin registration constraints"
```

---

## 验收检查

```bash
# 全量 Rust 测试
cd src-tauri && cargo test --tests --no-fail-fast

# 架构约束
cd src-tauri && cargo test review_ --tests --no-fail-fast

# 前端构建
pnpm build
```

所有测试通过，且 `internal_system.rs` 中不再有硬编码的 6 个工具字符串列表，即为 Plan-R 完成。
