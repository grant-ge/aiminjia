# 原子工具能力升级设计文档

**日期**：2026-04-17  
**状态**：草稿  
**来源**：`docs/2026-04-17-atomic-tool-vs-claude-code-best-gap.md` 中 P0-A/B/C、P1-A/B、P2-A 六项差距  
**对标**：`/Users/a20250311/github/claude-code-best` — `ToolUseContext`、`toolOrchestration.ts`、`bashPermissions.ts`

---

## 一、目标

将 lotus-app 工具系统从当前"工具是无状态孤岛"的状态，升级到工具可以感知 session 状态、支持并发执行、并能基于具体输入动态决策权限的状态。

**验收标准**：
1. `CapabilityContext` 携带 abort signal、通知回调、对话消息（Phase 1）
2. 工具返回有序 schema（按 id 字典序），prompt cache 命中率不再因顺序变化而下降（Phase 1）
3. 并发安全的只读工具可并行执行，非并发安全工具串行（Phase 2）
4. 工具可通过 `check_permissions` 基于具体输入内容动态决策（Phase 3）
5. `execute_python`/`generate_report`/`generate_chart` 完全脱离 `LegacyToolAdapter`（Phase 3）

---

## 二、Phase 1 — 基础层（无外部依赖）

### P2-A：Tool Pool 排序

**问题**：`get_schemas_filtered()` 内部用 `HashMap` 迭代，返回顺序不确定，每次 API call 工具列表顺序不同 → Anthropic prompt cache key 变化 → cache miss。

**改动**：在 `plugin/registry.rs` 的 `get_all_schemas()` 和 `get_schemas_filtered()` 两个函数末尾，对 `schemas: Vec<ToolDefinition>` 按 `name` 字段做字典序排序后返回。

```rust
// 改动点：plugin/registry.rs get_schemas_filtered() 末尾
schemas.sort_by(|a, b| a.name.cmp(&b.name));
schemas
```

**测试**：`tests/tool_catalog_contract_test.rs` 新增 `filtered_schemas_are_sorted_by_name`——构造包含乱序 tool id 的 `ToolFilter::Only`，断言返回结果按字典序排列。

---

### P0-A：CapabilityContext 扩展

**问题**：`CapabilityContext` 只有 4 个字段，工具无法利用文件状态缓存（导致重复读同一文件）、无法感知读取限制、无法向前端推送通知。

**对标 claude-code-best 实际使用**（来自 FileReadTool/BashTool/GlobTool/GrepTool 源码调研）：

| claude-code-best 字段 | 实际使用工具 | lotus-app 对应 |
|----------------------|------------|---------------|
| `abortController` | 全部 4 个工具 | **已有**：`ToolExecutionContext.cancellation`，无需重复加入 CapabilityContext |
| `readFileState: FileStateCache` | FileReadTool、BashTool(sed) | **缺失**：需新增 `read_file_state` |
| `fileReadingLimits` | FileReadTool | **缺失**：需新增 `file_reading_limits` |
| `globLimits` | GlobTool | 已有：`search_files` 参数 `max_results` 覆盖，暂不重复 |
| `setToolJSX` | BashTool | **缺失**：需新增 `notification_sink` |
| `getAppState()` | 全部 4 个工具（权限上下文） | `CapabilityPermissionPipeline` 覆盖，工具不直接访问 |
| `contentReplacementState` | 由 query loop 管理 | 暂不加入，Phase 2+ 考虑 |

**新增字段**（全部 `Option`，向后兼容，不破坏现有测试）：

```rust
pub struct CapabilityContext {
    // 现有字段不变
    pub storage: Option<StorageCapability>,
    pub workspace_id: Option<String>,
    pub browser_available: bool,
    pub file_ops: Option<Arc<dyn FileOperations>>,

    // 新增字段（对标 claude-code-best ToolUseContext）

    /// 文件状态缓存——防止重复读取未修改的文件。
    /// 对应 claude-code-best `readFileState: FileStateCache`（LRU cache by path）。
    /// 工具在读取文件后写入缓存，下次读取同文件同范围时检查 mtime，未修改则返回缓存。
    pub read_file_state: Option<Arc<FileStateCache>>,

    /// 文件读取限制——防止超大文件撑满 LLM 上下文窗口。
    /// 对应 claude-code-best `fileReadingLimits: { maxTokens?, maxSizeBytes? }`。
    pub file_reading_limits: Option<FileReadingLimits>,

    /// 工具通知回调——工具可向前端推送非阻塞消息（进度、提示）。
    /// 对应 claude-code-best `setToolJSX`（简化版，仅文字通知）。
    pub notification_sink: Option<Arc<dyn NotificationSink>>,
}
```

**新增类型**（同在 `runtime/tools/capability.rs`）：

```rust
/// 文件状态缓存条目。
#[derive(Clone, Debug)]
pub struct FileState {
    pub content: String,
    pub mtime_secs: u64,          // 文件修改时间（秒级）
    pub offset: Option<usize>,    // 读取起始行
    pub limit: Option<usize>,     // 读取行数限制
}

/// LRU 文件状态缓存（最多 100 条，防止重读未修改文件）。
/// 对应 claude-code-best FileStateCache（LRU, max 100 entries, max 25MB）。
pub struct FileStateCache {
    inner: std::sync::Mutex<lru::LruCache<std::path::PathBuf, FileState>>,
}

impl FileStateCache {
    pub fn new() -> Self { ... }
    pub fn get(&self, path: &Path) -> Option<FileState> { ... }
    pub fn set(&self, path: PathBuf, state: FileState) { ... }
}

/// 文件读取大小上限。
#[derive(Clone, Debug)]
pub struct FileReadingLimits {
    pub max_size_bytes: usize,    // 默认 1MB（已有 read_workspace_file 默认值对齐）
}

impl Default for FileReadingLimits {
    fn default() -> Self { Self { max_size_bytes: 1_048_576 } }
}

/// 工具通知回调 trait。
pub trait NotificationSink: Send + Sync + std::fmt::Debug {
    fn notify(&self, message: &str);
}
```

**`CapabilityContext` builder 方法**（链式，保持 `with_workspace`/`with_browser` 风格）：

```rust
impl CapabilityContext {
    pub fn with_read_file_state(mut self, cache: Arc<FileStateCache>) -> Self { ... }
    pub fn with_file_reading_limits(mut self, limits: FileReadingLimits) -> Self { ... }
    pub fn with_notification_sink(mut self, sink: Arc<dyn NotificationSink>) -> Self { ... }
}
```

**构建点更新**：`plugin/registry.rs` 的 `execute()` 中构建 `CapabilityContext` 时注入 `file_reading_limits`（固定默认值 1MB）；`read_file_state` 和 `notification_sink` 在 S4 TurnDriver 路径中按 turn 创建后注入（本期 `None`，工具收到 `None` 时降级为无缓存模式）。

**`ReadWorkspaceFileRuntimeTool` 同期更新**：读文件前检查 `ctx.capability?.read_file_state`，命中且 mtime 未变则返回缓存；读后写入缓存。`max_bytes` 参数以 `file_reading_limits.max_size_bytes` 为上限。

**测试**：`tests/tool_capability_context_test.rs` 新增：
- `file_state_cache_returns_none_for_unknown_path`
- `file_state_cache_hit_when_mtime_unchanged`
- `capability_context_file_reading_limits_default_is_one_mb`
- `capability_context_notification_sink_tool_can_notify`
- `read_workspace_file_uses_file_state_cache_on_second_read`

---

## 三、Phase 2 — 并发编排 + 能力谓词（依赖 Phase 1）

### P0-B：RuntimeTool 并发编排

**问题**：所有工具调用全部串行，只读工具（list_directory、read_workspace_file、web_search 等）无法并行。

**`RuntimeTool` trait 新增方法**（`dispatcher.rs`）：

```rust
pub trait RuntimeTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError>;

    // 新增：工具声明自己是否并发安全（默认 false，保守）
    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }
}
```

**`ToolDispatcher` 新增批量调度方法**：

```rust
impl ToolDispatcher {
    /// 批量调度：将工具调用按并发安全性分区，安全的并行执行，不安全的串行执行。
    pub async fn dispatch_batch(
        &self,
        calls: Vec<(String, Value, ToolExecutionContext)>,
    ) -> Vec<Result<ToolDispatchOutcome, ToolError>>;
}
```

**分区算法**（同 claude-code-best `partitionToolCalls`）：
- 遍历 calls，若当前 call `is_concurrency_safe` 且上一批也是，则合并
- 否则新建批次
- 并发批：`tokio::join_all` 执行，最多 10 个并行（`TOOL_MAX_CONCURRENCY = 10`）
- 串行批：顺序 await

**各内置工具 `is_concurrency_safe` 声明**：

| 工具 | 安全 | 理由 |
|------|------|------|
| list_directory | ✅ | 只读 |
| read_workspace_file | ✅ | 只读 |
| search_files | ✅ | 只读 |
| get_file_info | ✅ | 只读 |
| web_search | ✅ | 只读 |
| browse_navigate | ❌ | 修改 browser state |
| read_page_content | ✅ | 只读 |
| page_execute_js | ❌ | 有副作用 |
| extract_table_data | ✅ | 只读 |
| load_file | ❌ | 写入 session cache |

**测试**：`tests/tool_dispatcher_test.rs` 新增：
- `dispatch_batch_concurrent_safe_tools_run_in_parallel`
- `dispatch_batch_non_concurrent_tools_run_serially`
- `dispatch_batch_mixed_partition_correct_order`

---

### P1-B：ToolDefinition 运行时谓词

**问题**：`ToolKind` 是静态标签，无法基于具体输入判断工具行为（如同一 `bash` 工具执行 `ls` vs `rm -rf` 的破坏性不同）。

**`ToolDefinition` 新增字段**：

```rust
pub struct ToolDefinition {
    // 现有字段不变
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub capability_scope: Vec<String>,
    pub kind: ToolKind,

    // 新增：静态默认值（运行时谓词由 RuntimeTool 方法覆盖）
    pub default_read_only: bool,       // 默认 false
    pub default_destructive: bool,     // 默认 false
}
```

**`RuntimeTool` trait 新增谓词方法**（与 `is_concurrency_safe` 同期加入）：

```rust
fn is_read_only(&self, _input: &Value) -> bool {
    self.definition().default_read_only
}
fn is_destructive(&self, _input: &Value) -> bool {
    self.definition().default_destructive
}
```

**用途**：权限管线可用 `is_destructive` 决定是否升级为 Ask；并发编排可用 `is_read_only` 辅助判断并发安全性。

**测试**：`tests/tool_catalog_contract_test.rs` 新增：
- `read_workspace_file_is_read_only`
- `execute_python_is_not_read_only`
- `tool_definition_default_destructive_is_false`

---

## 四、Phase 3 — 工具迁移 + 动态权限（依赖 Phase 1）

### P0-C：核心工具迁离 LegacyToolAdapter

**迁移目标（3 个）**：

| 工具 | 当前 | 目标 | 关键依赖 |
|------|------|------|---------|
| `execute_python` | LegacyToolAdapter | RuntimeTool | `2026-04-16-execute-python-migration-boundary.md` 已分析 |
| `generate_report` | LegacyToolAdapter | RuntimeTool | `report_gen.rs` → `ReportCapability` trait |
| `generate_chart` | LegacyToolAdapter | RuntimeTool | `chart_gen.rs` → `ChartCapability` trait |

**迁移模式**（与 workspace tools 一致）：

1. 在 `runtime/tools/builtin/` 下新建实现文件（`python.rs`、`report.rs`、`chart.rs`）
2. 将原 `PluginContext` 依赖提取为 capability trait（`PythonExecution`、`ReportCapability`、`ChartCapability`）
3. `plugin/registry.rs` 的 `try_build_request_scoped_tool` 中构建这些工具并注入 capability
4. 原 `plugin/builtin/tools/python_exec.rs` 等保留为 dead code（加 `#[allow(dead_code)]`），待所有调用点确认迁移完成后删除

**`execute_python` 迁移**（最优先，参照已有 boundary 分析）：
- 纯值字段（`workspace_path`、`conversation_id`、`run_id`、`model`）迁入 `CapabilityContext`（本 Phase 补充字段）或 `ToolExecutionContext`
- `session_manager` 通过 `PythonExecution` trait 注入（不进 context）
- `app_handle` 在启动期解析为 `python_binary: PathBuf`

**测试**：`tests/primitive_tools_migration_test.rs` 新增：
- `execute_python_tool_is_runtime_tool_not_legacy`
- `generate_report_tool_is_runtime_tool_not_legacy`
- `generate_chart_tool_is_runtime_tool_not_legacy`
- `execute_python_does_not_accept_plugin_context_directly`

---

### P1-A：RuntimeTool 动态权限参与

**问题**：权限检查在 `CapabilityPermissionPipeline` 层做静态能力判断，工具本身无法基于具体输入内容动态决策。

**`RuntimeTool` trait 新增方法**：

```rust
pub trait RuntimeTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError>;
    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }
    fn is_read_only(&self, _input: &Value) -> bool { false }
    fn is_destructive(&self, _input: &Value) -> bool { false }

    // 新增：工具自定义权限检查（默认返回 None = 交给 pipeline 处理）
    async fn check_permissions(
        &self,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        None
    }
}
```

**`ToolDispatcher::dispatch()` 调用顺序**：
1. 先调用 `tool.check_permissions(input, ctx)` — 若返回 `Some(decision)` 则直接使用
2. 若返回 `None`，则走现有 `permission_pipeline.authorize()`

**优先级**：工具自定义检查 > pipeline 静态检查

**适用场景**：
- `execute_python`：检查代码中是否有危险 pattern（已有 `validate_code()`，迁入此方法）
- `browse_navigate`：检查 URL 是否在允许域名列表内
- 未来 `bash` 工具：tree-sitter 解析命令语义

**测试**：`tests/tool_permission_pipeline_test.rs` 新增：
- `tool_check_permissions_overrides_pipeline_when_some`
- `tool_check_permissions_falls_through_to_pipeline_when_none`
- `execute_python_check_permissions_denies_dangerous_code`

---

## 五、关键约束

1. **`runtime/` 禁止 `use tauri::*`**：所有新字段通过 trait 注入，不直接依赖 `AppHandle`
2. **新字段全部 `Option`**：Phase 1 上线后已有测试无需修改
3. **`CapabilityContext` 不进编排层对象**：`LlmGateway`、`AuthManager`、`AgentRuntime` 永远不进 `CapabilityContext`
4. **迁移工具保留旧实现**：迁移期间旧 `ToolPlugin` 实现保留，确认 zero regression 后再删除
5. **排序不影响过滤语义**：`get_schemas_filtered` 的 `Only`/`Exclude` 逻辑不变，排序在过滤后应用

---

## 六、文件改动清单

### Phase 1
| 文件 | 改动 |
|------|------|
| `src-tauri/src/runtime/tools/capability.rs` | 新增 `FileState`、`FileStateCache`、`FileReadingLimits`、`NotificationSink` 类型；`CapabilityContext` 新增 3 个字段 + 3 个 builder 方法 |
| `src-tauri/src/plugin/registry.rs` | `get_all_schemas`/`get_schemas_filtered` 末尾加排序；`execute()` 构建 capability 时注入 `file_reading_limits` |
| `src-tauri/src/runtime/tools/builtin/workspace.rs` | `ReadWorkspaceFileRuntimeTool` 使用 `read_file_state` 缓存 + `file_reading_limits` 上限 |
| `src-tauri/tests/tool_catalog_contract_test.rs` | 新增排序测试 |
| `src-tauri/tests/tool_capability_context_test.rs` | 新增 5 个缓存/限制/通知测试 |

### Phase 2
| 文件 | 改动 |
|------|------|
| `src-tauri/src/runtime/tools/dispatcher.rs` | `RuntimeTool` 新增 3 个默认谓词方法；`ToolDispatcher` 新增 `dispatch_batch` |
| `src-tauri/src/runtime/tools/definition.rs` | `ToolDefinition` 新增 2 个谓词字段 |
| `src-tauri/src/runtime/tools/builtin/workspace.rs` | 各工具实现 `is_concurrency_safe`/`is_read_only` |
| `src-tauri/src/runtime/tools/builtin/browser.rs` | 各工具实现谓词 |
| `src-tauri/tests/tool_dispatcher_test.rs` | 新增并发分区测试 |
| `src-tauri/tests/tool_catalog_contract_test.rs` | 新增谓词测试 |

### Phase 3
| 文件 | 改动 |
|------|------|
| `src-tauri/src/runtime/tools/builtin/python.rs` | 新建，`ExecutePythonRuntimeTool` 实现 |
| `src-tauri/src/runtime/tools/builtin/report.rs` | 新建，`GenerateReportRuntimeTool` 实现 |
| `src-tauri/src/runtime/tools/builtin/chart.rs` | 新建，`GenerateChartRuntimeTool` 实现 |
| `src-tauri/src/runtime/tools/dispatcher.rs` | `RuntimeTool` 新增 `check_permissions` 默认方法；`dispatch` 调用顺序更新 |
| `src-tauri/src/plugin/registry.rs` | `try_build_request_scoped_tool` 新增 3 个工具构建 |
| `src-tauri/tests/primitive_tools_migration_test.rs` | 新增迁移验证测试 |
| `src-tauri/tests/tool_permission_pipeline_test.rs` | 新增动态权限测试 |
