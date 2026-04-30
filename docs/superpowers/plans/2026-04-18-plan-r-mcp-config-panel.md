# MCP 服务器配置面板（Plan-R）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:executing-plans`. Each task ends with a mandatory review checkpoint before proceeding.

**Goal:** 实现 MCP server 的前端配置与管理界面，让用户能在 Settings → MCP 标签页中增删 MCP server、查看连接状态、手动连接/断开。
**Architecture:** 后端 `McpServerManager` 已就绪；本计划补齐配置持久化层（R0）和 Tauri command 层（R0），再逐步构建前端 UI（R1–R4）及可选事件订阅（R5）。
**Tech Stack:** Rust (Tauri v2), React, TypeScript, Zustand (可选), i18n (react-i18next)
**前置依赖:** R0 是后端任务，必须先合入才能开展 R1–R5。
**Worktree branch:** pzc

---

## 背景与约束

- `src-tauri/src/runtime/mcp/manager.rs` 已实现完整的 `McpServerManager`（connect / disconnect / refresh / list_servers / unregister），但它在内存中管理 server，没有持久化。
- `src-tauri/src/runtime/mcp/types.rs` 中 `McpServerConfig` 字段：`name: String`、`transport_type: String`（"stdio" / "http" / "sse"）、`endpoint: String`（命令路径或 URL）、`env_vars: Option<HashMap<String, String>>`。
- `McpServerManager` 在 `lib.rs` 中通过 `app.manage(Arc<McpServerManager>)` 注册，可在 Tauri command 中通过 `State<Arc<McpServerManager>>` 访问。
- Tauri command 层按 `transport/tauri_commands/` 目录分文件组织，每个文件暴露若干 `#[tauri::command]` 函数；新增 command 需同时在 `lib.rs` 的 `generate_handler![]` 中注册。
- 前端 `src/lib/tauri.ts` 是 IPC 唯一真相源，所有新 invoke 必须在此文件增加类型化封装。
- 现有 Settings tab 类型为 `'account' | 'models' | 'search' | 'general' | 'persona' | 'skills'`，新增 `'mcp'`。
- i18n key 约定：新增 key 统一放在 `settings.mcp.*` 命名空间下，中英文均需补充。
- CLAUDE.md 约束：`runtime/` 下禁止 `use tauri::*`；Tauri command 层只做参数接收 → 转发 Manager，不含业务逻辑。

---

## Task R0：后端前置 — MCP 配置持久化 + Tauri Commands

**类型：Rust 后端任务（前端 R1–R5 的硬前置）**

### R0-1 配置持久化：`McpConfigStore`

新建文件 `src-tauri/src/storage/mcp_config_store.rs`，职责：将 `Vec<McpServerConfig>` 序列化为 JSON，持久化到 `{app_config_dir}/mcp_servers.json`。

```
AppConfigDir（Tauri v2 API: app_config_dir()）
  └── mcp_servers.json   ← [{ name, transport_type, endpoint, env_vars }, ...]
```

实现要点：
- 结构体 `McpConfigStore { path: PathBuf }`
- `fn load(&self) -> Result<Vec<McpServerConfig>, String>` — 文件不存在返回空 vec
- `fn save(&self, configs: &[McpServerConfig]) -> Result<(), String>`
- `fn add(&self, config: McpServerConfig) -> Result<(), String>` — 检查 name 唯一性，重名返回 Err
- `fn remove(&self, name: &str) -> Result<(), String>` — 不存在返回 Err
- 使用 `serde_json` 做序列化；文件不存在时 `load` 返回 `Ok(vec![])`

在 `lib.rs` setup 阶段初始化 `McpConfigStore`，并在初始化 `McpServerManager` 之后，读取持久化的 configs 并逐一调用 `manager.register(...)` 预注册（不自动 connect，保持惰性连接策略）。

### R0-2 新增 Tauri Command 文件

新建 `src-tauri/src/transport/tauri_commands/mcp.rs`，实现以下 5 个 command。每个 command 都只做：拿到 state → 调用 manager/store → 返回结果。

```rust
// 所有 command 均声明为 async，用 #[tauri::command] 标注

// 列出所有已注册 server 的配置 + 运行时连接状态
#[tauri::command]
pub async fn list_mcp_servers(
    manager: State<'_, Arc<McpServerManager>>,
) -> Result<Vec<McpServerStatusDto>, String>

// 新增并持久化一个 server（不自动连接）
#[tauri::command]
pub async fn add_mcp_server(
    config: McpServerConfigDto,
    manager: State<'_, Arc<McpServerManager>>,
    config_store: State<'_, Arc<McpConfigStore>>,
) -> Result<(), String>

// 从 manager 和持久化存储中删除一个 server（若已连接先断开）
#[tauri::command]
pub async fn remove_mcp_server(
    server_name: String,
    manager: State<'_, Arc<McpServerManager>>,
    config_store: State<'_, Arc<McpConfigStore>>,
) -> Result<(), String>

// 连接一个已注册的 server
#[tauri::command]
pub async fn connect_mcp_server(
    server_name: String,
    manager: State<'_, Arc<McpServerManager>>,
) -> Result<Vec<String>, String>   // 返回注册的 tool ids

// 断开一个已连接的 server
#[tauri::command]
pub async fn disconnect_mcp_server(
    server_name: String,
    manager: State<'_, Arc<McpServerManager>>,
) -> Result<(), String>
```

**DTO 类型**（在 `mcp.rs` 内定义，仅用于 transport 层序列化，`#[serde(rename_all = "camelCase")]`）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfigDto {
    pub name: String,
    pub transport_type: String,
    pub endpoint: String,
    pub env_vars: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatusDto {
    pub name: String,
    pub transport_type: String,
    pub endpoint: String,
    pub connected: bool,
    pub registered_tool_ids: Vec<String>,
}
```

`McpServerConfigDto` → `McpServerConfig` 的转换在 command 层完成（`From` impl 或手动转换）。

`add_mcp_server` 实现逻辑：
1. 将 DTO 转换为 `McpServerConfig`
2. `config_store.add(config.clone())?`
3. 构建 `MockMcpConnection`（此处需要一个"已配置但未连接"的占位 connection，或者直接用 `StdioMcpConnection`/`HttpMcpConnection`——取决于 transport_type；如果真实 connection 实现尚未就绪，用一个轻量的 `PendingMcpConnection` 占位，实现 `McpConnection` trait，`connect()` 时再真正建立连接）
4. `manager.register(Arc::new(connection)).await.map_err(|e| e.to_string())`

> **注意**：R0-2 中 `add_mcp_server` 需要把 `McpServerConfig` 包装成 `SharedMcpConnection` 注册到 manager。由于真实的 stdio/http 传输实现可能不在本计划范围内，R0-2 可以先用一个最小 `PendingMcpConnection` 实现，保证整个 CRUD 流程可测试，真实传输实现放到独立任务。

### R0-3 注册 Commands

在 `transport/tauri_commands/mod.rs` 增加 `pub mod mcp;`。

在 `lib.rs` 的 `generate_handler![]` 中追加：

```rust
// MCP server management commands
mcp::list_mcp_servers,
mcp::add_mcp_server,
mcp::remove_mcp_server,
mcp::connect_mcp_server,
mcp::disconnect_mcp_server,
```

同时在 `lib.rs` setup 阶段用 `app.manage(Arc::new(mcp_config_store))` 注册 `McpConfigStore`。

### R0-4 测试

新建 `src-tauri/tests/mcp_config_test.rs`，测试：
- `McpConfigStore::add` 后 `load` 能读回正确数据
- 重名 `add` 返回 Err
- `remove` 后 `load` 不含已删 server
- round-trip 序列化（env_vars=None 和 Some(map) 两种情况）

### R0 Commit

```
feat(mcp): add config persistence and tauri commands for mcp server management - R0
```

---

## Task R1：`tauri.ts` MCP invoke 封装 + 类型定义

**类型：前端，依赖 R0**

在 `src/lib/tauri.ts` 末尾新增 MCP Commands 区块：

```typescript
// ---------------------------------------------------------------------------
// MCP Server Commands
// ---------------------------------------------------------------------------

/** MCP server config as returned / sent by the backend. */
export interface McpServerConfig {
  name: string
  transportType: string  // "stdio" | "http" | "sse"
  endpoint: string       // command path for stdio, URL for http/sse
  envVars?: Record<string, string>
}

/** Runtime status of an MCP server (config + connection state). */
export interface McpServerStatus {
  name: string
  transportType: string
  endpoint: string
  connected: boolean
  registeredToolIds: string[]
}

/** List all registered MCP servers with their connection status. */
export function listMcpServers(): Promise<McpServerStatus[]> {
  return invoke<McpServerStatus[]>('list_mcp_servers')
}

/** Add and persist a new MCP server (does not auto-connect). */
export function addMcpServer(config: McpServerConfig): Promise<void> {
  return invoke<void>('add_mcp_server', { config })
}

/** Remove an MCP server from registry and persistent config. */
export function removeMcpServer(serverName: string): Promise<void> {
  return invoke<void>('remove_mcp_server', { serverName })
}

/**
 * Connect a registered MCP server.
 * @returns Array of fully-qualified tool IDs registered by this server.
 */
export function connectMcpServer(serverName: string): Promise<string[]> {
  return invoke<string[]>('connect_mcp_server', { serverName })
}

/** Disconnect a connected MCP server (tools are unregistered). */
export function disconnectMcpServer(serverName: string): Promise<void> {
  return invoke<void>('disconnect_mcp_server', { serverName })
}
```

### R1 测试

在 `src/lib/tauri.ts` 的测试约定下，无需单测 Tauri IPC 封装本身（mock 意义有限）。仅做类型检查验证：`pnpm build` 后无 TypeScript 类型错误。

### R1 Commit

```
feat(mcp): add typed tauri.ts invoke wrappers for mcp server commands - R1
```

---

## Task R2：`McpServerList` 组件

**类型：前端，依赖 R1**

新建 `src/components/settings/McpServerList.tsx`。

**职责：** 展示已配置的 MCP server 列表，每行显示 server 名称、transport 类型、endpoint、连接状态徽章，以及"连接"/"断开"/"删除"操作按钮。

**Props：**

```typescript
interface McpServerListProps {
  servers: McpServerStatus[]
  loading: boolean
  onConnect: (name: string) => Promise<void>
  onDisconnect: (name: string) => Promise<void>
  onDelete: (name: string) => Promise<void>
  actionLoading: Record<string, boolean>  // serverName → 是否正在操作
}
```

**布局结构：**

```
┌──────────────────────────────────────────────────────────────┐
│ [server name]    stdio  /path/to/cmd   ● Connected  [断开][删除] │
│ [server name]    http   http://...     ○ Offline    [连接][删除]  │
│ ...                                                          │
└──────────────────────────────────────────────────────────────┘
```

- 连接状态徽章：绿色实心圆 + "已连接" 或灰色空心圆 + "未连接"，样式参考 `SkillsTab` 中 `devWatching` 徽章
- 操作按钮用 `<Button variant="secondary" size="sm">`，与 `SkillsTab` 风格一致
- 正在执行操作时该行对应按钮 disabled，显示加载文字
- 空列表时显示 empty state 提示，参考 `SkillsTab` empty 状态的写法
- loading 时显示占位文字

**样式约定：** 使用 CSS 变量（`var(--color-border)` 等），不硬编码颜色，与 `SkillsTab` 中 skill 卡片保持一致。

### R2 测试

暂不写 Vitest 单测（需要 Tauri mock，成本高）。完成后手动验证：在 SettingsModal 中能正确渲染 loading 状态和空列表。

### R2 Commit

```
feat(mcp): add McpServerList component - R2
```

---

## Task R3：`McpServerForm` 新增/编辑组件

**类型：前端，依赖 R1**

新建 `src/components/settings/McpServerForm.tsx`。

**职责：** 一个内联表单（非弹窗），用于新增 MCP server 配置。展开/收起由父组件控制（传入 `visible: boolean`）。

**Props：**

```typescript
interface McpServerFormProps {
  visible: boolean
  onSubmit: (config: McpServerConfig) => Promise<void>
  onCancel: () => void
  submitting: boolean
}
```

**表单字段：**

| 字段 | 输入类型 | 说明 |
|------|---------|------|
| 名称 (name) | text | 必填，唯一标识，如 `my-mcp-server` |
| 传输类型 (transportType) | select | `stdio` / `http` / `sse`，默认 `stdio` |
| 命令/URL (endpoint) | text | stdio 时为命令路径（如 `/usr/bin/mcp-server`），http/sse 时为 URL |
| 环境变量 (envVars) | textarea | 可选，每行 `KEY=VALUE` 格式，提交时解析为 Record |

**内联 env vars 解析：**

```typescript
function parseEnvVars(raw: string): Record<string, string> | undefined {
  if (!raw.trim()) return undefined
  return Object.fromEntries(
    raw.trim().split('\n')
      .map(line => line.split('='))
      .filter(parts => parts.length >= 2)
      .map(([k, ...rest]) => [k.trim(), rest.join('=').trim()])
  )
}
```

**布局：** 使用与 `SettingsModal` 内部 `FormGroup` + `FormInput` 相同的视觉风格（不复用，因为是跨文件，直接内联实现相同的 className 模式）。提交按钮文案"添加服务器"/"正在添加..."，取消按钮"取消"。

**验证：** name 和 endpoint 均非空时才允许提交；name 不能含空格（提示用下划线替代）。

### R3 测试

Vitest 单测 `src/components/settings/McpServerForm.test.tsx`，测试：
- `parseEnvVars` 工具函数：空字符串返回 undefined；`"K=V\nK2=V2"` 解析正确；值含 `=` 号时正确处理
- 提交时 name 为空应 disabled 提交按钮（snapshot/RTL 测试）

### R3 Commit

```
feat(mcp): add McpServerForm component with env vars parsing - R3
```

---

## Task R4：`SettingsModal` MCP tab 集成

**类型：前端，依赖 R1 R2 R3**

修改 `src/components/settings/SettingsModal.tsx`，新增 `'mcp'` tab。

### R4-1 扩展类型与 tab 栏

```typescript
type MainTab = 'account' | 'models' | 'search' | 'general' | 'persona' | 'skills' | 'mcp'
```

在 Tab 栏 `skills` 之后追加：

```tsx
<TabButton active={mainTab === 'mcp'} onClick={() => setMainTab('mcp')}>
  {t('settings.tabs.mcp')}
</TabButton>
```

### R4-2 新建 `McpTab` 组件

新建 `src/components/settings/McpTab.tsx`，将 MCP tab 内容独立成组件，避免 `SettingsModal.tsx` 继续膨胀（参考 `SkillsTab` 的拆分方式）。

`McpTab` 内部状态：
- `servers: McpServerStatus[]`
- `loading: boolean`
- `showForm: boolean`
- `submitting: boolean`
- `actionLoading: Record<string, boolean>`

加载逻辑：组件 mount 时调用 `listMcpServers()` 拉取列表。

操作实现：

```typescript
async function handleAdd(config: McpServerConfig) {
  setSubmitting(true)
  try {
    await addMcpServer(config)
    setShowForm(false)
    await reload()
    // toast success
  } catch (e) { /* toast error */ }
  finally { setSubmitting(false) }
}

async function handleConnect(name: string) {
  setActionLoading(prev => ({ ...prev, [name]: true }))
  try {
    await connectMcpServer(name)
    await reload()
    // toast success: 显示注册的 tool 数量
  } catch (e) { /* toast error */ }
  finally { setActionLoading(prev => ({ ...prev, [name]: false })) }
}

async function handleDisconnect(name: string) { /* 类似 */ }

async function handleDelete(name: string) {
  const confirmed = await ask(t('settings.mcp.confirmDelete', { name }), { title: 'AI小家', kind: 'warning' })
  if (!confirmed) return
  // removeMcpServer → reload
}
```

**UI 结构：**

```
┌─ MCP 服务器 ─────────────────────────────────────────────────────┐
│ 说明文案（MCP 允许 AI 调用外部工具...）                              │
│                                              [+ 添加服务器]       │
│ ┌── McpServerForm（visible=showForm） ───────────────────────┐    │
│ │  name | transportType | endpoint | envVars | [添加][取消]  │    │
│ └──────────────────────────────────────────────────────────┘    │
│                                                                  │
│ ┌── McpServerList ──────────────────────────────────────────┐    │
│ │  server row × N                                           │    │
│ └──────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────┘
```

### R4-3 i18n 补充

在中英文翻译文件中（通过搜索 `src/i18n/` 或 `src/locales/` 确认路径）新增：

```json
// zh-CN
"settings": {
  "tabs": {
    "mcp": "MCP"
  },
  "mcp": {
    "title": "MCP 服务器",
    "description": "MCP (Model Context Protocol) 允许 AI 调用外部工具和服务。添加 MCP server 后可在对话中使用其提供的工具。",
    "addServer": "添加服务器",
    "form": {
      "name": "服务器名称",
      "nameDesc": "唯一标识，用于区分不同 MCP server，建议使用字母数字下划线",
      "namePlaceholder": "my-mcp-server",
      "transportType": "传输类型",
      "endpoint": "命令 / URL",
      "endpointDescStdio": "stdio 类型：填写可执行命令路径，如 /usr/local/bin/mcp-server",
      "endpointDescHttp": "http/sse 类型：填写服务器 URL，如 http://localhost:3000/mcp",
      "endpointPlaceholder": "/path/to/mcp-server 或 http://...",
      "envVars": "环境变量（可选）",
      "envVarsDesc": "每行一条，格式 KEY=VALUE",
      "envVarsPlaceholder": "API_KEY=xxx\nSOME_VAR=value",
      "submit": "添加",
      "submitting": "正在添加...",
      "cancel": "取消"
    },
    "list": {
      "empty": "还没有配置 MCP 服务器",
      "emptyHint": "点击「添加服务器」配置第一个 MCP server",
      "statusConnected": "已连接",
      "statusDisconnected": "未连接",
      "connect": "连接",
      "connecting": "连接中...",
      "disconnect": "断开",
      "disconnecting": "断开中...",
      "delete": "删除",
      "tools": "{{count}} 个工具"
    },
    "confirmDelete": "确认删除 MCP 服务器「{{name}}」？已连接的 server 将先断开连接。",
    "addSuccess": "MCP 服务器已添加",
    "connectSuccess": "已连接，注册了 {{count}} 个工具",
    "disconnectSuccess": "已断开连接",
    "deleteSuccess": "MCP 服务器已删除",
    "addFailed": "添加失败",
    "connectFailed": "连接失败",
    "disconnectFailed": "断开失败",
    "deleteFailed": "删除失败"
  }
}

// en-US（对应翻译）
```

### R4 测试

手动验证：
1. SettingsModal 能打开 MCP tab，不报错
2. 空列表显示 empty state
3. 表单填写后点击"添加"调用 `addMcpServer`（可在开发模式用 DevTools 观察 IPC）
4. `pnpm build` TypeScript 检查通过

### R4 Commit

```
feat(mcp): integrate McpTab into SettingsModal with add/connect/disconnect/delete - R4
```

---

## Task R5：连接状态实时更新（可选，后端事件）

**类型：可选增强，依赖后端先发出 MCP 连接事件**

**前提：** 后端 `McpServerManager` 目前不发出任何 Tauri 事件。若要实现实时更新，需后端在 connect / disconnect 成功后通过 `RuntimeEventBus` 或直接 `app.emit()` 发出事件。

**如果后端实现了事件（建议事件名 `mcp:status-changed`）：**

在 `src/lib/tauri.ts` 新增：

```typescript
export const TAURI_EVENTS = {
  // ... 现有 events ...
  MCP_STATUS_CHANGED: 'mcp:status-changed',
} as const

export interface McpStatusChangedPayload {
  serverName: string
  connected: boolean
  registeredToolIds: string[]
}

export function onMcpStatusChanged(
  handler: (payload: McpStatusChangedPayload) => void,
): Promise<() => void> {
  return listen<McpStatusChangedPayload>(TAURI_EVENTS.MCP_STATUS_CHANGED, (event) => {
    handler(event.payload)
  })
}
```

在 `McpTab` 中：

```typescript
useEffect(() => {
  let unlisten: (() => void) | null = null
  onMcpStatusChanged((payload) => {
    setServers(prev =>
      prev.map(s =>
        s.name === payload.serverName
          ? { ...s, connected: payload.connected, registeredToolIds: payload.registeredToolIds }
          : s
      )
    )
  }).then(fn => { unlisten = fn })
  return () => { unlisten?.() }
}, [])
```

**如果后端未发出事件（当前情况）：** R5 不实现，`McpTab` 在每次操作后手动调用 `listMcpServers()` 刷新状态（已在 R4 `handleConnect` / `handleDisconnect` 中通过 `await reload()` 实现，足够用）。

### R5 Commit（若实现）

```
feat(mcp): subscribe to mcp:status-changed event for realtime status update - R5
```

---

## 实施顺序

```
R0（Rust 后端，独立完成）
  ↓
R1（tauri.ts 类型，可与 R2/R3 并行）
  ├── R2（McpServerList）
  └── R3（McpServerForm）
        ↓
       R4（McpTab + SettingsModal 集成，需 R1+R2+R3 全完成）
         ↓
        R5（可选，需后端事件支持）
```

## 文件变更汇总

### 后端（R0）
- **新建** `src-tauri/src/storage/mcp_config_store.rs`
- **新建** `src-tauri/src/transport/tauri_commands/mcp.rs`
- **修改** `src-tauri/src/transport/tauri_commands/mod.rs` — 增加 `pub mod mcp`
- **修改** `src-tauri/src/lib.rs` — 注册 `McpConfigStore`、注册 5 个新 command、startup 时预注册持久化 configs
- **新建** `src-tauri/tests/mcp_config_test.rs`

### 前端（R1–R5）
- **修改** `src/lib/tauri.ts` — 新增 MCP 类型 + 5 个 invoke 函数（R1），可选事件监听（R5）
- **新建** `src/components/settings/McpServerList.tsx` — R2
- **新建** `src/components/settings/McpServerForm.tsx` + `McpServerForm.test.tsx` — R3
- **新建** `src/components/settings/McpTab.tsx` — R4
- **修改** `src/components/settings/SettingsModal.tsx` — 新增 mcp tab — R4
- **修改** `src/i18n/zh-CN.json`（或同等路径）— 新增 `settings.mcp.*` key — R4
- **修改** `src/i18n/en-US.json`（或同等路径）— 对应英文翻译 — R4
