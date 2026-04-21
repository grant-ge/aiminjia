# Atomic Tool Harness Plan（Plan-AH）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 确保测试合约和系统提示词只包含有效工具，杜绝废弃工具名混入。

**Tech Stack:** Rust, cargo test

---

## 背景

lotus-app 当前只有以下 8 个工具真正注册进了 `ToolDispatcher`（通过 `register_runtime()` 注册），LLM 调用它们时能正常执行：

**有效工具：**
`bash` / `read_workspace_file` / `write_file` / `edit_file` / `list_directory` / `search_files` / `get_file_info` / `grep_content`

以下工具的 ToolPlugin 注册已关闭，也没有对应的 RuntimeTool 实现，LLM 调用它们会报错：

**废弃工具：**
`save_memory` / `search_memory` / `load_file` / `execute_python` / `generate_report` / `generate_chart` / `export_data` / `browse_data` / `web_search` / `browse_navigate` / `read_page_content` / `plan_update` / `progress_update` / `save_analysis_note` / `hypothesis_test` / `detect_anomalies` / `slides_gen`

当前 `src-tauri/tests/skill_tool_contract_test.rs` 中的 `DAILY_ALLOWED_TOOLS` 常量还包含废弃工具名，与 `src-tauri/src/runtime/tools/catalog.rs` 中的实际定义不一致。

---

## 验收标准

在 AIjia 新建对话，输入：**"你有哪些工具？"**

**失败条件：** AI 回复中出现废弃工具列表中任意一个词即失败。

**通过条件：** 废弃工具列表中的词一个都不出现。

---

## Task AH1：同步测试合约

- [ ] 在 `src-tauri/tests/skill_tool_contract_test.rs` 中找到 `DAILY_ALLOWED_TOOLS` 常量，更新为只包含 8 个有效工具，与 `src-tauri/src/runtime/tools/catalog.rs` 中的 `DAILY_ALLOWED_TOOLS` 保持一致
- [ ] `cargo test skill_tool_contract` 通过后 commit
