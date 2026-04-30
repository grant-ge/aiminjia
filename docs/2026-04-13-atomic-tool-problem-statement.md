# 2026-04-13 Atomic Tool 工具体系 — 问题定义与目标

## 背景

本文档是 `docs/2026-04-12-runtime-gap-problem-statement.md` 中"专项 2：Atomic Tool 工具体系"的细化展开。
定位为专项正式启动前的**问题定义与验收标准**，不是实施计划本身。

---

## 一、问题陈述

### 1.1 工具 schema 存在双轨

`src-tauri/src/llm/tools.rs` 维护了 10 个静态 `ToolDefinition`（用于分析步骤 schema hint），而真实的工具注册在 `src-tauri/src/plugin/registry.rs`（27 个工具），两者**没有共享约束**。  

实际效果：更新 `llm/tools.rs` 不会影响真实执行；更新 `plugin/registry.rs` 不会同步 schema hint。工具名不一致时无编译告警、无测试拦截。

**代码证据：**
- `src-tauri/src/llm/tools.rs:18` — `static ALL_TOOLS: LazyLock<Vec<ToolDefinition>>`
- `src-tauri/src/plugin/registry.rs:99` — `pub async fn get_all_schemas()`
- `src-tauri/src/plugin/builtin/tools/mod.rs:39` — 27 工具注册点

### 1.2 全部 builtin 工具停留在 legacy `ToolPlugin` 层

`plugin/builtin/tools/` 下 27 个工具全部实现 `ToolPlugin`（已标 `#[deprecated(since="0.4.0")]`），真正的 `RuntimeTool` 实现只有 `echo_runtime.rs` 一个示例。迁移路径已存在（`LegacyToolAdapter`），但没有一个生产工具真正走通。

**代码证据：**
- `src-tauri/src/plugin/builtin/tools/mod.rs:7` — `#![allow(deprecated)]`
- `src-tauri/src/plugin/builtin/tools/echo_runtime.rs` — 唯一 RuntimeTool，仅示例

### 1.3 `PluginContext` 仍是 service locator，传遍所有工具

`PluginContext` 包含 18 个字段：`storage`, `file_manager`, `gateway`, `agent_runtime`, `auth_manager`, `connector_engine`, `session_manager`, `tool_registry`, `app_settings`, `event_bus`, `authorized_workspace` 等。

每个工具调用时都拿到全量系统上下文，无法从类型系统判断一个工具实际依赖什么。

**代码证据：**
- `src-tauri/src/plugin/context.rs:74` — 18 字段的 `PluginContext`

### 1.4 `PermissionPipeline` 空转

`AllowAllPermissionPipeline` 是唯一实现，`authorize()` 直接返回 `Ok(())`。
`ToolDefinition.capability_scope: Vec<String>` 字段已存在但始终为空、从不检查。

**代码证据：**
- `src-tauri/src/runtime/tools/permission.rs:19` — `AllowAllPermissionPipeline`
- `src-tauri/src/runtime/tools/definition.rs:6` — `capability_scope: Vec<String>` 未被使用

### 1.5 工具职责混杂，composite 工具伪装成基础工具

以下工具暗含多段动作语义，但和原子工具平级暴露给 LLM：

| 工具 | 隐藏的多阶段行为 |
|---|---|
| `browse_data` | 启动子 agent → 多步浏览器操作 → 写文件 → 返回文件路径 |
| `generate_report` | 渲染 HTML → 写文件 → 按需 PDF/DOCX 转换 |
| `execute_python` | 读取 session 状态 → 执行 → 写 stdout/stderr → 可能写文件 |
| `browse_and_extract` | 导航 + JS + 结构化抽取 = 3步合1 |

LLM 无法从 schema 判断哪些是"一步操作"，哪些是"多步编排"，只能靠 prompt 提示，导致工具选择不稳定。

**代码证据：**
- `src-tauri/src/plugin/builtin/tools/browse_data.rs` — 调用 `handle_browse_data` 委托子代理
- `src-tauri/src/llm/tool_executor/internal_system.rs` — 7 个浏览器工具实现 + 子代理编排混合
- `src-tauri/src/llm/tool_executor/report.rs` — 渲染 + 写文件 + 格式转换 3步

### 1.6 `llm/tool_executor/internal_system.rs` 职责过重

7 个浏览器类工具（`browse_navigate`, `read_page_content`, `page_execute_js`, `browse_and_extract`, `browse_data`, `extract_table_data`, `extract_with_pagination`）全部实现在 `internal_system.rs`，包含浏览器动作 + 子代理编排 + 文件写出，是"工具"和"编排器"的混合体。

### 1.7 新增的 `workspace_tools` 仍走 legacy 路径

`workspace-first` 专项新增的 4 个工具（`list_directory`, `read_workspace_file`, `search_files`, `get_file_info`）已有正确的 workspace 边界校验，但仍然实现 `ToolPlugin` 而非 `RuntimeTool`，是首批迁移的最优候选。

**代码证据：**
- `src-tauri/src/plugin/builtin/tools/workspace_tools.rs:22` — `impl ToolPlugin for ListDirectoryTool`

### 1.8 skill / workflow / prompt 工具名与 registry 无校验绑定

`plugin/builtin/skills/daily_assistant.rs` 中的 `ToolFilter::Only(vec![...])` 引用工具名为字符串字面量，不存在编译期或启动期检查，名字错误静默失效。

---

## 二、目标

1. **单一 schema 真相源**：工具元数据（id、kind、description、capability_scope、json_schema）在 `runtime/tools/catalog.rs` 统一定义，`llm/tools.rs` 降为兼容层或退役。

2. **工具四分类**：每个工具都带 `ToolKind`（Primitive / Power / Composite / Support），从类型上区分"原子能力"和"编排工具"。

3. **原子工具 runtime-first**：第一批至少 11 个原子工具迁到 `RuntimeTool` 体系，经过真实 `ToolDispatcher` 分发。

4. **权限不再空转**：workspace / browser / python 三类能力有显式 `PermissionPipeline` 策略，unauthorized 路径有测试覆盖的拒绝行为。

5. **Composite 工具降级**：`browse_data`, `generate_report`, `data_export`, `slides_gen`, `chart_gen` 在 catalog 中被明确标为 `Composite`，schema description 反映多阶段语义。

6. **skill/workflow/prompt 收口**：引用的工具名必须在 registry 中可解析，建立测试拦截。

---

## 三、验收标准（10 条，硬标准）

| # | 标准 | 检验方式 |
|---|---|---|
| AC-1 | `llm/tools.rs` 不再作为 LLM schema 主来源 | 代码审查：`llm/tools.rs` 降级或退役，registry 通过 catalog 提供 schema |
| AC-2 | 每个注册工具都有 catalog entry（含 `ToolKind`） | `tool_catalog_contract_test` 全通过 |
| AC-3 | 第一批 11 个工具迁成 `RuntimeTool` | `runtime_tool_registry_test` 验证不经 `PluginContext` 能运行 |
| AC-4 | `execute_python` 被显式标为 `Power` | `tool_catalog_contract_test` 断言 kind = Power |
| AC-5 | `browse_data` 被显式标为 `Composite`，schema 描述多阶段 | `composite_tool_delegation_test` 验证 |
| AC-6 | 未授权 workspace 时文件类工具返回结构化 `PermissionDenied` 错误 | `tool_permission_pipeline_test` |
| AC-7 | 无 browser connector 时浏览器类工具返回结构化错误 | `tool_permission_pipeline_test` |
| AC-8 | 工具调用全链路统一经 `ToolDispatcher`，不再有 `ToolRegistry::execute()` 旁路路径 | `tool_dispatch_outcome_test` |
| AC-9 | skill/workflow 的 `ToolFilter::Only(...)` 中的工具名都能在 registry 解析 | `skill_tool_contract_test` |
| AC-10 | 全部 `review_*` 回归测试和新增 TDD 套件通过 | `cargo test` 无失败 |

---

## 四、工具分类矩阵（当前全量）

### Primitive — 原子工具（首批迁移目标）

| 工具名 | 当前路径 | 能力域 |
|---|---|---|
| `list_directory` | `workspace_tools.rs` | workspace:read |
| `read_workspace_file` | `workspace_tools.rs` | workspace:read |
| `search_files` | `workspace_tools.rs` | workspace:read |
| `get_file_info` | `workspace_tools.rs` | workspace:read |
| `web_search` | `web_search.rs` | network |
| `browse_navigate` | `browse_navigate.rs` | browser |
| `read_page_content` | `read_page_content.rs` | browser |
| `page_execute_js` | `page_execute_js.rs` | browser |
| `extract_table_data` | `extract_table_data.rs` | browser |
| `extract_with_pagination` | `extract_with_pagination.rs` | browser |
| `load_file` | `file_load.rs` | workspace:read |

### Power — 强能力单域执行器

| 工具名 | 当前路径 | 说明 |
|---|---|---|
| `execute_python` | `python_exec.rs` | 单段代码执行，但有 session 状态和文件副作用 |

### Composite — 编排工具（显式降级目标）

| 工具名 | 当前路径 | 内部依赖 |
|---|---|---|
| `browse_data` | `browse_data.rs` | 子代理 + 多步 browser primitive |
| `browse_and_extract` | `browse_and_extract.rs` | navigate + read + extract 三步 |
| `generate_report` | `report_gen.rs` | 渲染 + 写文件 + 格式转换 |
| `generate_chart` | `chart_gen.rs` | 数据计算 + 渲染 + 写文件 |
| `export_data` | `data_export.rs` | 数据转换 + 写文件 |
| `generate_slides` | `slides_gen.rs` | 多页渲染 + 写文件 |

### Support — 辅助工具（状态/记忆/进度）

| 工具名 | 说明 |
|---|---|
| `plan_update` | 计划状态更新 |
| `progress_update` | 步骤进度 |
| `save_analysis_note` | 中间分析记录 |
| `save_memory`, `search_memory`, `core_memory`, `distill_memory` | 记忆管理 |

---

## 五、不做什么（边界约束）

- **不一开始就全量重写工具实现**：legacy 工具通过 `LegacyToolAdapter` 继续工作，迁移是逐步的
- **不删除 `execute_python`**：它是核心 Power 工具，只是需要明确分类和权限策略
- **不修改前端**：本专项只收口后端工具系统
- **不修改 LLM provider 层**：本专项不涉及 `llm/gateway.rs`、`llm/providers/`
- **不和 workspace-first、prompt slimming、skill import 专项同步进行**：独立实施，接口对接即可

---

## 六、与其他专项的关系

| 依赖方向 | 说明 |
|---|---|
| Workspace-First → Atomic Tool | workspace tools 迁到 RuntimeTool 后，workspace-first 的权限链路可直接用 capability scope 而非 PluginContext |
| Atomic Tool → Prompt Slimming | 工具 kind/scope 显式后，prompt 中"先 load_file 再 execute_python"这类协议说明可从 prompt 移除 |
| Atomic Tool → Skill Import | skill manifest 中 `allowed_tools` 校验依赖 catalog 单一真相源 |

---

## 附录：参考文档

- `docs/2026-04-12-runtime-gap-problem-statement.md` — 母问题定义
- `docs/architecture-blueprint.md` — 架构蓝图
- `docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md` — 本专项实施计划
