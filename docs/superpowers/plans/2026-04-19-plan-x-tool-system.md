# 工具系统改进（Plan-X）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:executing-plans`
> 每个 Task 完成后必须运行对应 cargo test 命令并确认通过后再 commit，不允许跳过测试。

**Goal:** Tool pool 分区排序保证 prompt cache stability；同时为 lotus 补上 compact 保留位与默认超时声明，但明确只有排序项是直接对标 `claude-code-best`，其余为 lotus 扩展。

**Architecture:**
- X1 在 `get_schemas_filtered`（`plugin/registry.rs`）层按 built-in/MCP 分区各自字母序排序再合并，built-in 在前、MCP 在后，不改变 `get_all_schemas`；
- X2 在 `runtime/tools/definition.rs` 的 `ToolDefinition` 新增 `preserve_tool_use_results: bool`（default false），`runtime/chat/compaction.rs` 的 `microcompact` 函数读取该标志跳过对应工具结果；
- X3 在 `ToolDefinition` 新增 `default_timeout_secs: Option<u64>`，`builtin/bash.rs` 和 `builtin/python.rs` 的执行层在没有 input 级 timeout 时回落到 definition 级默认值。

**Tech Stack:** Rust

**Worktree branch:** pzc

---

## 对标修订（2026-04-19）

- X1（built-in/MCP 分区排序）是本计划中唯一直接 1:1 对标 `claude-code-best` 的部分。
- X2 若继续沿用 `preserve_tool_use_results` 字段名，文档必须明确它在 lotus 中表示“compact 时保护工具结果”，不是 `claude-code-best` 的 subagent transcript 可见性语义。
- X3 的 catalog 级默认超时属于 lotus 统一化增强，不应在文档中表述为对标仓库已有同构机制。
- `get_schemas_filtered` 与 UI/Schema 列表若都要排序，需要分别说明；不要在计划中同时写“只改 query-visible tool pool”和“顺带改全部 schema”两种口径。

---

## 背景与问题定位

### X1：Tool Pool 无 built-in/MCP 分区排序

**现状（`plugin/registry.rs` L282-L352）：**

`get_schemas_filtered` 当前把 runtime tools（来自 `runtime_tools` map）、request-scoped tools（`REQUEST_SCOPED_RUNTIME_TOOL_NAMES`）、legacy tools 三段拼接后做一次全局字母序排序（`schemas.sort_by(|a, b| a.name.cmp(&b.name))`）。当有 MCP 工具时（由 `register_mcp_server` 注入 `runtime_tools`），MCP 工具名称（格式 `mcp__<server>__<tool>`）会被穿插进 built-in 工具中间，导致每次 MCP server 增减都让 prompt cache key 全部失效。

**参考（`claude-code-best/src/tools.ts` L343-L364，`assembleToolPool`）：**
```
// Sort each partition for prompt-cache stability, keeping built-ins as a
// contiguous prefix. The server's claude_code_system_cache_policy places a
// global cache breakpoint after the last prefix-matched built-in tool;
// a flat sort would interleave MCP tools into built-ins and invalidate
// all downstream cache keys whenever an MCP tool sorts between existing
// built-ins.
const byName = (a: Tool, b: Tool) => a.name.localeCompare(b.name)
return uniqBy(
  [...builtInTools].sort(byName).concat(allowedMcpTools.sort(byName)),
  'name',
)
```

**识别 MCP 工具的方式：** 工具名以 `mcp__` 前缀开头（见 `runtime/mcp/types.rs` fully-qualified 命名规则）。

### X2：`ToolDefinition` 无 `preserve_tool_use_results` 字段

**现状：** `runtime/tools/definition.rs` 的 `ToolDefinition` struct 共 8 个字段，无 preserve 标志。`runtime/chat/compaction.rs` 的 `microcompact` 函数（L96-L164）仅按 `role == "tool"` 筛选，无差别清空超出 `keep_recent_tool_results` 数量的旧工具结果。

`execute_python` 的工具结果包含 DataFrame 统计摘要，`generate_report` 的结果包含报告文件路径，两者若被 microcompact 清空，后续对话上下文会丢失关键分析结论。

**参考（`claude-code-best/src/Tool.ts` L278）：**
```typescript
/** When true, preserve toolUseResult on messages even for subagents.
 * Used by in-process teammates whose transcripts are viewable by the user. */
preserveToolUseResults?: boolean
```

这里语义略有差异：claude-code-best 的字段控制跨代理结果可见性，本项目需要的是 compact 时不截断该工具的结果——目的不同但字段名和思路一致。

**目前 microcompact 消费路径：**
`chat_turn_driver.rs` 调用 `compaction::microcompact(messages, &config)`，参数仅有 messages 和 config（含 `keep_recent_tool_results: usize`），函数无法感知工具身份。

### X3：工具执行超时只靠 input 参数传入

**现状：**
- `bash.rs` L311-315：从 input `timeout_secs` 字段取值，缺省 `DEFAULT_TIMEOUT_SECS=120`，上限 `MAX_TIMEOUT_SECS=600`。
- `python.rs` execute 方法（L168 调用 `handle_execute_python_core`）→ `llm/tool_executor/python.rs` L217：硬编码 `let timeout = Duration::from_secs(600)`，完全忽略 input。

两者的默认值都以魔法数字散落在实现文件中，没有在 `ToolDefinition` 层统一声明，导致：
1. 调用方（未来的 dispatcher）无法在路由层为工具应用一致的超时策略；
2. catalog 不是工具行为约束的单一真相源。

---

## Task X1：get_schemas_filtered 分区排序

**文件：** `src-tauri/src/plugin/registry.rs`

**变更描述：**

将 `get_schemas_filtered`（和 `get_all_schemas`）中的最终排序从单次全局字母序改为分区排序：
1. 先收集所有条目，为每条打 partition 标记：若工具名以 `mcp__` 开头则为 MCP 分区，否则为 built-in 分区；
2. built-in 分区内字母序排序；
3. MCP 分区内字母序排序；
4. 合并为 `[built-ins sorted] ++ [mcp sorted]`。

`get_all_schemas` 也采用相同逻辑保持一致性（虽然该函数目前仅用于管理 UI，不影响 LLM 请求）。

**实现草稿（`get_schemas_filtered` 结尾处，替换原 sort 调用）：**

```rust
// Partition-stable sort: built-ins first (sorted), MCP last (sorted).
// This matches claude-code-best assembleToolPool's cache-stability guarantee:
// built-in tools form a contiguous prefix so a prompt cache breakpoint
// after the last built-in remains valid when MCP servers are added/removed.
let (mut builtin_schemas, mut mcp_schemas): (Vec<_>, Vec<_>) =
    schemas.into_iter().partition(|td| !td.name.starts_with("mcp__"));
builtin_schemas.sort_by(|a, b| a.name.cmp(&b.name));
mcp_schemas.sort_by(|a, b| a.name.cmp(&b.name));
builtin_schemas.extend(mcp_schemas);
let schemas = builtin_schemas;
```

**新增 review 测试：**

文件：`src-tauri/tests/review_tool_pool_ordering.rs`

测试要点：
1. `review_builtin_tools_precede_mcp_tools_in_filtered_schema`：构造 mock registry，注册若干 built-in 工具（含字母序靠后的名称，如 `web_search`、`write_file`）和 MCP 工具（`mcp__a_server__alpha`、`mcp__b_server__zz`），调用 `get_schemas_filtered(All)` 后断言：所有 non-mcp 工具的 schema 下标均小于所有 mcp 工具的 schema 下标。
2. `review_builtin_partition_is_internally_sorted`：断言返回结果中 built-in 子序列为严格字母序。
3. `review_mcp_partition_is_internally_sorted`：断言 MCP 子序列为严格字母序。
4. `review_filtered_schema_order_is_stable_when_mcp_server_added`：先获取无 MCP 的 schema 列表，再注册一个 MCP server，再次获取，断言 built-in 部分顺序不变。

**cargo test 命令：**
```bash
cd src-tauri && cargo test review_tool_pool_ordering --tests --no-fail-fast -- --nocapture
```

**commit message:** `feat(tool-pool): partition-sort built-ins before MCP tools for prompt cache stability - X1`

---

## Task X2：ToolDefinition 新增 preserve_tool_use_results + microcompact 消费

### X2-a：ToolDefinition 字段扩展

**文件：** `src-tauri/src/runtime/tools/definition.rs`

新增字段与 builder：

```rust
pub struct ToolDefinition {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub capability_scope: Vec<String>,
    pub kind: ToolKind,
    pub default_read_only: bool,
    pub default_destructive: bool,
    pub default_max_result_size_chars: usize,
    /// 工具结果在 microcompact 时必须保留（不得被截断替换为 [microcompacted]）。
    /// 适用于包含后续分析必要数据的工具，如 execute_python（统计摘要）、
    /// generate_report（输出文件路径）。
    pub preserve_tool_use_results: bool,
}
```

`ToolDefinition::new` 中将 `preserve_tool_use_results` 初始化为 `false`。

新增 builder 方法：
```rust
pub fn with_preserve_tool_use_results(mut self, preserve: bool) -> Self {
    self.preserve_tool_use_results = preserve;
    self
}
```

### X2-b：catalog 标注关键工具

**文件：** `src-tauri/src/runtime/tools/catalog.rs`

在 `execute_python` 和 `generate_report` 的 `ToolDefinition` 构建链末尾加：
```rust
.with_preserve_tool_use_results(true)
```

这两个工具的结果是后续步骤的数据来源：
- `execute_python`：返回 DataFrame 统计摘要、计算结论，下一轮分析依赖这些结果；
- `generate_report`：返回报告文件路径，后续引用或附件下载依赖该路径。

### X2-c：microcompact 感知 preserve 标志

**问题：** `microcompact` 函数签名为 `fn microcompact(messages: &[serde_json::Value], config: &MicrocompactConfig) -> MicrocompactResult`，messages 是 JSON Value，无法直接访问 `ToolDefinition`。

**方案：** 在 `MicrocompactConfig` 中新增一个工具名集合，列出所有需要保留结果的工具 ID：

```rust
pub struct MicrocompactConfig {
    pub trigger_chars: usize,
    pub keep_recent_tool_results: usize,
    /// 这些工具的结果即使超出 keep_recent_tool_results 窗口也不得被截断。
    pub preserved_tool_names: std::collections::HashSet<String>,
}
```

`Default` 实现从 `TOOL_CATALOG` 中收集 `preserve_tool_use_results == true` 的工具 ID 填充该集合：

```rust
impl Default for MicrocompactConfig {
    fn default() -> Self {
        use crate::runtime::tools::catalog::TOOL_CATALOG;
        let preserved = TOOL_CATALOG
            .all_ids()
            .into_iter()
            .filter(|id| {
                TOOL_CATALOG
                    .get(id)
                    .map(|def| def.preserve_tool_use_results)
                    .unwrap_or(false)
            })
            .collect();
        Self {
            trigger_chars: 120_000,
            keep_recent_tool_results: 2,
            preserved_tool_names: preserved,
        }
    }
}
```

`microcompact` 函数在决定是否清空工具结果时，先检查该条消息是否携带 `tool_name`（或 `name`）字段，若工具名在 `preserved_tool_names` 中则跳过。

**工具结果消息格式：** 当前 messages 是 `serde_json::Value`，工具结果的 `role` 为 `"tool"`，工具名字段视上层序列化结果而定；需在 `microcompact` 内部从 `tool_call_id` 或 `name` 字段提取，或通过 tool_use 消息（role=assistant）的 tool_calls 数组反查。如果消息中没有直接的工具名字段，则退化为：保留所有被标注为 preserve 的工具的 tool_result（通过查找 assistant 消息中 tool_calls 的 name 字段，建立 `tool_call_id → tool_name` 映射）。

**microcompact 修改逻辑摘要：**

```rust
// 1. 建立 tool_call_id → tool_name 映射
let mut id_to_name: HashMap<String, String> = HashMap::new();
for message in messages {
    if message["role"] == "assistant" {
        if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tool_calls {
                if let (Some(id), Some(name)) = (
                    tc.get("id").and_then(|v| v.as_str()),
                    tc.get("name").and_then(|v| v.as_str()),
                ) {
                    id_to_name.insert(id.to_string(), name.to_string());
                }
            }
        }
    }
}

// 2. 在清空前检查是否 preserve
if indices_to_clear.contains(&index) {
    let tool_call_id = message.get("tool_call_id").and_then(|v| v.as_str());
    let is_preserved = tool_call_id
        .and_then(|id| id_to_name.get(id))
        .map(|name| config.preserved_tool_names.contains(name))
        .unwrap_or(false);
    if is_preserved {
        return message.clone(); // 跳过截断
    }
}
```

**新增测试（单元测试在 `compaction.rs` mod tests 中）：**

1. `microcompact_skips_preserved_tool_results`：构造包含多轮 execute_python 结果和普通工具结果的 messages，触发 microcompact，断言 execute_python 的结果内容未被替换为 `[microcompacted]`，而普通工具结果被清空。
2. `microcompact_still_clears_non_preserved_results`：确认非 preserve 工具结果在超出窗口后正常清空。
3. `microcompact_config_default_includes_execute_python_and_generate_report`：断言 `MicrocompactConfig::default().preserved_tool_names` 包含 `execute_python` 和 `generate_report`。

**cargo test 命令：**
```bash
cd src-tauri && cargo test microcompact -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast
```

**commit message:** `feat(tool-def): add preserve_tool_use_results field and wire microcompact skip logic - X2`

---

## Task X3：ToolDefinition 新增 default_timeout_secs + 执行层消费

### X3-a：ToolDefinition 字段扩展

**文件：** `src-tauri/src/runtime/tools/definition.rs`

在 X2-a 的基础上继续新增字段与 builder：

```rust
pub struct ToolDefinition {
    // ... 已有字段 ...
    pub preserve_tool_use_results: bool,
    /// 工具执行默认超时（秒）。None 表示交由执行层自行决定。
    /// 当 LLM input 中未显式传入 timeout 参数时，执行层使用此值。
    /// 优先级：input 参数 > definition 级默认值 > 执行层硬编码默认值。
    pub default_timeout_secs: Option<u64>,
}
```

`ToolDefinition::new` 中初始化为 `None`。

新增 builder 方法：
```rust
pub fn with_default_timeout_secs(mut self, secs: u64) -> Self {
    self.default_timeout_secs = Some(secs);
    self
}
```

### X3-b：catalog 声明工具超时

**文件：** `src-tauri/src/runtime/tools/catalog.rs`

- `bash`：`.with_default_timeout_secs(120)` — 与现有 `DEFAULT_TIMEOUT_SECS` 对齐；
- `execute_python`：`.with_default_timeout_secs(600)` — 与 `llm/tool_executor/python.rs` L217 硬编码值对齐；
- `load_file`：`.with_default_timeout_secs(120)` — Python 解析有时会超时，给出合理默认值；
- `generate_report`、`generate_chart`：`.with_default_timeout_secs(300)` — 涉及 Python 渲染和文件转换，3 分钟；
- 其余工具无需声明（`None`，执行层自行决定）。

### X3-c：bash.rs 消费 definition 级超时

**文件：** `src-tauri/src/runtime/tools/builtin/bash.rs`

当前取值逻辑（L311-315）：
```rust
let timeout_secs = input
    .get("timeout_secs")
    .and_then(Value::as_u64)
    .unwrap_or(DEFAULT_TIMEOUT_SECS)
    .min(MAX_TIMEOUT_SECS);
```

改为三级优先级：input → definition → 硬编码默认：

```rust
// 1. input 参数最优先
// 2. 若无 input 参数则取 definition 级默认值
// 3. 最终回落到 DEFAULT_TIMEOUT_SECS
let def_timeout = ctx.tool_definition_timeout.unwrap_or(DEFAULT_TIMEOUT_SECS);
let timeout_secs = input
    .get("timeout_secs")
    .and_then(Value::as_u64)
    .unwrap_or(def_timeout)
    .min(MAX_TIMEOUT_SECS);
```

这里的 `ctx.tool_definition_timeout` 需要从执行上下文中取得（见下方 ToolExecutionContext 扩展）。

**替代方案（不改 ToolExecutionContext）：** 因为 `BashRuntimeTool::execute` 只能访问 `ToolExecutionContext` 中已有的字段，而当前 `ToolExecutionContext` 不携带工具定义元数据，更轻量的方案是：

在 `BashRuntimeTool` 的 `definition()` 返回值中包含 `default_timeout_secs`，在 `execute` 中通过 `TOOL_CATALOG.get("bash")` 取出 definition 后读取该值：

```rust
let def_timeout = crate::runtime::tools::catalog::TOOL_CATALOG
    .get("bash")
    .and_then(|def| def.default_timeout_secs)
    .unwrap_or(DEFAULT_TIMEOUT_SECS);
let timeout_secs = input
    .get("timeout_secs")
    .and_then(Value::as_u64)
    .unwrap_or(def_timeout)
    .min(MAX_TIMEOUT_SECS);
```

此方案无需修改 `ToolExecutionContext`，catalog 成为超时配置的单一真相源。

### X3-d：python.rs 消费 definition 级超时

**文件：** `src-tauri/src/llm/tool_executor/python.rs`（`handle_execute_python_core` 函数体内，L217 附近）

当前硬编码：
```rust
let timeout = Duration::from_secs(600);
```

改为从 catalog 取值（同 X3-c 思路）：

```rust
let timeout_secs = crate::runtime::tools::catalog::TOOL_CATALOG
    .get("execute_python")
    .and_then(|def| def.default_timeout_secs)
    .unwrap_or(600);
let timeout = Duration::from_secs(timeout_secs);
```

### X3 测试

**单元测试（在 `catalog.rs` 的 `#[cfg(test)]` 块中）：**

1. `catalog_bash_has_default_timeout_120`：`TOOL_CATALOG.get("bash").unwrap().default_timeout_secs == Some(120)`
2. `catalog_execute_python_has_default_timeout_600`：`TOOL_CATALOG.get("execute_python").unwrap().default_timeout_secs == Some(600)`
3. `catalog_generate_report_has_default_timeout_300`
4. `catalog_primitive_tools_have_no_default_timeout`：`list_directory`、`web_search` 等 primitive 工具的 `default_timeout_secs` 为 `None`

**review 测试（`src-tauri/tests/review_tool_timeout_declarations.rs`）：**

1. `review_long_running_tools_declare_timeout`：断言 `bash`、`execute_python`、`generate_report`、`generate_chart`、`load_file` 均已在 catalog 中声明 `default_timeout_secs`（`!= None`）。
2. `review_bash_timeout_in_catalog_matches_tool_constant`：断言 catalog 中 bash 的 `default_timeout_secs == Some(120)`，与 `DEFAULT_TIMEOUT_SECS` 一致。

**cargo test 命令：**
```bash
cd src-tauri && cargo test catalog_bash_has_default_timeout -- --nocapture
cd src-tauri && cargo test catalog_execute_python_has_default_timeout -- --nocapture
cd src-tauri && cargo test review_tool_timeout_declarations --tests --no-fail-fast -- --nocapture
```

**commit message:** `feat(tool-def): add default_timeout_secs field and wire catalog timeouts into bash/python execution - X3`

---

## 执行顺序

```
X1 → X2-a → X2-b → X2-c → X3-a → X3-b → X3-c → X3-d
```

X2-a 和 X3-a 均修改 `ToolDefinition`，必须合并到同一 commit 或按顺序执行（先 X2-a，X3-a 在其基础上追加字段）。

## 不做的事

- **PluginContext 全量迁移**：`REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 的工具仍通过 `try_build_request_scoped_tool` 按需构建，本计划不改变这一机制；
- **X2 不引入 per-call ToolUseContext**：preserve 判断通过 catalog 静态声明 + config 传递，不需要改 `ToolExecutionContext`；
- **X3 不改 ToolExecutionContext**：timeout 回落通过工具自身查 catalog 实现，不扩大 context 接口。

## 验收标准

1. `cargo test review_tool_pool_ordering --tests` 全绿；
2. `cargo test microcompact` 全绿，且包含 `microcompact_skips_preserved_tool_results` 测试；
3. `cargo test review_tool_timeout_declarations --tests` 全绿；
4. `cargo test review_ --tests --no-fail-fast` 全绿（无回归）；
5. `cargo build` 无新增 warning。
