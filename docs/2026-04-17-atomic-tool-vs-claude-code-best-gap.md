# 原子工具体系 vs claude-code-best 工具架构差距分析

**日期**：2026-04-17  
**调查方式**：两路并行 agent 深度调研（各约 300 个 tool use）  
**对标基准**：`/Users/a20250311/github/claude-code-best` 源码  
**关联文档**：
- `docs/2026-04-13-atomic-tool-problem-statement.md` — 原子工具专项问题定义
- `docs/2026-04-14-backend-architecture-gap-assessment.md` — 后端架构差距全景报告

---

## 一、差距总览

| 维度 | lotus-app 当前 | claude-code-best | 差距等级 |
|------|---------------|------------------|---------|
| 工具执行上下文 | `CapabilityContext` 4 字段 | `ToolUseContext` 40+ 字段 | **P0** |
| 工具并发编排 | 全串行 | 智能分区并行（读并发/写串行） | **P0** |
| 工具迁移进度 | 11/36 已迁移（~30%） | 全部统一接口 | **P0** |
| 工具权限参与 | 静态能力检查（不基于输入） | 每工具 `checkPermissions(input, ctx)` | **P1** |
| 工具能力声明 | `ToolKind` 静态枚举 | 运行时谓词（isConcurrencySafe/isReadOnly/isDestructive） | **P1** |
| 工具结果预算 | 无限制 | `maxResultSizeChars` + 全局预算追踪 | **P1** |
| Tool Pool 组装 | 全局 RwLock HashMap（无序） | per-call 动态 filter+sort（prompt cache 稳定） | **P2** |
| 流式工具进度 | EventSink emit 事件名 | `onProgress` 回调 + `renderToolUseProgressMessage` | **P2** |

---

## 二、P0 — 架构级缺陷

### 2.1 工具执行上下文贫瘠

**当前：`CapabilityContext`（4 字段）**

```rust
// src-tauri/src/runtime/tools/capability.rs
pub struct CapabilityContext {
    pub storage: Option<StorageCapability>,   // 工作目录
    pub workspace_id: Option<String>,
    pub browser_available: bool,
    pub file_ops: Option<Arc<dyn FileOperations>>,  // 仅 load_file 使用
}
```

**对标：`ToolUseContext`（40+ 字段，精选关键字段）**

```typescript
// claude-code-best/src/Tool.ts:158-300
export type ToolUseContext = {
  abortController: AbortController      // 取消当前操作
  getAppState(): AppState               // 读会话状态
  setAppState(f): void                  // 写会话状态
  messages: Message[]                   // 完整对话历史
  readFileState: FileStateCache         // 避免重复读文件
  options: { tools, mcpClients, ... }   // 当前可用工具列表、MCP 连接
  setToolJSX / addNotification          // 发 UI 通知
  toolDecisions: Map<string, Decision>  // 权限决策历史
  contentReplacementState               // 工具结果预算追踪
  updateFileHistoryState / Attribution  // 文件归因追踪
  // ... 还有约 30 个字段
}
```

**影响**：lotus-app 工具是"无状态孤岛"——无法感知对话历史、无法发 UI 通知、无法访问其他工具的状态结果。复合工具（`browse_and_extract`、`generate_report`）只能靠 prompt 里写逻辑来弥补状态隔离问题，而不是靠结构化接口。

---

### 2.2 无并发工具编排

**当前**：所有工具调用全部串行，`ToolDispatcher` 没有分区逻辑，`RuntimeTool` trait 没有 `isConcurrencySafe` 方法。

**对标**：

```typescript
// claude-code-best/src/services/tools/toolOrchestration.ts
function partitionToolCalls(toolUses, context): Batch[] {
  // 连续并发安全工具合并为一批 → Promise.all 并行执行（最多 10 个）
  // 非并发安全工具各自一批 → 串行执行
}

// 典型并发安全工具（只读）：GlobTool, GrepTool, FileReadTool, WebFetchTool
// 典型非并发安全工具（写操作）：BashTool, FileEditTool, FileWriteTool
```

**影响**：多工具调用时延叠加。`web_search` + `list_directory` + `get_file_info` 本可并行，现在必须排队。

---

### 2.3 工具迁移进度低（约 70% 仍在 LegacyToolAdapter）

**已迁移为 RuntimeTool（11 个）**：

| 工具 | 文件 |
|------|------|
| list_directory, read_workspace_file, search_files, get_file_info | `runtime/tools/builtin/workspace.rs` |
| web_search | `runtime/tools/builtin/network.rs` |
| browse_navigate, read_page_content, page_execute_js, extract_table_data, extract_with_pagination | `runtime/tools/builtin/browser.rs` |
| load_file | `runtime/tools/builtin/file.rs` |

**仍走 LegacyToolAdapter（~25 个，包含全部核心业务工具）**：

| 工具类别 | 工具名 |
|---------|--------|
| Power（最核心） | `execute_python`, `hypothesis_test`, `detect_anomalies` |
| Composite（生成类） | `generate_report`, `generate_chart`, `export_data`, `generate_slides` |
| Composite（导航类） | `browse_data`, `browse_and_extract` |
| Support（记忆） | `save_memory`, `search_memory`, `core_memory`, `distill_memory` |
| Support（规划） | `plan_update`, `progress_update`, `save_analysis_note` |

**影响**：最核心的 Power/Composite 工具全部还走 `PluginContext` 全局 service locator 路径：
- 不受 `CapabilityContext` 隔离约束，仍可访问完整 `PluginContext`（206 引用点）
- 无法利用 per-call `ToolExecutionContext` 的级联取消特性
- `LegacyToolAdapter` 适配层有额外开销，且掩盖了 context 贫瘠问题

---

## 三、P1 — 功能差距

### 3.1 工具不能参与权限决策

**当前**：权限检查在 `CapabilityContext` 层做静态能力判断，基于 `capability_scope`（字符串列表），与具体的工具调用输入无关。

```rust
// src-tauri/src/runtime/tools/permission.rs
// CapabilityPermissionPipeline：只检查 scope 是否有对应 capability
// 不感知：命令内容、文件路径、破坏性等具体输入
```

**对标**：每个工具实现 `checkPermissions(input, context)` 基于**具体输入**动态决策：

```typescript
// claude-code-best/src/tools/BashTool/BashTool.tsx
async checkPermissions(input, context): Promise<PermissionResult> {
  return bashToolHasPermission(
    input.command,  // ← 感知具体命令内容
    context.getAppState().toolPermissionContext,
    context.abortController.signal,
    context.options.isNonInteractiveSession,
  )
  // 内部：tree-sitter 解析命令语义、检查危险模式、路径约束等
}
```

**影响**：`execute_python` 无论执行什么代码都走同一权限路径；`browse_navigate` 无论访问什么 URL 都一样处理。无法基于内容做细粒度权限判断。

---

### 3.2 工具能力声明不足

**当前**：

```rust
pub struct ToolDefinition {
    pub id: String,
    pub capability_scope: Vec<String>,  // ["workspace:read"]
    pub kind: ToolKind,                 // Primitive/Power/Composite/Support（静态标签）
}
```

`ToolKind` 是静态标签，不接受 input 参数，无法反映"同一工具在不同参数下的行为差异"。

**对标**：工具声明运行时可查询的布尔谓词：

```typescript
isConcurrencySafe(input): boolean   // 影响并发调度
isReadOnly(input): boolean          // 影响权限评估
isDestructive(input): boolean       // 影响确认提示（rm -rf vs ls 不同）
isOpenWorld(input): boolean         // 影响安全边界判断
interruptBehavior(): 'cancel'|'block' // ESC 时的行为
```

所有谓词接受 `input` 参数，同一工具在不同输入下可以返回不同值。例如 `BashTool.isDestructive('rm -rf')` 返回 `true`，`BashTool.isDestructive('ls')` 返回 `false`。

---

### 3.3 无工具结果预算控制

**当前**：工具结果无大小限制，单个工具（如 `execute_python` 返回大量数据）可能一次性占满 LLM 上下文窗口，没有截断机制。

**对标**：
- 每个工具声明 `maxResultSizeChars`
- `contentReplacementState` 在 `ToolUseContext` 中全局追踪预算
- 超限工具结果自动截断并附提示���模型知道结果被截断，可以请求分页）

---

## 四、P2 — 优化项

### 4.1 Tool Pool 组装影响 Prompt Cache 命中率

**当前**：全局 `RwLock<HashMap<String, Arc<dyn RuntimeTool>>>`，HashMap 无序，`get_schemas_filtered()` 每次返回的工具顺序不确定。

**对标**：每次 API call 重新组装 tool pool，**按名称排序保证顺序稳定**：

```typescript
// claude-code-best/src/tools.ts
assembleToolPool(permissionContext, mcpTools): Tools
// 内置工具 + MCP 工具 → 按 name 排序 → dedup
// 目的：工具列表是 Anthropic API prompt cache 的 key，顺序变化 = cache miss
```

**影响**：每次 `get_schemas_filtered` 返回顺序不稳定时，Anthropic API 侧的 prompt cache 无法命中工具列表部分，token 消耗增加。

---

### 4.2 旧工具取消绑定未验证

lotus-app 已有 `CancellationToken::child_token()` 级联机制，但以下问题尚未验证：

- `LegacyToolAdapter` 里的旧工具（~25 个）是否真正接入了 cancellation token？
- `PluginContext` 传入旧工具时是否携带了 cancel signal？
- 旧工具内部的 `tokio::spawn` 是否在 spawn 时绑定了 `CancellationToken`？

若旧工具里有 `tokio::spawn(async { ... })` 而未绑定 token，ESC 后这些 spawn 任务仍在后台运行（fire-and-forget），与 claude-code-best 的 AbortController 级联广播相比存在静默后台任务泄漏风险。

---

### 4.3 工具流式进度粒度不足

**当前**：`EventCollectingSink` 只 emit 字符串事件名（`"tool:executing"` / `"tool:completed"`），工具内部进度无法流式推给前端。

**对标**：工具 `call()` 方法有 `onProgress` 回调，执行中可实时流式推进度消息，`renderToolUseProgressMessage` 在 UI 渲染实时进度。

---

## 五、核心结论

差距的根本不是工具**数量**，而是工具与系统的**集成深度**：

> claude-code-best 的工具是"系统的一等公民"——通过 `ToolUseContext` 可以感知和影响整个 agent runtime 状态。
>
> lotus-app 的工具是"系统的执行器"——通过 `CapabilityContext` 只能做有限的文件/浏览器操作，核心 agent 状态对工具不可见。

### 建议优先级

> ⚠️ 以下优先级在对标 claude-code-best 实际工具源码调研后已修正（见 `docs/superpowers/specs/2026-04-17-atomic-tool-capability-upgrade-design.md`）。

1. **P0-A**：`CapabilityContext` 扩展——补充 `FileStateCache`（防重读）、`FileReadingLimits`（防超大文件）、`NotificationSink`（UI 通知）。`abortController` 已有（`ToolExecutionContext.cancellation`），`messages` 在实际工具中未直接使用，暂不加入。
2. **P0-B**：`RuntimeTool` trait 增加 `isConcurrencySafe` + `ToolDispatcher` 支持并发分区编排
3. **P0-C**：`execute_python` / `generate_report` / `generate_chart` 尽快迁离 `LegacyToolAdapter`（核心 Power/Composite 工具优先）
4. **P1-A**：`RuntimeTool` trait 增加 `check_permissions(input, ctx)` 方法，允许工具基于具体输入动态决策
5. **P1-B**：`ToolDefinition` 增加运行时谓词字段（`default_read_only`、`default_destructive`）+ `RuntimeTool` 对应默认方法
6. **P2-A**：`get_schemas_filtered()` 返回结果按 tool id 排序，稳定 prompt cache key

**实施计划**：`docs/superpowers/plans/2026-04-17-atomic-tool-capability-upgrade-plan.md`
