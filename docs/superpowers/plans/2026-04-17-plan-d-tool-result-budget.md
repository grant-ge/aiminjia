# 工具结果预算实施计划（Plan-D）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为每个工具声明结果大小上限，`collect_results` 使用工具声明的限制进行截断，TurnDriver 硬编码的 `8000` 替换为来自工具定义的 `max_result_size_chars`，防止超大输出撑爆 LLM 上下文窗口。

**Architecture:**
- D1 在 `RuntimeTool` trait 层新增 `max_result_size_chars()` 方法（默认 8000）
- D2 在 `ToolDefinition` struct 层新增 `default_max_result_size_chars: usize` 字段，`catalog.rs` 各工具按实际需要设置合理值
- D3 将 `chat_turn_driver.rs` 中硬编码的 `8000` 替换为通过 `ToolRoundResult` 携带的 per-tool limit，并更新截断消息格式
- D4 为 catalog 中所有工具设置合理值

各子任务顺序依赖：D1/D2 并行（无依赖），D3 依赖 D1，D4 依赖 D2。

**Tech Stack:** Rust

**Worktree branch:** `feat/tool-result-budget`

---

## 现状（Pre-D）

- `tool_result_collector::collect_results(round_results, max_tool_result_chars)` 已支持参数化截断，但调用方 `chat_turn_driver.rs:450` 将 `max_tool_result_chars` 硬编码为 `8000`
- 截断消息格式为 `"{}...\n[output truncated — {} chars total]"`，不包含 "Use a more specific query" 引导
- `ToolDefinition` struct 没有 `default_max_result_size_chars` 字段
- `RuntimeTool` trait 没有 `max_result_size_chars()` 方法
- catalog 中所有工具使用相同的隐式限制

**对标（claude-code-best）：**
- `Tool.ts` 中每个工具声明 `maxResultSizeChars: number`（约第 466 行）
- `toolResultStorage.ts` 中 `getPersistenceThreshold(toolName, declaredMaxResultSizeChars)` 读取工具声明的限制
- 截断（persist）后的消息格式为 `<persisted-output>\nOutput too large... Full output saved to: {filepath}\n\nPreview...`

本项目实现截断（不持久化到磁盘），但截断消息应对 LLM 更友好，引导 LLM 使用更精确的查询。

---

## Task D1：RuntimeTool trait 新增 `max_result_size_chars`

**文件：** `src-tauri/src/runtime/tools/dispatcher.rs`

### D1-Step 1：写失败测试

**文件：** `src-tauri/tests/tool_catalog_contract_test.rs`（追加到文件末尾）

```rust
// ── Plan-D1: RuntimeTool.max_result_size_chars ────────────────────────────

#[test]
fn runtime_tool_default_max_result_size_chars_is_8000() {
    use app_lib::runtime::tools::{RuntimeTool, ToolError, ToolExecutionContext, ToolResult};
    use async_trait::async_trait;
    use serde_json::Value;

    struct MinimalTool;

    #[async_trait]
    impl RuntimeTool for MinimalTool {
        fn definition(&self) -> app_lib::runtime::tools::definition::ToolDefinition {
            app_lib::runtime::tools::definition::ToolDefinition::new("minimal", "desc")
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("minimal", "ok", None))
        }
    }

    let tool = MinimalTool;
    assert_eq!(tool.max_result_size_chars(), 8000,
        "default max_result_size_chars must be 8000");
}

#[test]
fn runtime_tool_can_override_max_result_size_chars() {
    use app_lib::runtime::tools::{RuntimeTool, ToolError, ToolExecutionContext, ToolResult};
    use async_trait::async_trait;
    use serde_json::Value;

    struct LargeTool;

    #[async_trait]
    impl RuntimeTool for LargeTool {
        fn definition(&self) -> app_lib::runtime::tools::definition::ToolDefinition {
            app_lib::runtime::tools::definition::ToolDefinition::new("large_tool", "desc")
        }

        fn max_result_size_chars(&self) -> usize {
            32_000
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("large_tool", "ok", None))
        }
    }

    let tool = LargeTool;
    assert_eq!(tool.max_result_size_chars(), 32_000,
        "overridden max_result_size_chars must return 32000");
}
```

**运行（预期失败）：**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test runtime_tool_default_max_result_size_chars_is_8000 \
             runtime_tool_can_override_max_result_size_chars \
  --test tool_catalog_contract_test -- --nocapture
```

预期：编译错误 — `no method named max_result_size_chars found for type MinimalTool`

### D1-Step 2：最小实现

在 `dispatcher.rs` 的 `RuntimeTool` trait 中（`is_destructive` 方法之后，`check_permissions` 方法之前）添加：

```rust
fn max_result_size_chars(&self) -> usize {
    8000
}
```

### D1-Step 3：验证通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test runtime_tool_default_max_result_size_chars_is_8000 \
             runtime_tool_can_override_max_result_size_chars \
  --test tool_catalog_contract_test -- --nocapture
```

预期：2 个测试通过。

### D1-Step 4：回归检查

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test tool_catalog_contract_test -- --nocapture
```

- [ ] D1 完成

---

## Task D2：ToolDefinition 新增 `default_max_result_size_chars`

**文件：** `src-tauri/src/runtime/tools/definition.rs`

### D2-Step 1：写失败测试

**文件：** `src-tauri/tests/tool_catalog_contract_test.rs`（追加到 D1 测试之后）

```rust
// ── Plan-D2: ToolDefinition.default_max_result_size_chars ────────────────

#[test]
fn tool_definition_default_max_result_size_chars_is_8000() {
    use app_lib::runtime::tools::definition::ToolDefinition;

    let def = ToolDefinition::new("some_tool", "desc");
    assert_eq!(def.default_max_result_size_chars, 8000,
        "ToolDefinition must default default_max_result_size_chars to 8000");
}

#[test]
fn tool_definition_with_max_result_size_chars_sets_field() {
    use app_lib::runtime::tools::definition::ToolDefinition;

    let def = ToolDefinition::new("execute_python", "desc")
        .with_max_result_size_chars(32_000);
    assert_eq!(def.default_max_result_size_chars, 32_000,
        "with_max_result_size_chars must set the field");
}

#[test]
fn catalog_execute_python_has_32000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let def = TOOL_CATALOG.get("execute_python")
        .expect("execute_python must be in catalog");
    assert_eq!(def.default_max_result_size_chars, 32_000,
        "execute_python must declare 32000 char limit");
}

#[test]
fn catalog_list_directory_has_4000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let def = TOOL_CATALOG.get("list_directory")
        .expect("list_directory must be in catalog");
    assert_eq!(def.default_max_result_size_chars, 4_000,
        "list_directory must declare 4000 char limit");
}

#[test]
fn catalog_read_workspace_file_has_16000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let def = TOOL_CATALOG.get("read_workspace_file")
        .expect("read_workspace_file must be in catalog");
    assert_eq!(def.default_max_result_size_chars, 16_000,
        "read_workspace_file must declare 16000 char limit");
}

#[test]
fn catalog_search_files_has_4000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let def = TOOL_CATALOG.get("search_files")
        .expect("search_files must be in catalog");
    assert_eq!(def.default_max_result_size_chars, 4_000,
        "search_files must declare 4000 char limit");
}

#[test]
fn catalog_web_search_has_8000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let def = TOOL_CATALOG.get("web_search")
        .expect("web_search must be in catalog");
    assert_eq!(def.default_max_result_size_chars, 8_000,
        "web_search must declare 8000 char limit (default)");
}

#[test]
fn catalog_unlisted_tools_default_to_8000() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    // Support tools and others not explicitly listed should use the default 8000.
    for id in &["plan_update", "progress_update", "save_analysis_note"] {
        let def = TOOL_CATALOG.get(id)
            .unwrap_or_else(|| panic!("{} must be in catalog", id));
        assert_eq!(def.default_max_result_size_chars, 8_000,
            "{} must use default 8000 char limit", id);
    }
}
```

**运行（预期失败）：**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test tool_definition_default_max_result_size_chars_is_8000 \
             tool_definition_with_max_result_size_chars_sets_field \
             catalog_execute_python_has_32000_limit \
             catalog_list_directory_has_4000_limit \
             catalog_read_workspace_file_has_16000_limit \
             catalog_search_files_has_4000_limit \
             catalog_web_search_has_8000_limit \
             catalog_unlisted_tools_default_to_8000 \
  --test tool_catalog_contract_test -- --nocapture
```

预期：编译错误 — `no field default_max_result_size_chars on type ToolDefinition`

### D2-Step 2：最小实现（分三步）

**2a. `definition.rs`**：在 `ToolDefinition` struct 中添加字段和构建方法

在 `default_destructive: bool,` 之后添加：
```rust
pub default_max_result_size_chars: usize,
```

在 `Self { ... }` 构造块（`new` 方法）中，`default_destructive: false,` 之后添加：
```rust
default_max_result_size_chars: 8000,
```

在 `with_destructive` 方法之后添加：
```rust
/// 设置工具结果的最大字符数限制。
pub fn with_max_result_size_chars(mut self, limit: usize) -> Self {
    self.default_max_result_size_chars = limit;
    self
}
```

**2b. `catalog.rs`**：为各工具设置非默认限制

| 工具 ID | 限制（chars） | 理由 |
|---|---|---|
| `list_directory` | 4000 | 目录列表结构化，不需大 |
| `read_workspace_file` | 16000 | 文件内容，中等 |
| `search_files` | 4000 | 文件名列表，不需大 |
| `execute_python` | 32000 | Python 输出可能含大量数据 |
| 其他（默认）| 8000 | — |

在 `catalog.rs` 的 `build_default_catalog` 中，以下工具的 `ToolDefinition::new(...)` builder chain 末尾追加 `.with_max_result_size_chars(N)`：

- `list_directory`：`.with_max_result_size_chars(4_000)`
- `read_workspace_file`：`.with_max_result_size_chars(16_000)`
- `search_files`：`.with_max_result_size_chars(4_000)`
- `execute_python`：`.with_max_result_size_chars(32_000)`

其余工具不追加（使用默认 8000）。

### D2-Step 3：验证通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test tool_definition_default_max_result_size_chars_is_8000 \
             tool_definition_with_max_result_size_chars_sets_field \
             catalog_execute_python_has_32000_limit \
             catalog_list_directory_has_4000_limit \
             catalog_read_workspace_file_has_16000_limit \
             catalog_search_files_has_4000_limit \
             catalog_web_search_has_8000_limit \
             catalog_unlisted_tools_default_to_8000 \
  --test tool_catalog_contract_test -- --nocapture
```

预期：8 个测试通过。

### D2-Step 4：回归检查

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test tool_catalog_contract_test -- --nocapture
```

- [ ] D2 完成

---

## Task D3：collect_results 使用 per-tool 限制 + 改善截断消息

**文件：**
- `src-tauri/src/runtime/chat/tool_round_driver.rs`（携带 per-tool 限制到结果）
- `src-tauri/src/runtime/chat/tool_result_collector.rs`（使用 per-tool 限制）
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`（移除硬编码 8000）

### D3-Step 1：写失败测试

> 本 Task 有两个测试点：
> 1. 截断消息格式（unit test in `tool_result_collector.rs` 内置测试）
> 2. per-tool 限制被正确使用（integration test）

**追加到 `src-tauri/tests/s4_driver_loop_test.rs`**：

```rust
// ── Plan-D3: per-tool limit via ToolRoundResult ────────────────────────────

#[test]
fn collect_results_truncation_message_includes_guidance() {
    use app_lib::runtime::chat::tool_result_collector::collect_results;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
    use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;

    let long = "x".repeat(10_000);
    let results = vec![
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc1".to_string(),
            tool_name: "search_files".to_string(),
            content: long.clone(),
            is_error: false,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        }),
    ];
    // search_files limit = 4000
    let out = collect_results(results, 4000);
    let content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert!(content.contains("Use a more specific query"),
        "truncation message must include guidance for LLM, got: {}", content);
    assert!(content.len() < long.len(),
        "truncated content must be shorter than original");
}

#[test]
fn collect_results_respects_per_tool_limit_smaller_than_default() {
    use app_lib::runtime::chat::tool_result_collector::collect_results;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
    use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;

    // Content is 6000 chars — under default 8000 but over list_directory limit 4000.
    let content_6k = "d".repeat(6000);
    let results = vec![
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc1".to_string(),
            tool_name: "list_directory".to_string(),
            content: content_6k.clone(),
            is_error: false,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        }),
    ];
    // Pass the per-tool limit (4000) rather than the old hardcoded 8000.
    let out = collect_results(results, 4_000);
    let result_content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert!(result_content.contains("[Output truncated"),
        "content exceeding per-tool limit must be truncated, got: {}", result_content);
}

#[test]
fn collect_results_does_not_truncate_within_per_tool_limit() {
    use app_lib::runtime::chat::tool_result_collector::collect_results;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
    use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;

    // Content is 5000 chars — under execute_python limit 32000.
    let content_5k = "p".repeat(5000);
    let results = vec![
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc1".to_string(),
            tool_name: "execute_python".to_string(),
            content: content_5k.clone(),
            is_error: false,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        }),
    ];
    // Pass the per-tool limit (32000).
    let out = collect_results(results, 32_000);
    let result_content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert!(!result_content.contains("[Output truncated"),
        "content within per-tool limit must NOT be truncated, got: {}", result_content);
    assert_eq!(result_content.len(), 5000,
        "content within limit must be returned unchanged");
}
```

**运行（预期失败）：**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test collect_results_truncation_message_includes_guidance \
             collect_results_respects_per_tool_limit_smaller_than_default \
             collect_results_does_not_truncate_within_per_tool_limit \
  --test s4_driver_loop_test -- --nocapture
```

预期：`collect_results_truncation_message_includes_guidance` 失败（截断消息不含 "Use a more specific query"），其余两个通过（截断逻辑本身正确，只是 caller 传入了什么 limit）。

### D3-Step 2：最小实现

**2a. 更新截断消息格式（`tool_result_collector.rs`）**

将（约第 147-151 行）：
```rust
let truncated_result = if tr_content.len() > max_tool_result_chars {
    let end = truncate_at_char_boundary(tr_content, max_tool_result_chars);
    format!(
        "{}...\n[output truncated — {} chars total]",
        &tr_content[..end],
        tr_content.len()
    )
```

替换为：
```rust
let truncated_result = if tr_content.len() > max_tool_result_chars {
    let end = truncate_at_char_boundary(tr_content, max_tool_result_chars);
    format!(
        "{}\n[Output truncated: exceeded {} char limit. Use a more specific query to get smaller results.]",
        &tr_content[..end],
        max_tool_result_chars,
    )
```

注意：移除了 `...`（省略号），使截断点更清晰；消息格式与 claude-code-best 对齐。

**2b. 移除 `chat_turn_driver.rs` 中的硬编码 8000**

在 D3 实现之前，`ToolRoundResult` 还不携带 per-tool limit。当前先做简单修复：

在 `chat_turn_driver.rs` 第 450 行（`tool_result_collector::collect_results(round_results, 8000)`），将参数更改为从 TurnConfig 获取的值（或保持 8000 作为 per-turn 上限的占位，D3 完整实现会在 D4 后续 Task 中由工具 dispatcher 注入）。

**说明：** 完整的 per-tool limit 传递需要修改 `ToolRoundResult` 类型以携带工具执行时声明的 limit，然后由 `collect_results` 的调用方按工具名查询 catalog。这属于更大的重构。本 Task D3 目标是：

1. 修正截断消息文案（立即有效）
2. 在 `collect_results` 的内置 unit test 中验证新格式
3. 新增集成测试验证 per-tool 参数被正确传递（当 caller 传入正确值时）

`chat_turn_driver.rs` 中调用 `collect_results` 的地方暂时保持传入 `8000`，因为 per-tool limit 的动态注入需要 `ToolDispatcher` 与 `ToolRoundResult` 的协作（属于 D3 扩展，见下文 D3-Step-2c）。

**2c. `ToolRoundResult` 携带 per-tool limit（可选扩展）**

如需完整实现动态 per-tool limit，需要：

1. `tool_round_types.rs` 的 `RuntimeToolCallOutcome::Completed` 变体新增 `max_result_size_chars: usize` 字段（默认 8000）
2. `ToolRoundDriver::execute_round` 在 dispatch 完成后，从 `ToolDispatcher` 查询工具定义的 `default_max_result_size_chars`，并写入 outcome
3. `tool_result_collector.rs` 的 `collect_results` 从每个 `ToolRoundResult` 读取对应的 `max_result_size_chars`，而非使用全局参数

由于此扩展需要跨多个文件协调，为保证 TDD 节奏，本 Task 的最小实现只更新消息格式，完整的 per-tool limit 动态注入作为可选扩展在同一 commit 中完成（若实现）或推迟。

### D3-Step 3：验证通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test collect_results_truncation_message_includes_guidance \
             collect_results_respects_per_tool_limit_smaller_than_default \
             collect_results_does_not_truncate_within_per_tool_limit \
  --test s4_driver_loop_test -- --nocapture
```

预期：3 个测试通过。

### D3-Step 4：更新内置单元测试（`tool_result_collector.rs`）

D3 修改了截断消息格式，需要同步更新 `tool_result_collector.rs` 末尾的内置测试 `long_content_is_truncated`，将断言改为匹配新格式：

**旧断言（约第 243 行）：**
```rust
assert!(content.contains("[output truncated"));
```

**新断言：**
```rust
assert!(content.contains("[Output truncated"));
assert!(content.contains("Use a more specific query"));
```

### D3-Step 5：回归检查

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast && \
  cargo test --test s4_driver_loop_test -- --nocapture && \
  cargo test --test tool_catalog_contract_test -- --nocapture
```

- [ ] D3 完成

---

## Task D4：catalog 各工具 max_result_size_chars 完整验证

**目标：** 验证 catalog 中所有工具都拥有合理的 `default_max_result_size_chars` 值，并通过回归测试固化 contract。

### D4-Step 1：写失败测试

**追加到 `src-tauri/tests/tool_catalog_contract_test.rs`**：

```rust
// ── Plan-D4: catalog max_result_size_chars contract ──────────────────────

#[test]
fn catalog_all_tools_have_nonzero_max_result_size_chars() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    // 所有工具都必须有大于 0 的限制。
    for id in TOOL_CATALOG.all_ids() {
        let def = TOOL_CATALOG.get(&id).unwrap();
        assert!(def.default_max_result_size_chars > 0,
            "tool {} must have a positive max_result_size_chars", id);
    }
}

#[test]
fn catalog_power_tools_have_larger_limit_than_primitive_list_tools() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let execute_python = TOOL_CATALOG.get("execute_python").unwrap();
    let list_directory = TOOL_CATALOG.get("list_directory").unwrap();
    let search_files = TOOL_CATALOG.get("search_files").unwrap();

    assert!(
        execute_python.default_max_result_size_chars
            > list_directory.default_max_result_size_chars,
        "execute_python ({}) must have larger limit than list_directory ({})",
        execute_python.default_max_result_size_chars,
        list_directory.default_max_result_size_chars,
    );
    assert!(
        execute_python.default_max_result_size_chars
            > search_files.default_max_result_size_chars,
        "execute_python ({}) must have larger limit than search_files ({})",
        execute_python.default_max_result_size_chars,
        search_files.default_max_result_size_chars,
    );
}

#[test]
fn catalog_read_workspace_file_limit_larger_than_list_directory() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    let rwf = TOOL_CATALOG.get("read_workspace_file").unwrap();
    let ld = TOOL_CATALOG.get("list_directory").unwrap();
    assert!(
        rwf.default_max_result_size_chars > ld.default_max_result_size_chars,
        "read_workspace_file ({}) must have larger limit than list_directory ({})",
        rwf.default_max_result_size_chars,
        ld.default_max_result_size_chars,
    );
}

#[test]
fn catalog_max_result_size_chars_snapshot() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    // Snapshot 测试：固化关键工具的已知值。任何意外修改都会让此测试失败。
    let expected: &[(&str, usize)] = &[
        ("list_directory",      4_000),
        ("search_files",        4_000),
        ("read_workspace_file", 16_000),
        ("web_search",          8_000),
        ("execute_python",      32_000),
        ("load_file",           8_000),
        ("browse_navigate",     8_000),
        ("read_page_content",   8_000),
        ("plan_update",         8_000),
        ("progress_update",     8_000),
        ("save_analysis_note",  8_000),
    ];

    for (id, expected_limit) in expected {
        let def = TOOL_CATALOG.get(id)
            .unwrap_or_else(|| panic!("tool {} must be in catalog", id));
        assert_eq!(
            def.default_max_result_size_chars,
            *expected_limit,
            "tool {} expected limit {}, got {}",
            id,
            expected_limit,
            def.default_max_result_size_chars,
        );
    }
}
```

**运行（预期失败）：**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test catalog_all_tools_have_nonzero_max_result_size_chars \
             catalog_power_tools_have_larger_limit_than_primitive_list_tools \
             catalog_read_workspace_file_limit_larger_than_list_directory \
             catalog_max_result_size_chars_snapshot \
  --test tool_catalog_contract_test -- --nocapture
```

预期：若 D2 已完成，则大部分通过；`catalog_max_result_size_chars_snapshot` 可能因 `load_file` 等工具未设置值而失败（使用默认 8000，但 snapshot 期望也是 8000，因此全部通过）。

若 D2 未完成（无 `default_max_result_size_chars` 字段），所有测试编译失败。

### D4-Step 2：确认 D2 已完成，此步骤无额外代码修改

D2 已经为所有需要非默认值的工具设置了 `.with_max_result_size_chars(N)`。D4 测试纯粹是 contract 回归测试。

若 D4 测试失败，意味着 D2 实现有遗漏，需要回到 D2 补充。

### D4-Step 3：验证通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test catalog_all_tools_have_nonzero_max_result_size_chars \
             catalog_power_tools_have_larger_limit_than_primitive_list_tools \
             catalog_read_workspace_file_limit_larger_than_list_directory \
             catalog_max_result_size_chars_snapshot \
  --test tool_catalog_contract_test -- --nocapture
```

预期：4 个测试通过。

### D4-Step 4：全量回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test review_ --tests --no-fail-fast
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test tool_catalog_contract_test -- --nocapture && \
  cargo test --test s4_driver_loop_test -- --nocapture && \
  cargo test --test tool_dispatcher_test -- --nocapture
```

- [ ] D4 完成

---

## Commit 顺序

每个 Task 完成后单独 commit，按以下顺序：

1. **D1 commit**：`feat(tools): RuntimeTool trait 新增 max_result_size_chars() 默认方法`
2. **D2 commit**：`feat(tools): ToolDefinition 新增 default_max_result_size_chars + catalog 各工具设置合理值`
3. **D3 commit**：`feat(collector): 截断消息格式更新，引导 LLM 使用更精确查询`
4. **D4 commit**：`test(catalog): contract 测试固化工具结果大小限制 snapshot`

---

## 验收标准

完成后满足：

1. `RuntimeTool` trait 有 `fn max_result_size_chars(&self) -> usize { 8000 }` 默认实现
2. `ToolDefinition` struct 有 `pub default_max_result_size_chars: usize` 字段，`ToolDefinition::new()` 默认值 8000，提供 `with_max_result_size_chars(usize)` builder
3. catalog 中的工具限制符合以下约束：`execute_python` (32000) > `read_workspace_file` (16000) > `web_search`/其他 (8000) > `list_directory`/`search_files` (4000)
4. 截断消息包含 `"[Output truncated: exceeded N char limit. Use a more specific query to get smaller results.]"`
5. 所有 `review_*` 回归测试通过
6. `tool_catalog_contract_test`、`s4_driver_loop_test` 全量通过

---

## 遗留 Gap（本计划范围外）

- **per-turn 全局预算追踪**：`TurnIterationState.content_budget_used` 跨 round 累计工具结果用量，超过 per-turn 预算时提前终止。本计划聚焦 per-tool 限制声明和静态截断；全局动态预算追踪复杂度更高，建议作为 Plan-E 独立实现。
- **动态 per-tool limit 注入**：`chat_turn_driver.rs` 目前仍传入固定值（8000 或 per-tool 值）。完整实现需要 `ToolRoundResult` 携带执行时的 `max_result_size_chars`，从 `ToolDispatcher` 查询 catalog 注入。详见 D3-Step-2c。
- **LegacyToolAdapter 的 `max_result_size_chars`**：旧 `ToolPlugin` 适配器尚未实现此接口；当前通过 `LegacyToolAdapter` 路由的工具仍使用默认 8000，直到 plugin 系统向 RuntimeTool 迁移完成。
