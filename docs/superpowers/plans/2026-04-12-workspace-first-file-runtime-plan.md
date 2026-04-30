# Workspace-First 文件能力模型 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 lotus-app 文件模型从 upload-first 改造为 workspace-first，使用户选择的本地目录成为 agent 的真实工作上下文，upload_file 退化为导入方式之一。

**Architecture:** 新增 `AuthorizedWorkspace` 概念，将用户授权的本地目录以一等对象形式注入到 `PluginContext`（真实生产链路）和 `CapabilityContext`（runtime 链路）；新增 4 个原子工具（`list_directory`、`read_workspace_file`、`search_files`、`get_file_info`），实现 `ToolPlugin` trait，通过 `register_builtin_tools()` 进入生产工具链，仅在 `authorized_workspace` 存在时对 LLM 暴露 schema；旧的 upload → load_file → execute_python 路径保持不变。

**工具名称 contract（本专项统一说法）：**

| 工具名（正式，用于 registry / skill allowlist / tests） | 上下文来源 | 作用 |
|---|---|---|
| `list_directory` | `PluginContext.authorized_workspace` | 列出授权目录内容 |
| `read_workspace_file` | `PluginContext.authorized_workspace` | 读取授权目录内的文件 |
| `search_files` | `PluginContext.authorized_workspace` | 在授权目录中 glob 搜索 |
| `get_file_info` | `PluginContext.authorized_workspace` | 获取文件/目录元数据 |

工具统一走 `ToolPlugin` + `PluginContext`（legacy 路径），不走 `RuntimeTool` + `ToolExecutionContext`（后者由专项 2 迁移）。

**Tech Stack:** Rust (Tauri backend), TypeScript/React (frontend), Tauri v2 APIs (dialog, fs), Python (sandbox), Vitest (frontend tests), Rust #[cfg(test)] + integration tests

---

## 背景

问题定义来源：`docs/2026-04-12-runtime-gap-problem-statement.md` — 专项 1

当前 lotus-app 的文件模型本质上是 upload-first：

- 用户文件先被复制到 `workspace/uploads/` 后，后续工具才围绕这些副本工作
- `SandboxConfig::for_workspace` 的 `allowed_paths` 写死为 workspace 的 7 个子目录（`sandbox.rs:70-82`）
- `load_file` 通过 `file_id → stored_path → uploads/` 解析路径，agent 从不直接接触用户原始目录（`file_load.rs:497-518`）
- `FileManager::store_upload` 强制把所有文件复制进 `uploads/`（`file_manager.rs:66-115`）
- `_find_data_file` 默认搜索模式是 `uploads/*`（`sandbox.rs:355`），把路径规则写进了 Python 代码

这导致 agent 更像"导入文件后再分析的助手"，而不是"可对本地目录进行连续工作的代理"。

---

## 一、claude-code-best Benchmark 分析

### 1.1 Claude Code 的 workspace-first 机制

**核心模块：**

- `src/utils/cwd.ts` — 工作目录的单一真相源
  - `getCwd()` 通过 `AsyncLocalStorage` 实现并发 agent 各自隔离的工作目录
  - `runWithCwdOverride(cwd, fn)` 让子 agent 在自己的 cwd 上下文中运行
  - 所有工具通过调用 `getCwd()` 获取当前工作目录，**不接受外部传入路径前缀**

- `src/tools/FileReadTool/FileReadTool.ts` — 文件读取工具
  - 接收 `file_path` 参数（相对或绝对路径）
  - 通过 `checkReadPermissionForTool()` 做权限校验
  - `getPath()` 方法返回 `file_path || getCwd()`，即文件路径解析依赖 cwd 上下文

- `src/utils/permissions/filesystem.ts` — 文件系统权限边界
  - 定义 `DANGEROUS_FILES` 和 `DANGEROUS_DIRECTORIES` 白名单保护集
  - `checkReadPermissionForTool()` 基于用户已授权的规则列表做路径检查
  - `matchingRuleForInput()` 支持 glob 匹配，授权粒度可精细到目录或文件

- `src/bootstrap/state.ts` — 原始工作目录
  - `getOriginalCwd()` 返回进程启动时的 cwd（CLI 启动目录）
  - 这是"用户当前工作上下文"的起始真相源

### 1.2 关键架构思想提炼

| 思想 | claude-code-best 实现 | 对 lotus-app 的价值 |
|------|---------------------|-------------------|
| **cwd 是一等上下文** | `getCwd()` 是全局单一真相源，所有工具通过它感知工作位置 | lotus-app 应有对应的 `AuthorizedWorkspace` 一等对象 |
| **工具原子化** | `FileReadTool`、`GlobTool`、`GrepTool` 各司其职，不混合多段操作 | lotus-app 需要 `list_directory`、`read_file` 等原子工具 |
| **路径权限在系统层** | `checkReadPermissionForTool()` 在 runtime 层校验，不在 prompt 层 | lotus-app 需要把路径规则从 `base.md` 迁移到 `CapabilityContext` |
| **并发隔离** | `AsyncLocalStorage` 使每个 agent 有独立 cwd | lotus-app 用 Session 默认 + Run 覆盖实现同等隔离 |
| **host adapter 职责** | CLI 入口把 `process.cwd()` 注入为初始上下文，tool 只消费不设置 | lotus-app 的 Tauri commands 层负责注入，runtime 只消费 |

### 1.3 不能直接照搬的部分

| claude-code-best 做法 | 不能照搬的原因 | lotus-app 的适配方案 |
|---------------------|--------------|-------------------|
| `process.cwd()` 作为初始工作目录 | lotus-app 是桌面应用，没有 CLI 入口，进程 cwd 无业务意义 | 用户通过 Tauri dialog 选择目录，持久化为 `authorized_workspace` |
| `AsyncLocalStorage` 隔离 | Rust 没有 AsyncLocalStorage | Session 默认 + Run 级别覆盖，通过 `CapabilityContext` 注入 |
| gitignore-based 文件过滤 | lotus-app 场景主要是数据分析，不是代码仓库 | 不需要 gitignore 语义，但需要文件类型过滤 |
| 运行时权限弹窗 | 复杂 UX | 用户选目录即完成授权，不需要逐文件弹窗 |
| 工具直接实现平台原生接口 | claude-code-best 工具直接调用 Node.js fs，lotus-app 需要通过 Tauri 沙箱 | 工具实现 `ToolPlugin` trait，通过 `PluginContext.workspace_path` 获取路径边界 |

---

## 二、当前问题拆解

### 2.1 upload-first 的代码证据（分层定位）

#### UI / 选择目录层

- `src/lib/tauri.ts:243-247` — `uploadFile()` 是唯一文件入口，只支持单文件上传，无"选择目录"接口
- `src/lib/tauri.ts:406-408` — `selectWorkspace()` 只用于设置 Lotus 工作区路径（生成物存储），不是用户工作目录的授权入口
- **缺失**：前端无 `authorizeDirectory()` 的"本地目录工作流"入口

#### Tauri Command 层

**关键：真实 `#[tauri::command]` 注册层在 `src-tauri/src/commands/`，不在 `transport/tauri_commands/`。**

- `src-tauri/src/commands/workspace.rs:10` — `select_workspace` 接受 `State<'_, Arc<RuntimeRepositoryFacade>>` 注入，这是真实 Tauri invoke 暴露层
- `src-tauri/src/transport/tauri_commands/workspace.rs` — 这是由 commands 层调用的 service adapter，自身不是 `#[tauri::command]`
- **缺失**：`commands/workspace.rs` 中无 `authorize_local_directory` 等命令

#### Runtime / CapabilityContext 层

- `src-tauri/src/runtime/tools/capability.rs:24-27` — `StorageCapability` 只有一个字段 `workspace_path: PathBuf`，没有"用户授权目录"的概念
- `src-tauri/src/runtime/tools/capability.rs:43-48` — `CapabilityContext` 无 `authorized_paths` 字段

#### Tool Capability 层

**关键：生产工具链的接入点是 `plugin/builtin/tools/mod.rs::register_builtin_tools()`，schema 通过 `ToolRegistry::get_schemas_filtered()` 暴露给 LLM，执行通过 `ToolRegistry::to_runtime_dispatcher()` 进入 `ToolDispatcher`。**

- **缺失**：无 `list_directory`、`read_file`（按路径）、`search_files` 等工具
- `src-tauri/src/plugin/registry.rs:112` — `get_schemas_filtered()` 遍历 `ToolPlugin` 列表生成 `ToolDefinition`，只有通过 `register()` 进入 registry 的工具才对 LLM 可见
- `src-tauri/src/plugin/registry.rs:186` — `to_runtime_dispatcher()` 为每个 `ToolPlugin` 包裹 `LegacyToolAdapter` 进入 `ToolDispatcher`

#### Python Sandbox 层

- `src-tauri/src/python/sandbox.rs:70-82` — `SandboxConfig::for_workspace` 的 `allowed_paths` 写死为 7 个 workspace 子目录，**没有机制允许用户授权的外部目录**
- `src-tauri/src/python/sandbox.rs:355-359` — `_find_data_file(pattern='uploads/*')` 把路径规则硬编码进 Python preamble
- `src-tauri/src/python/sandbox.rs:204-206` — Python 的 `os.chdir()` 设置工作目录为 `_ALLOWED_PATHS[0]`，即 workspace 根，不是用户目录

#### Store / State 层

- `src-tauri/src/storage/file_store/mod.rs:716` — `RuntimeRepositoryFacade` 是所有 runtime 存储的统一入口，包含 session/settings/memory/audit/conversation/persona/file_record store
- **缺失**：`RuntimeRepositoryFacade` 无 `authorized_workspace_store` 字段
- `src-tauri/src/storage/file_store/types.rs:64-105` — `FileEntry` 无 `source_type` 区分

---

## 三、目标态设计

### 3.1 核心概念定义

```rust
/// 用户通过 UI 授权的本地目录，Session 级别持久化，Run 级别可覆盖
pub struct AuthorizedWorkspace {
    pub id: String,           // UUID
    pub session_id: SessionId, // 绑定到哪个会话（真相源）
    pub root_path: PathBuf,   // 用户选择的本地目录绝对路径
    pub display_name: String, // UI 显示名称（目录名）
    pub authorized_at: String,
}

/// 注入到 CapabilityContext 中的轻量引用（不含 session_id）
pub struct AuthorizedWorkspaceRef {
    pub id: String,
    pub root_path: PathBuf,
    pub display_name: String,
}
```

```rust
/// FileHandle 区分文件来源（现有 FileEntry 的扩展方向，本专项不强制迁移）
pub enum FileSource {
    UploadedCopy { file_id: String, stored_path: String }, // 旧路径，兼容保留
    LocalReference { workspace_id: String, rel_path: String }, // 新路径
    Generated { file_id: String, stored_path: String },
}
```

```rust
/// 路径边界校验（核心安全原语）
pub struct AuthorizedPathScope {
    pub workspace_root: PathBuf,
}

impl AuthorizedPathScope {
    /// 规范化路径并校验不逃逸授权根目录
    pub fn resolve(&self, rel_path: &str) -> Result<PathBuf> { ... }
}
```

### 3.2 唯一决策：授权目录真相源作用域 — Session 单值覆盖

**决策：每个 `SessionId` 对应一个"当前授权目录"，重复授权时直接替换旧值（upsert 语义）。运行时每次 turn 开始时从 store 查询当前值注入进生产链路。**

**理由：**
- 用户"选择工作目录"是 session 级意图（打开一个目录，分析多轮），不是 run 级意图
- 用户可能在同一 session 内切换目录（先选 `/data-a`，再改选 `/data-b`），此时应用新目录，不累积多条记录
- 与 claude-code-best 的 `getOriginalCwd()` 语义对齐——session 起点有唯一工作目录

**store contract（单值覆盖语义）：**

```rust
pub trait AuthorizedWorkspaceStore: Send + Sync {
    // upsert：同一 session 第二次调用时替换旧记录
    fn replace_for_session(&self, ws: &AuthorizedWorkspace) -> Result<()>;
    // 查当前值（最多返回一条）
    fn get_current_for_session(&self, session_id: &SessionId) -> Result<Option<AuthorizedWorkspace>>;
    // 撤销：清空该 session 的授权
    fn clear_for_session(&self, session_id: &SessionId) -> Result<()>;
}
```

**生产链路注入点（见 3.3）：**
真实 `send_message` 主链路在 `chat_runtime_impl.rs` 中构造 `PluginContext`，注入点就在此处，而不是 `session_runtime.rs`。

**child run / sub-agent / background run 的继承语义：**
- child run 的 `PluginContext` 由父 turn 构建时传递，默认携带相同 `authorized_workspace`
- 本专项不实现 run 级别的独立目录覆盖（Not Doing）

**不做 RunId 绑定的原因：**
- RunId 级别的多工作区切换是更高级场景，列入 Not Doing

### 3.3 真实生产链路的授权目录注入

**关键结论：授权目录的注入点在 `chat_runtime_impl.rs:2596` 构造 `PluginContext` 处，不在 `session_runtime.rs`。**

真实 `send_message` 主链路：

```
src-tauri/src/commands/chat.rs::send_message
  → transport/tauri_commands/chat/chat_runtime_impl.rs::legacy_send_message_impl
    ↓ 在 chat_runtime_impl.rs:2596 构造 PluginContext
    ↓ 工具执行: tool_registry.execute(name, &plugin_ctx, input)
    ↓ schema 暴露: tool_registry.get_schemas_filtered(&ToolFilter::All)  [line:1574]
    ↓ precompute: SandboxConfig::for_workspace(&workspace_path)  [line:1642]
```

**`PluginContext` 是工具执行的真实上下文**（`plugin/context.rs` 标注了 `#[deprecated]` 但目前仍是生产唯一路径）。`session_runtime.rs` / `CapabilityContext` 是未来架构目标，**本专项的实际接线必须落在 `PluginContext` 上**。

**三条生产链路的注入方式：**

| 链路 | 代码位置 | 注入方式 |
|------|---------|---------|
| 普通工具执行 | `chat_runtime_impl.rs:2596` — `PluginContext {}` 字面量构造 | 新增 `authorized_workspace` 字段，从 facade 查 session 当前值 |
| analysis precompute | `chat_runtime_impl.rs:1642` — `SandboxConfig::for_workspace(&workspace_path)` | 改为按 session 有无授权目录决定调用哪个版本 |
| analysis precompute PluginContext | `chat_runtime_impl.rs:1651` — `auto_load_ctx` 构造 | 同样新增 `authorized_workspace` 字段 |

**注入代码示意（chat_runtime_impl.rs:2596）：**

`legacy_send_message_impl` 已有 `app: AppHandle` 参数，`RuntimeRepositoryFacade` 通过 `lib.rs:256` 注册为 `app.manage(facade)`。取法与 `agent_runtime`、`connector_engine` 完全一致：

```rust
// 新增：从 app managed state 取 facade，再查当前 session 的授权目录
// （与 connector_engine 取法一致：app.try_state::<Arc<...>>()）
let authorized_workspace: Option<AuthorizedWorkspaceRef> = app
    .try_state::<Arc<RuntimeRepositoryFacade>>()
    .and_then(|facade| {
        facade
            .authorized_workspace_store()
            .get_current_for_session(&SessionId::new(conversation_id.clone()))
            .ok()
            .flatten()
    })
    .map(|aw| AuthorizedWorkspaceRef {
        id: aw.id,
        root_path: aw.root_path,
        display_name: aw.display_name,
    });

let plugin_ctx = PluginContext {
    // ... 现有字段保持不变
    authorized_workspace,  // 新增字段
};
```

**schema 感知过滤（chat_runtime_impl.rs:~1560）也用同样方式取 facade：**

```rust
// authorized_workspace_present 从 app managed state 取，不需要额外参数
let authorized_workspace_present = app
    .try_state::<Arc<RuntimeRepositoryFacade>>()
    .and_then(|facade| {
        facade
            .authorized_workspace_store()
            .get_current_for_session(&SessionId::new(conversation_id.clone()))
            .ok()
            .flatten()
    })
    .is_some();
```

**不需要修��� `TauriChatServices` 或 `legacy_send_message_impl` 的参数列表**，因为 `app: AppHandle` 已在参数列表中，可直接通过 managed state 取到。

**schema 感知过滤规则：**

`ToolFilter::All` 当前直接拿所有 schema。workspace 工具需要在 `get_schemas_filtered` 调用处做上下文感知过滤，但该函数是无状态的（不知道当前 session 是否有授权目录）。

**解决方案：引入 `ToolFilter::AllWithContext(has_authorized_workspace: bool)` 的变体，或在 workspace 工具的 `ToolPlugin` 实现里通过"空 context 时返回空 schema"约定。**

选定方案：**在 `chat_runtime_impl.rs` 中构造 schema 列表时，检查 session 是否有授权目录，决定是否包含 workspace 工具 schema。**

```rust
// chat_runtime_impl.rs:~1560（schema 列表构造处）
// authorized_workspace_present 从 app managed state 取（见 §3.3 注入代码说明）
let authorized_workspace_present = app
    .try_state::<Arc<RuntimeRepositoryFacade>>()
    .and_then(|facade| {
        facade.authorized_workspace_store()
            .get_current_for_session(&SessionId::new(conversation_id.clone()))
            .ok().flatten()
    })
    .is_some();

let all_tool_defs = if authorized_workspace_present {
    tool_registry.get_schemas_filtered(&ToolFilter::All).await
} else {
    tool_registry.get_schemas_filtered(
        &ToolFilter::Exclude(WORKSPACE_TOOL_NAMES.to_vec())
    ).await
};
// WORKSPACE_TOOL_NAMES = ["list_directory", "read_workspace_file", "search_files", "get_file_info"]
```

**analysis precompute sandbox 注入（`:~1642`）**：`authorized_workspace` 在此处已经计算出（见上方 `:~2596` 注入点，precompute 早于工具执行，需在 precompute 前单独计算一次或提前提取）：

```rust
// chat_runtime_impl.rs:~1642（precompute sandbox 配置处）
// authorized_workspace 通过同样的 app.try_state 方式取得
let sandbox = if let Some(ref aw) = authorized_workspace {
    SandboxConfig::for_workspace_with_authorized(
        &workspace_path,
        vec![aw.root_path.clone()],
    )
} else {
    SandboxConfig::for_workspace(&workspace_path)
};
```

**实际顺序**：`authorized_workspace` 的查询提前到函数入口附近（在 schema 构造和 precompute 之前各自调用一次），每次从 managed state 取，性能可接受（KV 查一条记录）。

### 3.4 两种文件来源的角色

| 来源 | 目标架构角色 | 路径解析 | agent 消费方式 |
|------|------------|---------|-------------|
| `upload_file` → `load_file` | 导入方式之一（兼容保留） | `file_id → stored_path → workspace/uploads/` | 旧 `_df/_text` 方式不变 |
| `authorize_directory` → `list_directory` | 主工作流，一等对象 | `session_id → root_path + rel_path` | 新原子工具直接读取 |

### 3.5 各层如何消费

- **PluginContext（真实生产路径）**：新增 `authorized_workspace: Option<AuthorizedWorkspaceRef>` 字段，在 `chat_runtime_impl.rs:2596` 和 `:1651` 构造时从 `RuntimeRepositoryFacade` 注入
- **workspace ToolPlugin**：实现 `ToolPlugin` trait，从 `PluginContext.authorized_workspace` 获取路径，做 `AuthorizedPathScope::resolve()` 后只读操作文件
- **schema 感知过滤**：`chat_runtime_impl.rs` 在构造 `all_tool_defs` 时，按 session 是否有授权目录决定是否包含 workspace 工具 schema（see §3.3）
- **Python Sandbox**：`SandboxConfig::for_workspace_with_authorized(workspace, allowed_read_paths, ...)` 把授权目录加入 `allowed_read_paths`（只读语义），写权限不扩展到授权目录
- **report / export**：`generate_report` 中的 source 路径校验扩展为支持 workspace 或授权工作区内的路径

### 3.6 Python 授权目录的读写语义

**决策：授权目录只读，Lotus workspace 继续承担所有输出目录。**

**实现：** `SandboxConfig` 引入读写分离字段：

```rust
pub struct SandboxConfig {
    pub timeout_seconds: u32,
    pub memory_limit_mb: u32,
    pub allowed_read_paths: Vec<PathBuf>,  // 可读路径（原 allowed_paths 语义）
    pub allowed_write_paths: Vec<PathBuf>, // 可写路径（仅 Lotus workspace 子目录）
    pub max_output_bytes: usize,
    pub forbidden_modules: Vec<String>,
}
```

- `allowed_read_paths`：包含 Lotus workspace 的 7 个子目录 + 用户授权目录
- `allowed_write_paths`：只包含 Lotus workspace 的 7 个子目录，**不包含授权目录**

Python preamble 中 `_safe_open` 的写权限检查改为基于 `_ALLOWED_WRITE_PATHS`，读权限检查基于 `_ALLOWED_READ_PATHS`。

**Python 代码访问授权目录的标准方式：**

```python
# 通过显式变量访问，cwd 保持 Lotus workspace 不变
df = pd.read_csv(_WORKSPACE_ROOT + '/sales.csv')   # 正确
df.to_excel('exports/result.xlsx')                  # 正确（写到 Lotus workspace）
df.to_excel(_WORKSPACE_ROOT + '/output.xlsx')       # 运行时报 PermissionError（写回授权目录被拦截）
```

**cwd 语义固定：**
- Python 执行时 `os.chdir()` 继续设置为 Lotus workspace 根（`_ALLOWED_WRITE_PATHS[0]`），不改为授权目录
- 授权目录通过 `_WORKSPACE_ROOT` 环境变量访问，不通过相对路径
- golden path 中的示例统一为 `pd.read_csv(_WORKSPACE_ROOT + '/sales_2026.csv')`，不使用 `pd.read_csv('sales.csv')`

### 3.7 设计来源说明

| 设计点 | 来源 |
|--------|------|
| `AuthorizedPathScope` 的 containment 校验逻辑 | 参考 claude-code-best `filesystem.ts` 中的路径规范化 + starts_with 检查 |
| 工具原子化（list/read/search 分离） | 参考 claude-code-best 的 `FileReadTool`/`GlobTool`/`GrepTool` 分离 |
| cwd 上下文通过 context 注入，工具不直接访问全局状态 | 参考 claude-code-best `getCwd()` 的 AsyncLocalStorage 模式 |
| session 单值真相源 | 参考 claude-code-best `getOriginalCwd()`（session 唯一起点） |
| 注入点在真实 send_message 主链路 | lotus-app 特有：生产路径是 `chat_runtime_impl.rs` 构造 `PluginContext`，不是 `session_runtime.rs` |
| 读写分离的 sandbox 语义 | lotus-app 特有：用户目录只读，输出保留在 Lotus workspace |
| `ToolPlugin` 实现而非直接接 RuntimeTool | lotus-app 特有：现有生产链路通过 `ToolRegistry.execute() → LegacyToolAdapter`，本专项遵循此模式 |

---

## 四、文件级改造蓝图

### 4.0 关键接线路径总览

```
【Tauri 暴露层】
src-tauri/src/commands/workspace.rs          ← #[tauri::command] 注册，接受 State<RuntimeRepositoryFacade>
    ↓ 调用
src-tauri/src/runtime/store/authorized_workspace_store.rs  ← 通过 RuntimeRepositoryFacade 访问

【工具生产链路】
src-tauri/src/plugin/builtin/tools/mod.rs    ← register_builtin_tools() 注册新工具
    ↓ schema 暴露（感知过滤）
src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:~1560
    ← authorized_workspace_present ? ToolFilter::All : ToolFilter::Exclude(WORKSPACE_TOOL_NAMES)
    ↓ 执行
src-tauri/src/plugin/registry.rs             ← ToolRegistry::execute() → LegacyToolAdapter

【授权目录注入链路（真实生产路径）】
src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:~1560
    ← app.try_state::<Arc<RuntimeRepositoryFacade>>() → authorized_workspace_present
src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:~1642
    ← build_precompute_sandbox(app, workspace_path, conversation_id) → SandboxConfig
src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2596
    ← PluginContext { authorized_workspace, ... } → ToolRegistry::execute()
    ↓ 工具消费
src-tauri/src/plugin/builtin/tools/workspace_tools.rs  ← 从 PluginContext.authorized_workspace 读路径
```

### 需要修改的现有文件

#### 1. `src-tauri/src/commands/workspace.rs`（真实 Tauri 暴露层）

**现在职责：** `select_workspace`、`get_workspace_info` 等工作区管理命令，通过 `State<Arc<RuntimeRepositoryFacade>>` 注入

**改造内容：**
- 新增 `authorize_local_directory` 命令：接受用户选择的路径，通过 `facade.authorized_workspace_store().replace_for_session()` 持久化（覆盖旧值），返回 `AuthorizedWorkspaceRef`
- 新增 `get_authorized_workspace` 命令：通过 `facade.authorized_workspace_store().get_current_for_session(session_id)` 查询
- 新增 `revoke_authorized_workspace` 命令：通过 `facade.authorized_workspace_store().clear_for_session(session_id)` 清空

```rust
#[tauri::command]
pub async fn authorize_local_directory(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    path: String,
    session_id: String,
) -> Result<serde_json::Value, String> {
    // 校验路径存在且是目录
    let root = std::path::PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("Path is not a directory: {}", path));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let ws = AuthorizedWorkspace {
        id: id.clone(),
        session_id: SessionId::new(session_id),
        root_path: root.clone(),
        display_name: root.file_name().unwrap_or_default().to_string_lossy().to_string(),
        authorized_at: chrono::Utc::now().to_rfc3339(),
    };
    facade.authorized_workspace_store()
        .replace_for_session(&ws)  // upsert：同一 session 旧值被替换
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "id": ws.id,
        "rootPath": ws.root_path,
        "displayName": ws.display_name,
    }))
}
```

**为什么改这里而不是 `transport/tauri_commands/workspace.rs`：**
`commands/workspace.rs` 是真实的 `#[tauri::command]` 注册层，通过 `invoke_handler` 暴露给前端。`transport/tauri_commands/workspace.rs` 是一个 service adapter 结构体（`TauriWorkspaceCommandAdapter`），被 `commands/` 层调用，自身不带 `#[tauri::command]` 标注，无法直接被 `invoke()`。新命令必须落在 `commands/` 层，否则前端调用 `invoke('authorize_local_directory', ...)` 会找不到处理函数。

#### 2. `src-tauri/src/main.rs` 或 `src-tauri/src/lib.rs`（invoke_handler 注册）

**现在职责：** 注册所有 `#[tauri::command]`

**改造内容：**
- 在 `invoke_handler![]` 或 `.invoke_handler(tauri::generate_handler![...])` 中新增 `commands::workspace::authorize_local_directory`、`get_authorized_workspace`、`revoke_authorized_workspace`

**为什么改：** 新 command 函数写好后还必须在这里注册才能被 `invoke()` 调用，漏掉会在运行时报 `command not found`。

#### 3. `src-tauri/src/storage/file_store/mod.rs`（RuntimeRepositoryFacade）

**现在职责：** facade 收口所有 runtime 领域 store（session/settings/memory/audit/conversation/persona/file_record）

**改造内容：**
- 新增字段 `authorized_workspace_store: Arc<dyn AuthorizedWorkspaceStore>`
- `from_storage()` 中初始化为 `FileAuthorizedWorkspaceStore { storage: storage.clone() }`
- `for_test()` 中初始化为 `InMemoryAuthorizedWorkspaceStore::default()`
- 新增 accessor `pub fn authorized_workspace_store(&self) -> &dyn AuthorizedWorkspaceStore`

**为什么改这里：** `RuntimeRepositoryFacade` 是所有 runtime 存储的统���收口，commands 层和 session runtime 通过它访问所有 store，不允许绕过它直接 new 一个 store 实例。新增 `authorized_workspace_store` 字段遵循现有模式（与 `file_record_store`、`persona_store` 等保持一致）。

#### 4. `src-tauri/src/runtime/store/mod.rs`（store trait 集合）

**现在职责：** 定义和 re-export 所有 store trait（SessionStore、SettingsStore、MemoryStore 等）

**改造内容：**
- 新增 `AuthorizedWorkspaceStore` trait 定义

```rust
pub trait AuthorizedWorkspaceStore: Send + Sync {
    // upsert：同一 session 第二次调用时替换旧记录（单值语义）
    fn replace_for_session(&self, ws: &AuthorizedWorkspace) -> Result<()>;
    // 查当前值（最多返回一条）
    fn get_current_for_session(&self, session_id: &SessionId) -> Result<Option<AuthorizedWorkspace>>;
    // 清空该 session 的授权目录
    fn clear_for_session(&self, session_id: &SessionId) -> Result<()>;
}
```

**为什么改这里：** 与 `SessionStore`、`MemoryStore` 等 trait 保持一致的定义位置，通过 `RuntimeRepositoryFacade` 统一访问，实现可被 `InMemory*` 或 `File*` 两种版本替换（用于测试和生产）。

#### 5. `src-tauri/src/runtime/store/` 新增文件：`authorized_workspace_store.rs`

**职责：** `AuthorizedWorkspaceStore` 的两种实现

```rust
// 生产实现（file-based，通过 AppStorage）
// 存储键格式：authorized_workspace:{session_id}（单条，同 session 覆盖）
pub struct FileAuthorizedWorkspaceStore {
    storage: Arc<AppStorage>,
}

impl AuthorizedWorkspaceStore for FileAuthorizedWorkspaceStore {
    fn replace_for_session(&self, ws: &AuthorizedWorkspace) -> Result<()> {
        // 以 session_id 为键，直接覆盖旧值
        let key = format!("authorized_workspace:{}", ws.session_id.as_str());
        let value = serde_json::to_string(ws)?;
        self.storage.set_memory(&key, &value, Some("authorized_workspace"))
    }
    fn get_current_for_session(&self, session_id: &SessionId) -> Result<Option<AuthorizedWorkspace>> {
        let key = format!("authorized_workspace:{}", session_id.as_str());
        match self.storage.get_memory(&key)? {
            Some(value) => Ok(Some(serde_json::from_str(&value)?)),
            None => Ok(None),
        }
    }
    fn clear_for_session(&self, session_id: &SessionId) -> Result<()> {
        let key = format!("authorized_workspace:{}", session_id.as_str());
        self.storage.delete_memory(&key)
    }
}

// 测试用内存实现
pub struct InMemoryAuthorizedWorkspaceStore {
    // key = session_id 字符串，值为单条授权目录
    data: std::sync::Mutex<HashMap<String, AuthorizedWorkspace>>,
}
impl Default for InMemoryAuthorizedWorkspaceStore { ... }
impl AuthorizedWorkspaceStore for InMemoryAuthorizedWorkspaceStore {
    fn replace_for_session(&self, ws: &AuthorizedWorkspace) -> Result<()> {
        self.data.lock().unwrap().insert(ws.session_id.as_str().to_string(), ws.clone());
        Ok(())
    }
    fn get_current_for_session(&self, session_id: &SessionId) -> Result<Option<AuthorizedWorkspace>> {
        Ok(self.data.lock().unwrap().get(session_id.as_str()).cloned())
    }
    fn clear_for_session(&self, session_id: &SessionId) -> Result<()> {
        self.data.lock().unwrap().remove(session_id.as_str());
        Ok(())
    }
}
```

**为什么不直接建在 `storage/` 目录下：**
`storage/` 目录是 infra adapter 层（L6），直接在那里建新的 store 会绕开 `RuntimeRepositoryFacade` 这个收口。`runtime/store/` 目录是 L5（State Store 层），所有领域 store 定义在这里，通过 facade 暴露给上层，是正确的分层位置。底层实现仍然可以通过 `Arc<AppStorage>` 存储数据（与 `FileSessionStore`、`FileMemoryStore` 模式完全一致）。

#### 6. `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`（真实注入点）

**现在职责：** 生产主链路 `send_message` 的 agent loop，在 `:2596` 构造 `PluginContext`，在 `:1574` 构造 LLM 可见工具 schema 列表，在 `:1642` 配置 precompute sandbox

**改造内容（3 处）：**

**1. `:~1560`（schema 感知过滤）**：查 session 是否有授权目录，决定是否暴露 workspace 工具 schema：
```rust
let authorized_workspace_present = facade
    .authorized_workspace_store()
    .get_current_for_session(&SessionId::new(conversation_id.clone()))
    .ok().flatten().is_some();

let all_tool_defs = if authorized_workspace_present {
    tool_registry.get_schemas_filtered(&ToolFilter::All).await
} else {
    tool_registry.get_schemas_filtered(
        &ToolFilter::Exclude(WORKSPACE_TOOL_NAMES.to_vec())
    ).await
};
// WORKSPACE_TOOL_NAMES = ["list_directory", "read_workspace_file", "search_files", "get_file_info"]
```

**2. `:~2596`（工具执行 PluginContext 构造）**：注入授权目录：
```rust
let authorized_workspace: Option<AuthorizedWorkspaceRef> = facade
    .authorized_workspace_store()
    .get_current_for_session(&SessionId::new(conversation_id.clone()))
    .unwrap_or(None)
    .map(|aw| AuthorizedWorkspaceRef { id: aw.id, root_path: aw.root_path, display_name: aw.display_name });

let plugin_ctx = PluginContext {
    // ...现有字段保持不变
    authorized_workspace,  // 新增字段
};
```

**3. `:~1642`（precompute sandbox 配置）**：按是否有授权目录选择 sandbox：
```rust
let sandbox = match &authorized_workspace {
    Some(aw) => SandboxConfig::for_workspace_with_authorized(
        &workspace_path,
        vec![aw.root_path.clone()],
    ),
    None => SandboxConfig::for_workspace(&workspace_path),
};
```
同时 `auto_load_ctx`（`:~1651`）也补上 `authorized_workspace` 字段。

**为什么是这里：** 这是真实 `send_message` 主链路的工具执行入口，`PluginContext` 在这里被构造并传给 `tool_registry.execute()`。`session_runtime.rs` 是未来架构目标，不是当前生产真相源。

#### 7. `src-tauri/src/runtime/tools/capability.rs`

**现在职责：** 定义 `StorageCapability`（只有 `workspace_path`）和 `CapabilityContext`

**改造内容：**

```rust
#[derive(Clone, Debug)]
pub struct StorageCapability {
    pub workspace_path: PathBuf,
    /// 用户通过 UI 授权的本地目录（workspace-first 新增）
    pub authorized_workspace: Option<AuthorizedWorkspaceRef>,
}

#[derive(Clone, Debug)]
pub struct AuthorizedWorkspaceRef {
    pub id: String,
    pub root_path: PathBuf,
    pub display_name: String,
}

impl CapabilityContext {
    /// 原有方法保持不变（向后兼容）
    pub fn with_workspace(workspace_path: PathBuf, workspace_id: impl Into<String>) -> Self { ... }
    
    /// 新增重载
    pub fn with_workspace_and_authorized(
        workspace_path: PathBuf,
        workspace_id: impl Into<String>,
        authorized: AuthorizedWorkspaceRef,
    ) -> Self { ... }
}
```

**为什么改：** 这是 workspace-first 能力的运行时入口，工具通过此字段感知授权目录

#### 8. `src-tauri/src/plugin/builtin/tools/mod.rs`（工具生产注册入口）

**现在职责：** `register_builtin_tools()` 把所有 `ToolPlugin` 注册进 `ToolRegistry`，这是 schema 暴露和执行链路的起点

**改造内容：**

```rust
pub async fn register_builtin_tools(registry: &ToolRegistry) {
    let tools: Vec<Arc<dyn crate::plugin::ToolPlugin>> = vec![
        // ... 现有工具
        // 新增 4 个 workspace 工具
        Arc::new(workspace_tools::ListDirectoryTool),
        Arc::new(workspace_tools::ReadWorkspaceFileTool),
        Arc::new(workspace_tools::SearchFilesTool),
        Arc::new(workspace_tools::GetFileInfoTool),
    ];
    for tool in tools {
        registry.register(tool, "builtin").await;
    }
}
```

注册后自动进入：
- `ToolRegistry::get_schemas_filtered()` → LLM 可见的工具 schema
- `ToolRegistry::to_runtime_dispatcher()` → 执行链路

**为什么必须改这里：** 这是整个工具链的注册起点。只在 `runtime/tools/` 新增文件但不在这里注册，工具就是死代码——LLM 看不见、dispatcher 也找不到。这是 Finding 1 的核心修复点。

#### 9. `src-tauri/src/plugin/builtin/tools/workspace_tools.rs`（新增）

**职责：** 4 个基于授权目录的原子工具，实现 `ToolPlugin` trait

```rust
/// 工具 1: list_directory
pub struct ListDirectoryTool;

impl ToolPlugin for ListDirectoryTool {
    fn name(&self) -> &str { "list_directory" }
    fn description(&self) -> &str { "列出授权工作目录中的文件和子目录" }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "相对路径，默认为根目录 '.'", "default": "." }
            }
        })
    }
    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let aw = ctx.authorized_workspace.as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("No authorized workspace".into()))?;
        let scope = AuthorizedPathScope { workspace_root: aw.root_path.clone() };
        let rel_path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let resolved = scope.resolve(rel_path)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        // 读取目录内容并返回 JSON
        ...
    }
}

/// 工具 2: read_workspace_file
pub struct ReadWorkspaceFileTool;
// 参数: path (相对路径), max_bytes (默认 1MB)

/// 工具 3: search_files  
pub struct SearchFilesTool;
// 参数: pattern (glob), path (默认 "."), max_results (默认 100)

/// 工具 4: get_file_info
pub struct GetFileInfoTool;
// 参数: path
```

**关键：工具从 `PluginContext.authorized_workspace` 获取授权目录，而不是从 `PluginContext.workspace_path`（那是 Lotus 生成物目录）。**

**为什么实现 `ToolPlugin` 而不是 `RuntimeTool`：** 当前生产链路通过 `ToolRegistry::to_runtime_dispatcher()` 将 `ToolPlugin` 包裹为 `LegacyToolAdapter` 进入 `ToolDispatcher`。直接实现 `RuntimeTool` 需要绕过这条链路单独注册，与现有工具体系不一致，且 Finding 1 指出只改 `runtime/tools/` 而不改 `ToolRegistry` 是半接线状态。本专项遵循现有模式，后续 Atomic Tool 专项（专项 2）可统一迁移到 `RuntimeTool`。

**注意：** `PluginContext` 需要新增 `authorized_workspace: Option<AuthorizedWorkspaceRef>` 字段，在 `chat_runtime_impl.rs` 构建 `PluginContext` 时从 facade 查询注入（see §6）。

#### 10. `src-tauri/src/python/sandbox.rs`

**现在职责：** 沙箱配置，`for_workspace` 写死 7 个子目录，`_ALLOWED_PATHS` 同时控制读写权限

**改造内容（读写分离）：**

```rust
pub struct SandboxConfig {
    pub timeout_seconds: u32,
    pub memory_limit_mb: u32,
    pub allowed_read_paths: Vec<PathBuf>,  // 可读路径（原 allowed_paths 重命名）
    pub allowed_write_paths: Vec<PathBuf>, // 可写路径（只含 Lotus workspace 子目录）
    pub max_output_bytes: usize,
    pub forbidden_modules: Vec<String>,
}
```

- `for_workspace(workspace)` — 保持旧行为，`allowed_read_paths` 和 `allowed_write_paths` 都是旧 7 个子目录
- 新增 `for_workspace_with_authorized(workspace, extra_read_paths: Vec<PathBuf>)` — `allowed_read_paths` = 7 个旧路径 + 授权目录，`allowed_write_paths` = 7 个旧路径（**授权目录不在写路径中**）

Python preamble 更新：
- `_ALLOWED_PATHS` 拆分为 `_ALLOWED_READ_PATHS`（原语义）和 `_ALLOWED_WRITE_PATHS`
- `_safe_open` 的写权限检查改为检查 `_ALLOWED_WRITE_PATHS`
- 新增 `_WORKSPACE_ROOT` 变量（= 授权目录绝对路径，用于显式访问）
- `os.chdir()` 继续设置为 `_ALLOWED_WRITE_PATHS[0]`（Lotus workspace 根），**不改为授权目录**
- `_find_data_file` 的默认 pattern 保持 `uploads/*` 不变；新增 `_find_workspace_file(pattern='*')` 搜索 `_WORKSPACE_ROOT`

**为什么改：** 原 `allowed_paths` 同时控制读写，直接 append 授权目录会导致 Python 可写回用户目录，违背"授权目录只读"约束（见 §3.6）

**为什么改：** 现在授权目录在 Python 执行时无法访问，是 workspace-first 的关键卡点

#### 11. `src-tauri/src/llm/tool_executor/file_load.rs`

**现在职责：** `handle_load_file` — 通过 file_id 解析路径，加载到 Python 变量

**改造内容：**
- **不改现有 `handle_load_file` 逻辑**（保持旧路径）
- 新增 `build_local_workspace_preamble(authorized_workspace: &AuthorizedWorkspaceRef) -> String`
  - 生成 `_WORKSPACE_ROOT = '/Users/alice/data'` 的 Python 赋值语句
  - 供 `execute_python` handler 在有授权目录时追加到 preamble

**为什么改：** execute_python 需要感知授权工作区

#### 12. `src-tauri/src/llm/tool_executor/report.rs`

**现在职责：** `generate_report` — source 路径只允许在 workspace 内

**改造内容：**

```rust
// 现在：只检查 workspace_canonical
if !canonical.starts_with(&workspace_canonical) { ... }

// 改为：检查 workspace 或 authorized_workspace
let in_workspace = canonical.starts_with(&workspace_canonical);
let in_authorized = ctx.authorized_workspace
    .as_ref()
    .map(|aw| canonical.starts_with(&aw.root_path))
    .unwrap_or(false);
if !in_workspace && !in_authorized {
    return Err(anyhow::anyhow!("Source path outside allowed scope"));
}
```

#### 13. `src/lib/tauri.ts`

**改造内容：**
- 新增 `authorizeLocalDirectory(path: string, sessionId: string): Promise<AuthorizedWorkspaceRef>`
- 新增 `getAuthorizedWorkspace(sessionId: string): Promise<AuthorizedWorkspaceRef | null>`
- 新增 `revokeAuthorizedWorkspace(sessionId: string): Promise<void>` — 按 **sessionId** 撤销（与后端 `clear_for_session` 一致，不是按 id）
- **不新增** `workspace:authorized` 事件（当前无需后端主动推送，UI 在用户触发授权后由前端同步更新状态即可）

#### 14. `src/components/settings/SettingsModal.tsx` / 相关 UI 组件

**改造内容：**
- 新增"本地工作目录"区块，提供"选择目录"按钮和"撤销授权"按钮

---

## 五、分期方案

### Phase W1：AuthorizedWorkspace 领域模型 + Tauri 命令接线

**目标：** 用户可以通过 UI 授权一个本地目录，系统能持久化和查询，前后端 IPC 全链路打通。

**改哪些文件：**
- 新增 `src-tauri/src/runtime/store/authorized_workspace_store.rs` — trait + File/InMemory 两种实现
- 修改 `src-tauri/src/runtime/store/mod.rs` — re-export `AuthorizedWorkspaceStore`
- 修改 `src-tauri/src/storage/file_store/mod.rs` — `RuntimeRepositoryFacade` 新增字段和 accessor
- 修改 `src-tauri/src/commands/workspace.rs` — 新增 3 个 `#[tauri::command]`
- 修改 `src-tauri/src/main.rs` 或 `lib.rs` — 在 `invoke_handler!` 中注册新命令
- 修改 `src-tauri/src/runtime/tools/capability.rs` — `StorageCapability` 新增 `authorized_workspace` 字段
- 修改 `src/lib/tauri.ts` — 新增 3 个 IPC wrapper

**兼容边界：**
- 现有 `select_workspace`、`upload_file`、`load_file` 完全不变
- `RuntimeRepositoryFacade::for_test()` 新增字段用 `InMemoryAuthorizedWorkspaceStore` 默认初始化

**本期不做：**
- 不新增工具，不修改 Python sandbox，不修改 report.rs

**完成标志：**
- `invoke('authorize_local_directory', { path: '/tmp/test', sessionId: 'sess-1' })` 调用成功，通过 `get_authorized_workspace` 能查回
- 集成测试：`test_replace_and_get_for_session` PASS
- 集成测试：`test_replace_overwrites_previous` PASS（同一 session 重复授权后只保留最新）
- 集成测试：`test_session_isolation` PASS

---

### Phase W2：4 个原子工具接入生产工具链

**目标：** agent 可通过新工具列出、读取、搜索授权目录内的文件，工具通过正式注册链路对 LLM 可见并可执行。

**改哪些文件：**
- 新增 `src-tauri/src/plugin/builtin/tools/workspace_tools.rs` — 4 个 `ToolPlugin` 实现
- 修改 `src-tauri/src/plugin/builtin/tools/mod.rs` — `register_builtin_tools()` 注册 4 个新工具
- 修改 `src-tauri/src/storage/file_manager.rs` — 新增 `resolve_local_reference` + `is_within_authorized_workspace`
- 修改 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` — 新增 `build_visible_tool_defs` helper（schema 感知过滤）+ 在 `:~2596` 注入 `authorized_workspace` 到 `PluginContext`
- 修改 `src-tauri/src/plugin/context.rs` — `PluginContext` 新增 `authorized_workspace: Option<AuthorizedWorkspaceRef>` 字段

**兼容边界：**
- 4 个新工具只在 `authorized_workspace` 非 None 时返回有意义结果，否则返回明确错误
- 旧工具（`upload_file`、`load_file`、`execute_python`）完全不变
- 无授权目录的会话，`all_tool_defs` 自动排除 4 个 workspace 工具（通过 §3.3 的感知过滤）

**本期不做：**
- 不修改 Python sandbox，不修改 report.rs，不修改前端 UI

**完成标志：**
- 单测：`test_path_traversal_rejected` PASS
- 单测：`test_list_directory_requires_authorized_workspace` PASS
- 集成测试：`test_list_directory_golden_path` PASS
- **关键验证**：`test_build_visible_tool_defs_with_authorized_workspace` + `test_build_visible_tool_defs_without_authorized_workspace` 均 PASS（打到真实 `build_visible_tool_defs` helper）
- **关键验证**：`test_workspace_tools_dispatchable_via_registry` PASS

---

### Phase W3：Python Sandbox 扩展 + execute_python + analysis precompute 感知授权目录

**目标：** `execute_python` 和 `analysis precompute` 都能读取授权目录内的文件，Python 代码通过 `_WORKSPACE_ROOT` 显式访问，`cwd` 保持 Lotus workspace 不变，授权目录只读。

**改哪些文件：**
- 修改 `src-tauri/src/python/sandbox.rs` — 读写分离字段 + `for_workspace_with_authorized`
- 修改 `src-tauri/src/llm/tool_executor/file_load.rs` — 新增 `build_local_workspace_preamble`（注入 `_WORKSPACE_ROOT`）
- 修改 `src-tauri/src/llm/tool_executor/python.rs`（execute_python handler）— 有授权目录时用 `for_workspace_with_authorized` + 追加 local workspace preamble
- 修改 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1642` — precompute sandbox 按授权目录选择版本（§3.3 已描述）

**兼容边界：**
- 旧 `SandboxConfig::for_workspace` 保留，`allowed_read_paths` 和 `allowed_write_paths` 均为旧 7 路径（行为不变）
- `_find_data_file` 默认 pattern 保持 `uploads/*` 不变；新增 `_find_workspace_file` 搜索 `_WORKSPACE_ROOT`
- Python `cwd` 继续为 Lotus workspace 根，不改为授权目录

**本期不做：**
- 不修改前端 UI，不修改 report.rs（Phase W4 做）

**完成标志：**
- 单测：`for_workspace_with_authorized` 的 `allowed_read_paths` 包含授权目录，`allowed_write_paths` **不包含**授权目录
- 集成测试：授权目录 `/tmp/test_data/` → `execute_python` 中 `pd.read_csv(_WORKSPACE_ROOT + '/sales.csv')` PASS
- 集成测试：`df.to_excel(_WORKSPACE_ROOT + '/output.xlsx')` 运行时触发 `PermissionError`（写回被拦截）
- **precompute sandbox 行为测试**：`test_build_precompute_sandbox_with_authorized_workspace` + `test_build_precompute_sandbox_without_authorized_workspace` 均 PASS（打到真实 `build_precompute_sandbox` helper）
- 回归测试：旧 `upload_file → load_file → execute_python` 流程 PASS

---

### Phase W4：前端 UI + report.rs 扩展 + 完整联调验收

**目标：** 完整主链路可跑：选目录 → agent 识别 → 读取分析 → 生成报告。

**改哪些文件：**
- 新增或修改 `src/components/settings/WorkspaceAuthPanel.tsx` — 目录选择和授权 UI
- 修改 `src/components/settings/SettingsModal.tsx` — 引入 WorkspaceAuthPanel
- 修改 `src-tauri/src/llm/tool_executor/report.rs` — 扩展路径校验
- **注意：** W4 的 UI 目录浏览功能**不新增** `listAuthorizedDirectory` IPC wrapper。UI 展示已授权目录通过 `get_authorized_workspace` 查已有数据即可；目录内容浏览通过 agent 工具（`list_directory`）完成，不走额外 IPC。如后续需要纯 UI 文件树，再在 W4+ 阶段单独增加后端命令合同（命令名、参数、返回结构、权限边界、测试均需完整定义）。

**兼容边界：**
- 旧上传流程 UI 入口保留，report 路径校验新路径 OR 旧路径均可

**本期不做：**
- 不做多工作区并发授权

**完成标志：**
- Golden path 完整通过（见验收标准）
- Backward compatibility path 完整通过

---

## 六、TDD / 回归策略

### 6.1 先写哪些失败测试（Red 阶段）

**Phase W1 — 领域 store 测试**

文件：`src-tauri/src/runtime/store/authorized_workspace_store.rs` 底部 `#[cfg(test)]`

```rust
#[test]
fn test_replace_and_get_for_session() {
    // 失败原因：replace_for_session / get_current_for_session 还不存在
    let store = InMemoryAuthorizedWorkspaceStore::default();
    let ws = AuthorizedWorkspace {
        session_id: SessionId::new("sess-1"),
        root_path: PathBuf::from("/tmp/test"),
        ..Default::default()
    };
    store.replace_for_session(&ws).unwrap();
    let found = store.get_current_for_session(&SessionId::new("sess-1")).unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().root_path, PathBuf::from("/tmp/test"));
}

#[test]
fn test_replace_overwrites_previous() {
    // 证明同一 session 重复授权时旧值被替换（单值语义）
    let store = InMemoryAuthorizedWorkspaceStore::default();
    store.replace_for_session(&make_ws("sess-1", "/tmp/data-a")).unwrap();
    store.replace_for_session(&make_ws("sess-1", "/tmp/data-b")).unwrap();
    let found = store.get_current_for_session(&SessionId::new("sess-1")).unwrap().unwrap();
    assert_eq!(found.root_path, PathBuf::from("/tmp/data-b")); // 旧值被替换
}

#[test]
fn test_session_isolation() {
    // 证明 A session 的授权不被 B session 看到
    let store = InMemoryAuthorizedWorkspaceStore::default();
    store.replace_for_session(&make_ws("sess-a", "/tmp/a")).unwrap();
    let found = store.get_current_for_session(&SessionId::new("sess-b")).unwrap();
    assert!(found.is_none());
}
```

**Phase W1 — Facade 集成**

文件：`src-tauri/tests/authorized_workspace_facade_test.rs`（新增）

```rust
#[test]
fn test_facade_exposes_authorized_workspace_store() {
    // 失败原因：RuntimeRepositoryFacade 还没有 authorized_workspace_store 字段
    let facade = RuntimeRepositoryFacade::for_test();
    let store = facade.authorized_workspace_store();
    let ws = make_ws("sess-1", "/tmp/test");
    store.replace_for_session(&ws).unwrap();
    assert!(store.get_current_for_session(&SessionId::new("sess-1")).unwrap().is_some());
}
```

**Phase W2 — 路径边界测试**

文件：`src-tauri/src/plugin/builtin/tools/workspace_tools.rs` 底部 `#[cfg(test)]`

```rust
#[test]
fn test_path_traversal_rejected() {
    // 失败原因：AuthorizedPathScope 还不存在
    let scope = AuthorizedPathScope { workspace_root: PathBuf::from("/tmp/authorized") };
    let result = scope.resolve("../secret");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("traversal"));
}

#[test]
fn test_valid_path_within_scope_accepted() {
    let scope = AuthorizedPathScope { workspace_root: PathBuf::from("/tmp/authorized") };
    let _ = std::fs::create_dir_all("/tmp/authorized/subdir").unwrap();
    let result = scope.resolve("subdir/file.csv");
    assert!(result.is_ok());
}
```

**Phase W2 — 证明工具进入生产链路 + schema 感知（关键！）**

文件：`src-tauri/tests/workspace_tool_registry_test.rs`（新增）

```rust
#[tokio::test]
async fn test_workspace_tools_registered_in_tool_registry() {
    // 失败原因：register_builtin_tools 还没有注册新工具
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;
    let all = registry.get_schemas_filtered(&ToolFilter::All).await;
    let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"list_directory"));
    assert!(names.contains(&"read_workspace_file"));
    assert!(names.contains(&"search_files"));
    assert!(names.contains(&"get_file_info"));
}

#[tokio::test]
async fn test_workspace_tools_dispatchable_via_registry() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;
    let plugin_ctx = make_test_plugin_ctx_with_authorized("/tmp/test");
    let dispatcher = registry.to_runtime_dispatcher(plugin_ctx).await;
    let result = dispatcher.dispatch("list_directory", json!({"path": "."}), make_test_tool_ctx()).await;
    match result {
        Err(ToolError::ExecutionFailed(msg)) => assert!(!msg.contains("unknown tool"), "tool not registered: {}", msg),
        _ => {}
    }
}
```

**Phase W2 — schema 感知真实行为测试（Finding 11 升级版）**

抽取 `build_visible_tool_defs` 为独立 helper，测试打到真实业务函数而非只测 `ToolFilter::Exclude` API：

文件：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`（新增 helper）

```rust
/// 根据当前 session 是否有授权目录，决定暴露给 LLM 的工具 schema 列表。
/// 独立为 helper 以便测试。
pub(crate) async fn build_visible_tool_defs(
    registry: &ToolRegistry,
    has_authorized_workspace: bool,
) -> Vec<ToolDefinition> {
    if has_authorized_workspace {
        registry.get_schemas_filtered(&ToolFilter::All).await
    } else {
        registry.get_schemas_filtered(
            &ToolFilter::Exclude(WORKSPACE_TOOL_NAMES.to_vec())
        ).await
    }
}
```

文件：`src-tauri/tests/workspace_tool_registry_test.rs`（追加）

```rust
#[tokio::test]
async fn test_build_visible_tool_defs_with_authorized_workspace() {
    // 有授权目录时，helper 返回包含 workspace 工具的 schema 列表
    // 失败原因：build_visible_tool_defs 还不存在
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let defs = build_visible_tool_defs(&registry, true).await;
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"list_directory"), "workspace tool absent when authorized");
}

#[tokio::test]
async fn test_build_visible_tool_defs_without_authorized_workspace() {
    // 无授权目录时，helper 返回不含 workspace 工具的 schema 列表
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let defs = build_visible_tool_defs(&registry, false).await;
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(!names.contains(&"list_directory"), "workspace tool present without authorization");
    assert!(!names.contains(&"read_workspace_file"));
    assert!(!names.contains(&"search_files"));
    assert!(!names.contains(&"get_file_info"));
}
```

`chat_runtime_impl.rs` 主路径调用 `build_visible_tool_defs(&tool_registry, authorized_workspace_present).await`，使测试覆盖真实调用点。

**Phase W3 — Sandbox 读写分离测试**

文件：`src-tauri/src/python/sandbox.rs` 底部 `#[cfg(test)]` 新增：

```rust
#[test]
fn test_for_workspace_with_authorized_read_path_includes_authorized() {
    let workspace = PathBuf::from("/tmp/lotus");
    let extra = vec![PathBuf::from("/Users/user/data")];
    let config = SandboxConfig::for_workspace_with_authorized(&workspace, extra);
    assert!(config.allowed_read_paths.contains(&PathBuf::from("/Users/user/data")));
    assert!(!config.allowed_write_paths.contains(&PathBuf::from("/Users/user/data")));
    assert!(config.allowed_read_paths.contains(&workspace.join("uploads")));
    assert!(config.allowed_write_paths.contains(&workspace.join("uploads")));
}

#[test]
fn test_old_for_workspace_unchanged() {
    let workspace = PathBuf::from("/tmp/lotus");
    let config = SandboxConfig::for_workspace(&workspace);
    assert_eq!(config.allowed_read_paths.len(), 7);
    assert_eq!(config.allowed_write_paths.len(), 7);
}
```

**Phase W3 — precompute sandbox 真实行为测试（Finding 12 升级版）**

抽取 `build_precompute_sandbox` 为独立 helper，测试打到业务逻辑，不只测 `for_workspace_with_authorized` 内部：

文件：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`（新增 helper）

```rust
/// 根据是否有授权目录，构建 analysis precompute 阶段使用的 SandboxConfig。
/// 独立为 helper 以便测试验证分支逻辑。
pub(crate) fn build_precompute_sandbox(
    workspace_path: &PathBuf,
    authorized_workspace: Option<&AuthorizedWorkspaceRef>,
) -> SandboxConfig {
    match authorized_workspace {
        Some(aw) => SandboxConfig::for_workspace_with_authorized(
            workspace_path,
            vec![aw.root_path.clone()],
        ),
        None => SandboxConfig::for_workspace(workspace_path),
    }
}
```

文件：`src-tauri/tests/workspace_golden_path_test.rs`（追加）

```rust
#[test]
fn test_build_precompute_sandbox_with_authorized_workspace() {
    // 有授权目录时，precompute sandbox 的 allowed_read_paths 包含授权目录
    // 失败原因：build_precompute_sandbox 还不存在
    let workspace = PathBuf::from("/tmp/lotus");
    let aw = AuthorizedWorkspaceRef {
        id: "test".to_string(),
        root_path: PathBuf::from("/tmp/test_data"),
        display_name: "test".to_string(),
    };
    let config = build_precompute_sandbox(&workspace, Some(&aw));
    assert!(config.allowed_read_paths.contains(&PathBuf::from("/tmp/test_data")));
    assert!(!config.allowed_write_paths.contains(&PathBuf::from("/tmp/test_data")));
}

#[test]
fn test_build_precompute_sandbox_without_authorized_workspace() {
    // 无授权目录时，precompute sandbox 只有旧 7 个路径
    let workspace = PathBuf::from("/tmp/lotus");
    let config = build_precompute_sandbox(&workspace, None);
    assert_eq!(config.allowed_read_paths.len(), 7);
    assert!(!config.allowed_read_paths.iter().any(|p| p.starts_with("/tmp/test_data")));
}
```

`chat_runtime_impl.rs` 主路径调用 `build_precompute_sandbox(&workspace_path, authorized_workspace.as_ref())`，使测试覆盖真实调用点。

### 6.2 集成测试

文件：`src-tauri/tests/workspace_golden_path_test.rs`（新增）

```rust
#[tokio::test]
async fn test_list_directory_golden_path() {
    let test_dir = tempfile::tempdir().unwrap();
    std::fs::write(test_dir.path().join("sales.csv"), "date,amount\n2026-01,100\n").unwrap();

    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;
    let plugin_ctx = make_test_plugin_ctx_with_authorized(test_dir.path().to_path_buf());
    let dispatcher = registry.to_runtime_dispatcher(plugin_ctx).await;

    let result = dispatcher.dispatch("list_directory", json!({"path": "."}), make_test_tool_ctx()).await.unwrap();
    let parsed: Value = serde_json::from_str(&result.result.content).unwrap();
    let files = parsed["files"].as_array().unwrap();
    assert!(files.iter().any(|f| f["name"].as_str() == Some("sales.csv")));
}

#[tokio::test]
async fn test_old_upload_path_still_works() {
    // 确保旧上传流程未回归：SandboxConfig::for_workspace 不受影响
    let workspace = PathBuf::from("/tmp/test_workspace");
    let config = SandboxConfig::for_workspace(&workspace);
    assert_eq!(config.allowed_paths.len(), 7);
    assert!(config.allowed_paths.contains(&workspace.join("uploads")));
}
```

### 6.3 前端测试

文件：`src/lib/tauri.test.ts`（新增或扩展）

```typescript
describe('workspace authorization', () => {
  it('authorizeLocalDirectory sends correct IPC command', async () => {
    const mockInvoke = vi.fn().mockResolvedValue({ id: 'aw-1', rootPath: '/tmp/test', displayName: 'test' })
    vi.spyOn(tauriCore, 'invoke').mockImplementation(mockInvoke)
    
    await authorizeLocalDirectory('/tmp/test', 'sess-1')
    
    expect(mockInvoke).toHaveBeenCalledWith('authorize_local_directory', {
      path: '/tmp/test',
      sessionId: 'sess-1',
    })
  })
})
```

---

## 七、验收路径

### 7.1 问题文档验收标准 → 可执行检查项

| 原始验收标准 | 怎么验证 |
|------------|---------|
| 用户可以选择任意本地目录，不需要先把文件复制进 uploads/ | 运行 Phase W4 golden path，确认 `uploads/` 下没有新增文件 |
| agent 可以列出目录内容 | `test_workspace_tools_registered_in_tool_registry` PASS（工具进 LLM）+ `test_list_directory_golden_path` PASS（工具可执行） |
| agent 可以读取指定文本或结构化文件 | `test_workspace_tools_dispatchable_via_registry` 中 `read_workspace_file` 可被 dispatch |
| agent 可以在目录中搜索目标文件 | `search_files` 工具在 registry 中且可 dispatch |
| agent 可以基于目录中的文件直接分析并输出结果 | Phase W3 集成测试：`execute_python` 中 `pd.read_csv` 访问授权目录成功 |
| 安全边界仍成立 | `test_path_traversal_rejected` PASS，`test_session_isolation` PASS |
| 现有上传流程不能回归 | `test_old_upload_path_still_works` PASS，`test_old_for_workspace_unchanged` PASS |

### 7.2 Golden Path：本地目录工作流

```
步骤 1: 用户在 UI 点击"选择工作目录"
  → 调用 Tauri dialog 选择 /Users/alice/reports/
  → invoke('authorize_local_directory', { path: '/Users/alice/reports/', sessionId: 'sess-1' })
    [commands/workspace.rs::authorize_local_directory]
    → facade.authorized_workspace_store().save(ws)
  → 后端返回 { id: 'aw-1', rootPath: '/Users/alice/reports/', displayName: 'reports' }
  → UI 显示"当前工作目录：reports"

步骤 2: 用户发消息"帮我分析这个目录里的销售数据"
  → session_runtime::execute_turn() 查 facade.authorized_workspace_store().get_by_session()
  → 注入 CapabilityContext.storage.authorized_workspace = { root: /Users/alice/reports/ }
  → 注入 PluginContext.authorized_workspace 同值
  → agent 调用 list_directory({ path: "." })
    [ToolRegistry::to_runtime_dispatcher() → LegacyToolAdapter → ListDirectoryTool::execute()]
    → AuthorizedPathScope::resolve(".") → /Users/alice/reports/
    → 返回 ["sales_2026.csv", "report.xlsx", "notes.txt"]

步骤 3: agent 调用 read_workspace_file({ path: "sales_2026.csv" })
  → AuthorizedPathScope::resolve("sales_2026.csv") → /Users/alice/reports/sales_2026.csv
  → 校验通过，返回文件内容前 1000 行

步骤 4: agent 调用 execute_python
  → SandboxConfig::for_workspace_with_authorized(lotus_ws, read_paths=[/Users/alice/reports/])
  → allowed_write_paths 不含 /Users/alice/reports/（只读语义；cwd 保持 Lotus workspace 根）
  → Python preamble 注入 _WORKSPACE_ROOT = '/Users/alice/reports'
  → Python 代码：df = pd.read_csv(_WORKSPACE_ROOT + '/sales_2026.csv')  ← 显式路径，不用相对路径
  → 分析成功；df.to_excel('exports/result.xlsx') 写到 Lotus workspace（正确）

步骤 5: agent 调用 generate_report
  → report.rs：canonical 在 authorized_workspace 内 → 校验通过
  → 生成报告到 Lotus workspace/reports/
```

### 7.3 关键接线验证 Checklist

```
Phase W1:
[ ] commands/workspace.rs 中有 authorize_local_directory 且带 #[tauri::command]
[ ] lib.rs/main.rs 的 invoke_handler![] 包含新 3 个命令
[ ] RuntimeRepositoryFacade 有 authorized_workspace_store 字段和 accessor
[ ] RuntimeRepositoryFacade::for_test() 初始化 InMemoryAuthorizedWorkspaceStore
[ ] store trait 用 replace_for_session/get_current_for_session/clear_for_session
[ ] 前端 revokeAuthorizedWorkspace 参数为 sessionId（不是 id）
[ ] 无 workspace:authorized 事件声明（已从方案中删除）

Phase W2:
[ ] register_builtin_tools() 包含 4 个新工具，名称与 contract 表一致
[ ] chat_runtime_impl.rs 提取了 build_visible_tool_defs(registry, has_authorized) helper
[ ] test_build_visible_tool_defs_with/without_authorized_workspace 均 PASS（打到真实 helper）
[ ] PluginContext 有 authorized_workspace 字段
[ ] chat_runtime_impl.rs:~2596 从 app.try_state::<Arc<RuntimeRepositoryFacade>>() 取 facade
[ ] ListDirectoryTool 从 PluginContext.authorized_workspace 读路径，不从 workspace_path

Phase W3:
[ ] chat_runtime_impl.rs 提取了 build_precompute_sandbox(workspace, authorized) helper
[ ] test_build_precompute_sandbox_with/without_authorized_workspace 均 PASS（打到真实 helper）
[ ] SandboxConfig 有 allowed_read_paths 和 allowed_write_paths（读写分离）
[ ] execute_python handler 有授权时用 for_workspace_with_authorized
[ ] Python 中 _WORKSPACE_ROOT 是用户授权目录；cwd 是 Lotus workspace（不变）
[ ] Python 写回授权目录触发 PermissionError（负向测试 PASS）
```

### 7.4 Backward Compatibility Path：旧上传流程

```
步骤 1: 用户拖拽文件 sales.csv 到聊天窗口
  → invoke('upload_file', ...) → 复制到 workspace/uploads/uuid_sales.csv → 返回 fileId

步骤 2: agent 调用 load_file({ file_id: fileId })
  → file_load.rs 通过 DB 查 stored_path → 返回 _df 元数据（完全不变）

步骤 3: agent 调用 execute_python（无授权目录）
  → SandboxConfig::for_workspace（旧函数，7 个路径）
  → Python 中 _df 直接可用（旧 preamble 注入，完全不变）

步骤 4: 分析成功
```

---

## 八、风险与回滚

### 8.1 最容易引发的回归

1. **新工具默认全部对 LLM 可见，可能影响现有 daily skill 的工具过滤**
   - 防护：`ToolFilter::Only(names)` 的 skill 配置不受影响（filter 模式下只暴露指定工具）；检查 daily skill 的 `tool_filter` 配置是否用了 `All` 模式

2. **`register_builtin_tools()` 里的新工具名称与现有工具重名**
   - 防护：注册时 `ToolRegistry::register()` 有 shadowing 保护，会发 warn 并拒绝；选择唯一名称（`list_directory`、`read_workspace_file`、`search_files`、`get_file_info`）

3. **`allowed_paths` 被意外扩展**：`for_workspace_with_authorized` 的 `extra_paths` 传入空列表时行为需与旧版一致
   - 防护：`for_workspace_with_authorized` 永远先调用 `for_workspace` 再 extend，即使 `extra_paths` 为空也不少于 7 个

4. **path containment 校验过于严格**：`canonicalize` 在文件不存在时失败
   - 防护：参考 `file_manager.rs:40-51` 已有逻辑，对不存在的路径做 parent canonicalize

5. **session 隔离打破**：`get_by_session` 实现错误，导致 A session 授权被 B session 看到
   - 防护：`test_session_isolation` 单测覆盖

### 8.2 如何避免"表面支持目录，实际底层仍然只认 uploads"

审查 checklist（见 7.3）中的关键项：
- workspace_tools 读路径来自 `PluginContext.authorized_workspace`，不是 `workspace_path`
- `execute_python` handler 有 `if let Some(aw) = ctx.authorized_workspace` 分支调用新 sandbox config
- Python 中 `_WORKSPACE_ROOT` 是用户授权目录

### 8.3 改到一半如何保持系统可运行

- Phase W1 结束：旧系统 100% 不受影响，只新增了 store + 3 个新命令
- Phase W2 结束：新工具加入 registry，但不存在授权目录时工具返回清晰错误，旧工具不变
- Phase W3 结束：sandbox 有两条路径，无授权目录时 fallback 到旧行为（`for_workspace` 分支）

**回滚方式：**
- 每个 Phase 在独立 git branch
- Phase W2 的新工具若有 bug，在 `register_builtin_tools()` 注释掉对应注册即可退回
- Phase W3 出问题，删除 `execute_python` handler 中的新分支，退回旧 sandbox config

---

## 九、Not Doing

1. **多工作区并发授权**：每个 session 只支持一个授权目录（单值覆盖语义）
2. **RunId 级别独立授权**：child run / background run 默认继承 session 授权目录；run 级别覆盖留待后续扩展
3. **文件写回用户目录**：分析结果仍然写到 Lotus workspace 的 exports/reports，Python 写回授权目录会被 `_safe_open` 拦截
4. **目录权限细粒度控制**：选目录即完成授权，不提供子目录粒度
5. **iCloud/网络路径支持**：只支持本地文件系统
6. **Atomic Tool 体系重构（专项 2）**：新工具先实现 `ToolPlugin`，后续专项 2 统一迁移到 `RuntimeTool`
7. **Prompt Slimming（专项 3）**：不修改 `base.md`/`daily.md`
8. **`transport/tauri_commands/workspace.rs` 的 `TauriWorkspaceCommandAdapter`**：不新增方法到这个 adapter，新逻辑直接落在 `commands/workspace.rs`
9. **`listAuthorizedDirectory` IPC**：W4 不新增这个 wrapper；UI 目录浏览不走额外 IPC，通过 `get_authorized_workspace` 查已有状态，内容浏览通过 agent 工具完成

---

*文档版本：2026-04-12 v4（第二轮 review findings 9-14 修订）| 对标文档：`docs/2026-04-12-runtime-gap-problem-statement.md` 专项 1*
