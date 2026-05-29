# 2026-04-16 execute_python 迁移边界分析

## 结论

`execute_python` 在 S3 **不实施 runtime-native 迁移**，只做 dependency inventory + boundary definition。S4 的迁移目标应是：

- `handle_execute_python` 不再接收 `&PluginContext`
- 纯值型字段迁入 `ExecutionContext`
- 无状态服务迁入 `CapabilityContext`
- 有状态 `PythonSessionManager` 通过独立 `PythonExecution` trait 注入
- `AppHandle` 依赖在启动期提升为 `python_binary` 配置，消除运行时平台耦合

---

## 当前依赖的 PluginContext 字段

| 字段 | 用途 | 可迁到 CapabilityContext？ | 迁移方案 | 特殊处理 |
|------|------|--------------------------|---------|---------|
| `workspace_path` | SandboxConfig 构造、analysis preamble 路径、文件遍历、telemetry | 是 | `ctx.capability.workspace_path` | 无 |
| `conversation_id` | 上传文件查询、loaded_key 生成、analysis snap_dir、telemetry | 否（更适合 ExecutionContext） | `ctx.execution.conversation_id` | 无 |
| `run_id` | `loaded_scope_id()` 分支 + `execute_for_run` 路由 | 否（更适合 ExecutionContext） | `ctx.execution.run_id` | `loaded_scope_id()` 逻辑须随之迁移 |
| `storage` | 上传文件列表、loaded/failed key、insert_generated_file、get_step_state | 是 | `ctx.capability.storage` | `get_step_state` 依赖 storage，一并迁 |
| `authorized_workspace` | SandboxConfig::for_workspace_with_authorized + workspace preamble | 是 | `ctx.capability.authorized_workspace` | 需保持 AuthorizedWorkspaceRef 可访问 |
| `session_manager` | analysis mode 下 `execute_for_run` / `execute` 持久 REPL | 否 | 通过 `dyn PythonExecution` 注入 | 持有进程和 session map，不应塞进 context |
| `app_handle` | `PythonRunner::with_config` (daily mode) | 否 | 启动期解析 `python_binary` | Tauri 平台类型，应从运行时剥离 |
| `model` | telemetry 打点 | 否（更适合 ExecutionContext） | `ctx.execution.model` | 仅字符串标签 |
| `file_manager` | auto-load 中传递给 `handle_load_file_core` | 是 | `ctx.capability.file_manager` | 无 |

---

## S4 迁移目标拆分

### 可直接迁移的字段

迁入 `ExecutionContext`：
- `conversation_id`
- `run_id`
- `model`

迁入 `CapabilityContext`：
- `workspace_path`
- `storage`
- `authorized_workspace`
- `file_manager`

### 需要特殊处理的字段

#### `session_manager`

`PythonSessionManager` 持有：
- `HashMap<session_key, Arc<PythonSession>>`
- 每个 session 关联持久 Python 子进程
- LRU eviction / checkpoint / kill 逻辑

它是**有状态 runtime service**，不能简单塞进 `CapabilityContext`。S4 应通过 `PythonExecution` trait 注入。

#### `app_handle`

当前仅用于解析 Python 可执行路径。S4 应在启动期完成解析，持久化为 `python_binary: PathBuf`，避免 `handle_execute_python` 运行时依赖 Tauri `AppHandle`。

---

## S4 建议的 PythonExecution trait

```rust
#[async_trait]
pub trait PythonExecution: Send + Sync {
    /// analysis mode: 持久 session，按 run_id 或 conversation_id 路由
    async fn execute_in_session(
        &self,
        scope_key: &str,
        code: &str,
        timeout: Duration,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult>;

    /// daily mode: one-shot 执行，无持久状态
    async fn execute_oneshot(
        &self,
        workspace: &Path,
        code: &str,
        sandbox: &SandboxConfig,
    ) -> Result<ExecutionResult>;
}
```

### 调用侧目标形态

```rust
async fn handle_execute_python(
    ctx: &ExecutionContext,
    python: &dyn PythonExecution,
    args: &Value,
) -> Result<String>
```

这样 `handle_execute_python` 不再依赖 `PluginContext` 全量字段，只消费：
- `ExecutionContext`（identity + cancel + model）
- `CapabilityContext`（storage/file_manager/workspace）
- `PythonExecution` trait（analysis / daily 两条执行路径）

---

## S3 结论

S3 只做两件事：
1. 明确 `execute_python` 的依赖边界
2. 为 S4 留出清晰的迁移接口

**S3 不把 execute_python 记为 runtime-native migration 完成。**
