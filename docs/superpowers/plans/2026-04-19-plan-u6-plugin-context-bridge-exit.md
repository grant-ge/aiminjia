# PluginContext 热路径退出与 Request-Scoped Tool 运行时化（Plan-U6）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:test-driven-development` — 先锁住 request-scoped tool 与 transport 主路径，再拆桥。 REQUIRED SUB-SKILL: `superpowers:verification-before-completion` — 必须证明主路径不再依赖 `PluginContext` 才能关闭任务。

**Goal:** 让 lotus 的 request-scoped tool 与 transport 主路径不再依赖全能 `PluginContext`，把 `ToolRegistry::execute()` 和 legacy adapter 收缩成受控迁移岛，为后续 worker runtime-first 打基础。

**Architecture:** 持续对齐 `RuntimeTool + ToolExecutionContext + CapabilityContext` 的 runtime-first 设计；`PluginContext` 只保留给尚未迁移的 legacy `ToolPlugin`。request-scoped tool 通过显式 `Deps` / `Capability` 注入获得最小依赖，不再把 session 全上下文打包透传。

**Tech Stack:** Rust, async runtime

**Worktree branch:** pzc

---

## 背景与现状

| 文件 | 现状 |
|---|---|
| `src-tauri/src/plugin/context.rs` | 文档已经标注它是 legacy full-service-locator，但生产代码仍大量依赖它拼运行时上下文 |
| `src-tauri/src/plugin/registry.rs` | `execute()` 虽标注 deprecated，仍能在生产链路做 `RuntimeTool -> request-scoped factory -> legacy ToolPlugin` 三段桥接 |
| `src-tauri/src/plugin/registry.rs` | `to_runtime_dispatcher(plugin_ctx)` 仍靠 `PluginContext` 构造 request-scoped runtime tools |
| `src-tauri/src/runtime/tools/legacy_adapter.rs` | legacy adapter 继续把全量 `PluginContext` 暴露给旧工具，实现边界仍然过宽 |

### 当前问题

- request-scoped tool 需要什么依赖，现在不是显式接口，而是“从 `PluginContext` 里自己拿”。
- transport / subagent / tool runtime 很难确认谁真正拥有某条能力边界。
- 只要这条桥还在，后续权限、worker、memory 的 runtime-first 收口都容易倒回去。

## 范围

- 纳入：
  - request-scoped tool 的显式依赖建模
  - `ToolRegistry` 热路径收口
  - review 约束：禁止新 runtime 模块继续引 `PluginContext`
- 不纳入：
  - 一次性删除所有 legacy `ToolPlugin`
  - 提示词里的失效工具清理（那是 `Plan-AI`）
  - 新增更多 request-scoped tool 能力

## 任务拆分

### U6-1：盘点并切分 request-scoped tool 依赖

- [ ] 盘点当前仍经由 `PluginContext` 动态构造的 request-scoped runtime tools：`web_search`、`browse_*`、`load_file`、`execute_python`、`generate_report`、`generate_chart` 等。
- [ ] 为仍保留的工具定义显式 `Deps` / `Capability` 结构，把 `conversation_id`、`run_id`、`workspace_path`、`connector_engine` 等依赖按最小粒度拆开。
- [ ] 对已经不应继续暴露的工具，不再新增 runtime bridge，而是直接标记为 legacy island 或等待删除。

### U6-2：把 transport 主路径从 `ToolRegistry::execute()` 拉开

- [ ] 主 transport / query path 禁止再调用 deprecated `ToolRegistry::execute()`。
- [ ] `ToolRegistry` 只负责 catalog / runtime tool registry / legacy island，不再承担“遇到缺口就把 `PluginContext` 填进去”的万能桥接角色。
- [ ] `to_runtime_dispatcher()` 中基于 `PluginContext` 的 request-scoped factory 逐步退场，改为显式 runtime tool provider。

### U6-3：收缩 `PluginContext` 与 legacy island

- [ ] `PluginContext` 明确退回 migration-only 用途；新 runtime 模块不得再导入它。
- [ ] `legacy_adapter` 只服务尚未迁移的 `ToolPlugin`，不得再成为 request-scoped tool 的默认实现。
- [ ] 为 legacy island 建立清单，避免未来“临时先接 `PluginContext`”继续蔓延。

### U6-4：review 约束与测试

- [ ] 增加 review test：`runtime/`、`transport/tauri_commands/`、新的 request-scoped tool 实现不得再 `use crate::plugin::context::PluginContext`。
- [ ] 增加集成测试：request-scoped tool 在没有 `PluginContext` 的情况下仍能从显式 deps 正常执行。
- [ ] 验证 legacy tool 仍能在兼容窗口里工作，但不会重新污染主路径。

## 验收标准

- 生产热路径不再依赖 `PluginContext` 组装 request-scoped tool。
- `ToolRegistry::execute()` 退回纯 legacy / test 岛，不再是默认执行入口。
- 新增 runtime 模块无法轻易把 `PluginContext` 再拉回主路径。
- 此计划只做本地 runtime 边界收口，不扩展任何远程能力。
