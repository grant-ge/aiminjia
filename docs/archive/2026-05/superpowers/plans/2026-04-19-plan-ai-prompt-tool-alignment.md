# System Prompt & Tool Alignment Plan（Plan-AI）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从系统提示词里删掉废弃工具名，AI 不再提及无法使用的工具。

**Tech Stack:** Rust, cargo test

---

## 背景

lotus-app 当前的工具执行面分成两层：

1. **全局注册 RuntimeTool（8 个）**：`bash` / `read_workspace_file` / `write_file` / `edit_file` / `list_directory` / `search_files` / `get_file_info` / `grep_content`
2. **request-scoped RuntimeTool（仍然真实可执行）**：`web_search` / `browse_navigate` / `read_page_content` / `page_execute_js` / `extract_table_data` / `extract_with_pagination` / `load_file` / `browse_data` / `execute_python` / `generate_report` / `generate_chart`

其中第 2 类虽然不在 `register_runtime()` 的全局表里，但会在 `ToolRegistry::to_runtime_dispatcher()` / `get_schemas_filtered()` 中通过 request-scoped factory 暴露并执行，**不能误删**。

本计划真正要清理的是两类不对齐问题：

- **系统提示词中的失效工具名/过时工作流暗示**：例如 `save_memory` / `search_memory` / `plan_update` / `progress_update` / `save_analysis_note` / `hypothesis_test` / `detect_anomalies` / `slides_gen` / `export_data`
- **Analysis 模式带来的过度暴露**：当前 `is_analysis=true` 会走 `ToolFilter::All`，把不该给普通对话看到的 schema 一并暴露给 LLM；这与 `claude-code-best` 的单主循环 + 基于真实 tool pool 暴露工具的设计不一致

因此，这份计划的目标不是“只保留 8 个工具”，而是：

- prompt 不再提及已失效工具名
- 日常对话默认只暴露 daily allowlist
- 删除 lotus 自定义的 Analysis 模式分流，改为统一对话主路径

---

## 验收标准

在 AIjia 新建对话，输入：**"你有哪些工具？"**

**失败条件：** AI 回复中出现失效工具名（`save_memory` / `search_memory` / `plan_update` / `progress_update` / `save_analysis_note` / `hypothesis_test` / `detect_anomalies` / `slides_gen` / `export_data`）任意一个词即失败。

**通过条件：** 上述失效工具名一个都不出现；同时 daily 默认 tool surface 不再因为 Analysis 模式而暴露额外 schema。

---

## Task AI1：清理系统提示词中的失效工具名

- [ ] 在 `src-tauri/src/llm/prompts.rs` 中找到所有包含失效工具名的静态常量，替换为不绑定具体 tool id 的能力意图描述
- [ ] 在 `src-tauri/prompts/base.md` 中找到所有包含失效工具名或过时分析工作流暗示的行，替换为能力意图描述
- [ ] 保留仍真实可执行的 request-scoped tool 能力语义，不要把 `load_file` / `execute_python` / `web_search` 等有效能力误删成“不可用”
- [ ] 更新 `src-tauri/src/llm/prompts.rs` 中对应的测试断言，并补一个“system prompt 不包含失效工具名”的回归测试
- [ ] `cargo test --lib llm::prompts` 通过后 commit

---

## Task AI2：同步测试合约

- [ ] 在 `src-tauri/tests/skill_tool_contract_test.rs` 中找到 `DAILY_ALLOWED_TOOLS` 常量，与 `src-tauri/src/runtime/tools/catalog.rs` 中的 `DAILY_ALLOWED_TOOLS` 保持一致（daily 只含 8 个 allowlist 工具）
- [ ] 删除/修正仍把失效 analysis tools 当成有效 contract 的测试常量与断言
- [ ] `cargo test skill_tool_contract` 通过后 commit

---

## Task AI3：删除 Analysis 模式

Analysis 模式是 lotus-app 自己的概念，claude-code-best 没有这个模式。它通过 `is_analysis` 字段区分"数据分析工作流"和"日常助手"，用 `ToolFilter::All` 把所有工具（包括废弃工具的 schema）暴露给 LLM。

- [ ] 删除所有 `is_analysis` / `PromptMode::Analysis` / `resolve_request_is_analysis` 相关代码，并把 prompt/tool surface 收敛到统一 daily 主路径
- [ ] 删除 `llm/tools.rs` 中仅服务 Analysis 模式的函数（`get_tools_for_step`、`get_tool_definitions_for_step`）及其测试
- [ ] 删除 `conversation_mode` 存储字段相关读写与依赖它的 transport/orchestrator 流程
- [ ] 同步修正依赖 `is_analysis` 的 context decay / safeguard / metrics / tests，使其回到统一对话语义
- [ ] `cargo build` 和 `cargo test` 通过后 commit
