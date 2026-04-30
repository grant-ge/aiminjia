# 工具结果预算实施计划（Plan-D）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为工具结果引入“按工具声明”的大小上限，移除 runtime chat path 中硬编码的 `8000`，让截断行为由工具声明驱动，而不是由 `TurnDriver` 猜测；同时把截断提示文案改成更接近 `claude-code-best` 的 LLM 友好格式。

**对标结论（相对 claude-code-best）：**
- `claude-code-best` 的单一真相源是工具声明上的 `maxResultSizeChars`（见 `src/Tool.ts`），不是 driver 内部硬编码。
- `claude-code-best` 真正的完整方案还包括：
  - `toolResultStorage.ts` 的单工具 threshold / persistence
  - `query.ts` 的 per-message aggregate budget（`applyToolResultBudget`）
- lotus-app 当前阶段**不做磁盘持久化**、**不做 per-message aggregate budget**；本计划只完成第一阶段：
  1. 在 lotus 的工具声明层声明 per-tool limit
  2. 在 dispatcher/query/result collector 之间把这个 limit 真实传递下去
  3. 用该 limit 截断工具结果并生成统一提示文案

**Architecture（修订版）：**
- D1：在 `ToolDefinition` 增加 `default_max_result_size_chars` 字段与 builder；它是 lotus-app 的**单一声明源**。
- D2：`ToolDispatcher -> QueryEngine -> RuntimeToolCallOutcome` 逐层携带工具声明的 `max_result_size_chars`，避免 `collect_results` 再次猜测或查全局硬编码。
- D3：`tool_result_collector::collect_results` 改为读取每个结果自带的 limit，并更新截断提示文案；`RuntimeChatTurnDriver` 不再传 `8000`。
- D4：在 `catalog.rs` 为关键工具设置合理值，并通过 contract/snapshot 测试固化。

**核心架构约束：**
1. `ToolDefinition.default_max_result_size_chars` 是 lotus-app 的单一真相源；不要再额外引入第二套独立配置。
2. `runtime/` 目录禁止 `use tauri::*`。
3. `tool_result_collector.rs` 保持纯数据变换；不要在里面引入持久化、masking 或 transport 逻辑。
4. 本计划只覆盖 `runtime::chat` 主链路；`chat_runtime_impl.rs` 里的遗留截断分支若仍存在，后续应继续迁移/收敛，不在本计划内引入新的双实现。

---

## Task D1：ToolDefinition 声明 per-tool result limit

**文件：** `src-tauri/src/runtime/tools/definition.rs`

### D1-Step 1：写失败测试

**追加到** `src-tauri/tests/tool_catalog_contract_test.rs`

```rust
#[test]
fn tool_definition_default_max_result_size_chars_is_8000() {
    use app_lib::runtime::tools::definition::ToolDefinition;

    let def = ToolDefinition::new("some_tool", "desc");
    assert_eq!(def.default_max_result_size_chars, 8_000);
}

#[test]
fn tool_definition_with_max_result_size_chars_sets_field() {
    use app_lib::runtime::tools::definition::ToolDefinition;

    let def = ToolDefinition::new("execute_python", "desc")
        .with_max_result_size_chars(32_000);
    assert_eq!(def.default_max_result_size_chars, 32_000);
}
```

**运行（预期失败）：**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test tool_definition_default_max_result_size_chars_is_8000 \
             tool_definition_with_max_result_size_chars_sets_field \
  --test tool_catalog_contract_test -- --nocapture
```

### D1-Step 2：最小实现

在 `ToolDefinition` 中新增：
- 字段 `pub default_max_result_size_chars: usize`
- `ToolDefinition::new()` 默认值 `8_000`
- builder：`.with_max_result_size_chars(limit)`

### D1-Step 3：验证通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test tool_definition_default_max_result_size_chars_is_8000 \
             tool_definition_with_max_result_size_chars_sets_field \
  --test tool_catalog_contract_test -- --nocapture
```

- [ ] D1 完成

---

## Task D2：dispatch/query/outcome 真实携带 per-tool limit

**文件：**
- `src-tauri/src/runtime/tools/dispatcher.rs`
- `src-tauri/src/runtime/query_engine.rs`
- `src-tauri/src/runtime/chat/tool_round_types.rs`
- 相关测试文件

### D2-Step 1：写失败测试

**追加到** `src-tauri/tests/tool_dispatcher_test.rs`

```rust
#[tokio::test]
async fn dispatch_completed_outcome_carries_declared_max_result_size_chars() {
    use app_lib::runtime::tools::{ToolDispatchOutcome, ToolDispatcher, AllowAllPermissionPipeline};
    use std::sync::Arc;

    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(EchoTool));

    let ctx = app_lib::runtime::tools::ToolExecutionContext::for_test("conv", "run", "tc");
    let result = dispatcher.dispatch("echo", serde_json::json!({}), ctx).await;

    match result.expect("dispatch ok") {
        ToolDispatchOutcome::Completed { max_result_size_chars, .. } => {
            assert_eq!(max_result_size_chars, 12_345);
        }
        other => panic!("expected Completed, got {:?}", other),
    }
}
```

> 注：测试内的 `EchoTool::definition()` 需返回 `.with_max_result_size_chars(12_345)`。

**追加到** `src-tauri/tests/s4_driver_loop_test.rs`

```rust
#[test]
fn runtime_tool_call_outcome_exposes_declared_max_result_size_chars() {
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

    let outcome = RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc1".to_string(),
        tool_name: "echo".to_string(),
        content: "ok".to_string(),
        is_error: false,
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 12_345,
    };

    assert_eq!(outcome.max_result_size_chars(), 12_345);
}
```

**运行（预期失败）：**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test dispatch_completed_outcome_carries_declared_max_result_size_chars \
             runtime_tool_call_outcome_exposes_declared_max_result_size_chars \
  --tests -- --nocapture
```

### D2-Step 2：最小实现

- `ToolDispatchOutcome::Completed` 增加 `max_result_size_chars: usize`
- `ToolDispatcher::dispatch()` 在读取 `definition` 后，把 `definition.default_max_result_size_chars` 带回 `Completed`
- `RuntimeToolCallOutcome::Completed` 增加 `max_result_size_chars: usize`
- `QueryEngine::run_tool_call_with_bus()` 将 dispatch outcome 的 limit 透传到 runtime outcome
- 在 `RuntimeToolCallOutcome` 上新增 helper：

```rust
pub fn max_result_size_chars(&self) -> usize {
    match self {
        Self::Completed { max_result_size_chars, .. } => *max_result_size_chars,
        Self::AskRequired { .. } => 8_000,
    }
}
```

### D2-Step 3：验证通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test dispatch_completed_outcome_carries_declared_max_result_size_chars \
             runtime_tool_call_outcome_exposes_declared_max_result_size_chars \
  --tests -- --nocapture
```

- [ ] D2 完成

---

## Task D3：collector 按结果自带 limit 截断 + 更新提示文案

**文件：**
- `src-tauri/src/runtime/chat/tool_result_collector.rs`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- `src-tauri/tests/s4_driver_loop_test.rs`

### D3-Step 1：写失败测试

**追加到** `src-tauri/tests/s4_driver_loop_test.rs`

```rust
#[test]
fn collect_results_truncation_message_includes_guidance() {
    use app_lib::runtime::chat::tool_result_collector::collect_results;
    use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

    let long = "x".repeat(10_000);
    let results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc1".to_string(),
        tool_name: "search_files".to_string(),
        content: long,
        is_error: false,
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 4_000,
    })];

    let out = collect_results(results);
    let content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert!(content.contains("Use a more specific query"));
    assert!(content.contains("[Output truncated:"));
}

#[test]
fn collect_results_uses_per_result_limit_not_global_default() {
    use app_lib::runtime::chat::tool_result_collector::collect_results;
    use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

    let content_6k = "d".repeat(6_000);
    let results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc1".to_string(),
        tool_name: "list_directory".to_string(),
        content: content_6k,
        is_error: false,
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 4_000,
    })];

    let out = collect_results(results);
    let content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert!(content.contains("[Output truncated:"));
}

#[test]
fn collect_results_keeps_content_within_declared_limit() {
    use app_lib::runtime::chat::tool_result_collector::collect_results;
    use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

    let content_5k = "p".repeat(5_000);
    let results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc1".to_string(),
        tool_name: "execute_python".to_string(),
        content: content_5k,
        is_error: false,
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 32_000,
    })];

    let out = collect_results(results);
    let content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert_eq!(content.len(), 5_000);
    assert!(!content.contains("[Output truncated:"));
}
```

**运行（预期失败）：**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test collect_results_truncation_message_includes_guidance \
             collect_results_uses_per_result_limit_not_global_default \
             collect_results_keeps_content_within_declared_limit \
  --test s4_driver_loop_test -- --nocapture
```

### D3-Step 2：最小实现

- `tool_result_collector::collect_results` 签名改为：

```rust
pub fn collect_results(round_results: Vec<ToolRoundResult>) -> ToolRoundResults
```

- 每个 `ToolRoundResult::Ok(outcome)` 改用 `outcome.max_result_size_chars()` 作为 limit
- 截断文案更新为：

```text
[Output truncated: exceeded N char limit. Use a more specific query to get smaller results.]
```

- `chat_turn_driver.rs` 改为 `tool_result_collector::collect_results(round_results)`，不再传 `8000`
- 同步更新 `tool_result_collector.rs` 内置测试 `long_content_is_truncated`

### D3-Step 3：验证通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test collect_results_truncation_message_includes_guidance \
             collect_results_uses_per_result_limit_not_global_default \
             collect_results_keeps_content_within_declared_limit \
  --test s4_driver_loop_test -- --nocapture
```

- [ ] D3 完成

---

## Task D4：catalog 关键工具 limit contract 固化

**文件：**
- `src-tauri/src/runtime/tools/catalog.rs`
- `src-tauri/tests/tool_catalog_contract_test.rs`

### D4-Step 1：写失败测试

**追加到** `src-tauri/tests/tool_catalog_contract_test.rs`

```rust
#[test]
fn catalog_execute_python_has_32000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("execute_python").unwrap();
    assert_eq!(def.default_max_result_size_chars, 32_000);
}

#[test]
fn catalog_read_workspace_file_has_16000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("read_workspace_file").unwrap();
    assert_eq!(def.default_max_result_size_chars, 16_000);
}

#[test]
fn catalog_list_directory_has_4000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("list_directory").unwrap();
    assert_eq!(def.default_max_result_size_chars, 4_000);
}

#[test]
fn catalog_search_files_has_4000_limit() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;
    let def = TOOL_CATALOG.get("search_files").unwrap();
    assert_eq!(def.default_max_result_size_chars, 4_000);
}

#[test]
fn catalog_other_tools_default_to_8000_when_not_overridden() {
    use app_lib::runtime::tools::catalog::TOOL_CATALOG;

    for id in ["web_search", "plan_update", "progress_update", "save_analysis_note"] {
        let def = TOOL_CATALOG.get(id).unwrap();
        assert_eq!(def.default_max_result_size_chars, 8_000, "{} should default to 8000", id);
    }
}
```

**运行（预期失败）：**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test catalog_execute_python_has_32000_limit \
             catalog_read_workspace_file_has_16000_limit \
             catalog_list_directory_has_4000_limit \
             catalog_search_files_has_4000_limit \
             catalog_other_tools_default_to_8000_when_not_overridden \
  --test tool_catalog_contract_test -- --nocapture
```

### D4-Step 2：最小实现

在 `catalog.rs` 为以下工具设置非默认值：
- `list_directory` → `4_000`
- `search_files` → `4_000`
- `read_workspace_file` → `16_000`
- `execute_python` → `32_000`

其他工具保持默认 `8_000`。

### D4-Step 3：验证通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test catalog_execute_python_has_32000_limit \
             catalog_read_workspace_file_has_16000_limit \
             catalog_list_directory_has_4000_limit \
             catalog_search_files_has_4000_limit \
             catalog_other_tools_default_to_8000_when_not_overridden \
  --test tool_catalog_contract_test -- --nocapture
```

### D4-Step 4：全量回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
  cargo test --test tool_catalog_contract_test -- --nocapture && \
  cargo test --test tool_dispatcher_test -- --nocapture && \
  cargo test --test s4_driver_loop_test -- --nocapture && \
  cargo test review_ --tests --no-fail-fast
```

- [ ] D4 完成

---

## 推荐 commit 顺序

1. `feat(tools): declare per-tool result size in tool definition`
2. `feat(runtime): thread declared tool result limits through dispatcher`
3. `feat(runtime): use per-tool result budgets in collector`
4. `test(catalog): lock tool result budget contracts`

---

## 验收标准

完成后必须满足：

1. `ToolDefinition::new()` 默认声明 `default_max_result_size_chars = 8_000`
2. `ToolDispatcher::dispatch()` 返回的 `Completed` outcome 带有声明的 limit
3. `QueryEngine::run_tool_call_with_bus()` 透传该 limit 到 `RuntimeToolCallOutcome::Completed`
4. `tool_result_collector::collect_results()` 不再依赖外部传入的固定 `8000`
5. 截断消息包含：`Use a more specific query to get smaller results.`
6. `execute_python (32k) > read_workspace_file (16k) > default (8k) > list/search (4k)`
7. `review_` 回归、`tool_catalog_contract_test`、`tool_dispatcher_test`、`s4_driver_loop_test` 全绿

---

## 本计划明确不做的事（下一批架构债）

- `claude-code-best` 的磁盘持久化结果替换（`toolResultStorage.ts`）
- `claude-code-best` 的 per-message aggregate tool-result budget（`applyToolResultBudget`）
- 针对 `Infinity` / opt-out 工具的特殊预算策略
- `chat_runtime_impl.rs` 与 `runtime::chat` 的双路径彻底收敛

这些属于下一批架构债，应在后续 plan 中继续清理，而不是在本计划里把边界做糊。
