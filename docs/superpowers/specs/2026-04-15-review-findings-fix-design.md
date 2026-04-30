# 2026-04-15 复审 Findings 修复设计

来源：`docs/reviews/2026-04-15-plan-implementation-review.md` Findings 3/4/5

## C: P1-A file_meta 透传（Finding 5）

### 问题

`RuntimeToolCallOutcome` 只有 4 字段（tool_call_id/tool_name/content/is_error），不携带 file_meta。
`chat_runtime_impl.rs` 里 `all_file_metas` 初始化为空 Vec 后从未被 runtime tool round 路径填充。
导致 `verify_file_claims()`、文件格式纠错、降级提示静默退化。

### 设计

1. **扩展 `RuntimeToolCallOutcome`**（`runtime/chat/tool_round_types.rs`）：

   ```rust
   pub struct RuntimeToolCallOutcome {
       pub tool_call_id: String,
       pub tool_name: String,
       pub content: String,
       pub is_error: bool,
       // --- 新增 ---
       pub file_meta: Option<FileMeta>,
       pub is_degraded: bool,
       pub degradation_notice: Option<String>,
   }
   ```

2. **`ToolResult` → `RuntimeToolCallOutcome` 的映射**：
   - `ToolResult` 已有 `data: Option<Value>` 字段
   - `ToolOutput`（legacy）有 `file_meta`、`is_degraded`、`degradation_notice`
   - 在 `QueryEngine::run_tool_call_with_bus()` 构建 outcome 时，从 `ToolResult.data` 中提取这三个字段
   - `ToolResult` 新增 `file_meta`、`is_degraded`、`degradation_notice` 字段（与 `ToolOutput` 对齐）

3. **`chat_runtime_impl.rs` 收集 file_metas**：
   - tool round 收尾处遍历 `round_results`，从 `Ok(outcome)` 里提取 `file_meta` 填入 `all_file_metas`

4. **`LegacyToolAdapter` 桥接**：
   - `LegacyToolAdapter::execute()` 返回的 `ToolResult` 需要携带 `file_meta`/`is_degraded`/`degradation_notice`
   - 从 `ToolOutput` 的对应字段映射过来

### 不改

- `verify_file_claims()` 和 `tag_content_with_file_meta()` 逻辑本身不需要修改
- `FileMeta` struct 定义不变

---

## A: P4-C StorePolicyPipeline 接入生产（Finding 3）

### 问题

`StorePolicyPipeline` 已实现但只在测试中使用。生产 dispatcher 硬编码 `CapabilityPermissionPipeline`。
unknown scope fail-open。

### 设计

1. **`lib.rs` 初始化**：创建 `Arc<PermissionStore>` 存入 Tauri managed state
   - 路径：`{workspace_base}/shared/permissions.json`
   - 若 workspace 不存在则延迟到首次使用时创建

2. **`ToolRegistry` 注入**：新增 `permission_store: Option<Arc<PermissionStore>>` 字段
   - 构造时从 Tauri managed state 注入
   - 新增 `with_permission_store()` builder 方法

3. **`to_runtime_dispatcher()`**：
   ```rust
   let pipeline: Arc<dyn PermissionPipeline> = match &self.permission_store {
       Some(store) => Arc::new(StorePolicyPipeline::new(store.clone())),
       None => Arc::new(CapabilityPermissionPipeline),
   };
   let dispatcher = Arc::new(ToolDispatcher::new(pipeline));
   ```

4. **`execute()` 内部**：同理替换直接构造的 `CapabilityPermissionPipeline`

5. **更新注释**：移除 `permission.rs` 中"尚未接入生产 dispatcher"的注释

### 行为变化

- unknown scope 从 `debug log + allow` 变为 `bail + deny`
- 需要确认现有工具没有使用自定义 scope（否则会被意外 deny）

---

## B: P4-A CancellationToken 收口（Finding 4）

### 问题

两个 `tokio::spawn` 无 CancellationToken：
- `python/session.rs:640-648` LRU eviction
- `chat_runtime_impl.rs:1127-1132` agent loop

### 设计

1. **`PythonSessionManager::get_or_create()`**：接受 `CancellationToken` 参数
   - eviction spawn 内部：`write_checkpoint()` 前检查 `token.is_cancelled()`，已取消则跳过 checkpoint 直接 kill

2. **`chat_runtime_impl.rs` agent loop spawn**：
   - 从 `run_id` 层获取或创建 `CancellationToken`
   - spawn 内部在每次 iteration 开头检查 `token.is_cancelled()`

3. **调用链 threading**：
   - `get_or_create()` 的所有调用者传入 token（或 `CancellationToken::new()` 做 default）
   - agent loop 的 token 来源：从 `RuntimeRunRegistry` 或上层传入

### 不改

- 热路径上的 PluginContext 构建点不动
- `precompute_auto_load` 的 PluginContext 构建不动

---

## 实施顺序

C → A → B（file_meta 最独立，P4-C 影响权限可测试，P4-A 涉及并发最敏感）
