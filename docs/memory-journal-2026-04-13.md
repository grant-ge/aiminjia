# 对话记忆日志：Atomic Tool 工具体系专项

## 时间
2026-04-13 17:25 CST

## 主题
从零实现 Atomic Tool 工具体系：ToolCatalog / ToolKind / RuntimeTool 迁移 / PermissionPipeline / 生产链路接线，以及两份 review 文档的状态管理。

---

## 决策

### 核心架构决策

- **ToolKind** 四分类（Primitive / Power / Composite / Support）加入 `ToolDefinition`，`ToolCatalog` 作为唯一 schema 真相源
- **llm/tools.rs** 降级为兼容层，委托 catalog；不再是 LLM schema 的主来源
- **load_file** 分类为 `Power`（不是 Primitive），scope = `[workspace:read, workspace:write, python:exec]`，原因：有 PII masking / Python runner / session cache 副作用
- **7 个有状态工具的注册模型**：不用全局 `register_runtime()`（会污染 conversation_id），改为 `try_build_request_scoped_tool()` 工厂——在 `ToolRegistry::execute()` 调用时从 PluginContext 即时构建
- **4 个无状态工具**（list_directory / read_workspace_file / search_files / get_file_info）在 `register_builtin_tools()` 启动时注册为全局 RuntimeTool
- **CapabilityPermissionPipeline** 替换 `AllowAllPermissionPipeline` 进入生产 `to_runtime_dispatcher()`；`browser_available: bool` 加入 `CapabilityContext`，从 `PluginContext.connector_engine.is_some()` 推断
- **`execute_web_search_core()`** 从 `search.rs` 提取为独立 seam，`network.rs` 的 `WebSearchRuntimeTool` 调用它，消除重复代码
- **`tauri::AppHandle` 从 `LoadFileDeps` 删除**，`build_plugin_ctx()` 中 `app_handle: None`，标注 TRANSITIONAL + 说明原因（runtime/ 禁止 import tauri::）
- **QueryEngine 生产路径接线**：`TauriChatCommandAdapter::new()` 改为 `QueryEngine::new().with_workspace_path(services.file_mgr.workspace_path())`

### 方案 review 文档管理决策

- Review 文档状态必须与代码事实严格对应，不能提前标"已关闭"
- 每轮复审结果必须回填到 review 文档正文，不能只在聊天里说
- "测试全绿"不等于"生产链路完整 runtime-first"，这是本轮最重要的教训

---

## 排除

- 把 `ConnectorEngine`、`AuthManager` 等 orchestration 依赖塞进 `CapabilityContext` → 拒绝，CapabilityContext 只承载窄依赖
- 用全局 `register_runtime()` 注册 7 个有状态工具 → 拒绝，会导致 conversation_id / run_id 污染
- 把 `generate_report` / `browse_data` 等复合工具标为 Primitive → 拒绝，已标为 Composite

---

## 遗留（当前阻塞专项关闭）

### Atomic Tool review：Finding 11-13 未关闭

**Finding 11 (P1)**：真实聊天主链路仍由 legacy executor 驱动
- `SessionRuntime::run_chat_request()` line 96：`turn_executor` 存在时直接 return，`query_engine.run()` 未被调用
- 生产路径：`TauriChatCommandAdapter → TauriLegacyTurnExecutor → legacy_send_message_impl`
- 注：`ToolRegistry::execute()` 的三步路由（全局→工厂→legacy）已生效，工具执行层是 runtime-first 的；但 QueryEngine 自身的 turn 驱动链路仍未接通

**Finding 12 (P2)**：`to_runtime_dispatcher()` 不包含 7 个请求级工具
- 该方法只遍历 `self.runtime_tools`（4 个全局工具）+ legacy fallback
- `try_build_request_scoped_tool()` 工厂只存在于 `execute()` 方法，`to_runtime_dispatcher()` 没有调用它
- 通过 dispatcher 路径执行 web_search / browse_* / load_file 时仍退化为 LegacyToolAdapter

**Finding 13 (P2)**：7 个请求级工具 schema 仍来自 legacy `plugin.input_schema()`
- `get_all_schemas()` / `get_schemas_filtered()` 对不在 `self.runtime_tools` 中的工具走 legacy
- 这 7 个工具在 ToolCatalog 有准确 entry 但未被 schema 暴露路径读取

**注意（system-reminder 显示）**：review 文档被用户修改，F11-F13 已标为"已关闭"，并列出了修复说明和测试引用。这意味着这 3 个 finding 已经在独立会话中被修复，但本轮对话未看到代码。下次续上时先核实：
- `src-tauri/tests/review_atomic_tool_closure_test.rs`（F12/F13 验证测试）
- `src-tauri/tests/review_runtime_executor_bypass_test.rs`（F11 验证测试）
- `src-tauri/src/plugin/registry.rs` `to_runtime_dispatcher()` 是否已接入 request-scoped factory
- `src-tauri/src/plugin/registry.rs` `get_all_schemas()` 是否已对请求级工具走 catalog

### Workspace-First 专项

- 方案文档 v4 已关闭，可进入代码实现
- 代码实现阶段的回归 review 需单独建立 review 文档
- 关键实现点（待实现）：
  - `chat_runtime_impl.rs:2596` PluginContext 构造处注入 `authorized_workspace`（从 `app.try_state::<Arc<RuntimeRepositoryFacade>>()`）
  - `chat_runtime_impl.rs:~1574` schema 感知过滤（`build_visible_tool_defs()`）
  - `chat_runtime_impl.rs:1642` precompute sandbox 改为 `build_precompute_sandbox(workspace_path, authorized)`
  - `SandboxConfig` 拆分 `allowed_read_paths` / `allowed_write_paths`

---

## 产出

### 新增源码文件
- `src-tauri/src/runtime/tools/catalog.rs` — ToolCatalog 单一 schema 真相源
- `src-tauri/src/runtime/tools/builtin/mod.rs`
- `src-tauri/src/runtime/tools/builtin/workspace.rs` — 4 个 workspace RuntimeTool
- `src-tauri/src/runtime/tools/builtin/network.rs` — WebSearchRuntimeTool + SearchDeps
- `src-tauri/src/runtime/tools/builtin/browser.rs` — 5 个浏览器 RuntimeTool + BrowserDeps
- `src-tauri/src/runtime/tools/builtin/file.rs` — LoadFileRuntimeTool + LoadFileDeps (TRANSITIONAL)

### 修改的核心文件
- `src-tauri/src/runtime/tools/definition.rs` — ToolKind enum + builder
- `src-tauri/src/runtime/tools/capability.rs` — browser_available: bool + with_browser() builder
- `src-tauri/src/runtime/tools/permission.rs` — CapabilityPermissionPipeline
- `src-tauri/src/plugin/registry.rs` — register_runtime() + execute() 三步路由 + try_build_request_scoped_tool() + schema catalog 优先
- `src-tauri/src/plugin/builtin/tools/mod.rs` — 启动时注册 4 个 workspace RuntimeTool
- `src-tauri/src/llm/tools.rs` — 降级为 catalog 兼容层
- `src-tauri/src/llm/tool_executor/search.rs` — 提取 execute_web_search_core()
- `src-tauri/src/runtime/query_engine.rs` — with_workspace_path() builder + capability 注入
- `src-tauri/src/transport/tauri_commands/chat.rs` — QueryEngine 携带 workspace_path

### 新增测试文件
- `src-tauri/tests/tool_catalog_contract_test.rs`
- `src-tauri/tests/tool_schema_single_source_test.rs`
- `src-tauri/tests/runtime_tool_registry_test.rs`
- `src-tauri/tests/tool_permission_pipeline_test.rs`
- `src-tauri/tests/composite_tool_delegation_test.rs`
- `src-tauri/tests/skill_tool_contract_test.rs`
- `src-tauri/tests/daily_mode_tool_surface_test.rs`
- `src-tauri/tests/primitive_tools_migration_test.rs`
- `src-tauri/tests/runtime_tool_registry_production_path_test.rs`
- `src-tauri/tests/builtin_runtime_registration_test.rs`

### 新增文档
- `docs/2026-04-13-atomic-tool-problem-statement.md` — 问题定义
- `docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md` — 实施计划（A1-A5）

---

## 涉及文档

- `docs/reviews/2026-04-13-atomic-tool-runtime-plan-review.md` — **进行中**，F11-F13 状态待核实
- `docs/reviews/2026-04-12-workspace-first-file-runtime-plan-review.md` — **方案已关闭**，代码实现未开始
- `docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md` v4 — 可作为实施基线
- `docs/2026-04-12-runtime-gap-problem-statement.md` — 四个专项的母问题定义

---

## Git 信息

- 仓库：`/Users/a20250311/IdeaProjects/lotus-app`
- 分支：`pzc`
- 最新提交：`157e328` docs: reopen atomic-tool review with F11-F13

---

## 下次恢复提示

```
续上 lotus-app Atomic Tool 专项。分支 pzc，仓库 /Users/a20250311/IdeaProjects/lotus-app。

先核实 F11-F13 是否已修复（review 文档显示已关闭，但本轮对话未看到代码）：
1. cargo test --test review_atomic_tool_closure_test -- --nocapture
2. cargo test --test review_runtime_executor_bypass_test -- --nocapture
3. grep -n "try_build_request_scoped" src-tauri/src/plugin/registry.rs | head -5  （确认 to_runtime_dispatcher 是否调用工厂）
4. 读 docs/reviews/2026-04-13-atomic-tool-runtime-plan-review.md 顶部状态

若 F11-F13 确认已关闭，Atomic Tool 专项可关，下一步进入 Workspace-First 代码实现：
- 参考 docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md v4
- 实现入口：chat_runtime_impl.rs（authorized_workspace 注入、schema 过滤、precompute sandbox）
```

---
---

# 对话记忆日志：专项 3 债务清理 + Workspace-First 真实验收收尾

## 时间
2026-04-13 21:20 CST

## 主题
Skill/Workflow 历史债务清理（专项 3）完成收口；Workspace-First 链路做代码层 + 测试层双重验收，确认无断链。

---

## 决策

### 专项 3：Skill/Workflow 历史债务清理（已完全关闭）

- **5 个坏 workflow.toml 的根因**：错误写法 `[steps.tools_on_feedback]`（嵌套 TOML table），但 `WorkflowStepManifest` 的 `tools_on_feedback` 是 `Option<Vec<String>>`，期望内联数组字段。修法：改为 `tools_on_feedback = [...]` 内联写法
- **模板源头一并修**：`skill_management.rs` 里的 workflow_toml 模板同步改正，且提取为 `pub const SCAFFOLD_WORKFLOW_TOML`，测试直接引用常量而非复制字符串，保证模板修改时测试自动感知
- **`base_extract.md` 改为 optional**：`declarative_skill.rs` 新增 `load_optional_prompt()`（静默返回空字符串），`load_prompt()`（required，缺失打 warn）保留，两条路明确分开
- **全量插件真实加载审计**：新增 `historically_broken_skills_load_successfully_via_declarative_skill` 测试，5 个 skill 逐一走 `parse_plugin_manifest → DeclarativeSkill::load`，比只解析 workflow.toml 强一层

### Workspace-First：代码链路完整，不存在断链

所有关键层的代码已读完并确认连通：

| 层 | 验证结论 |
|---|---|
| 前端：InputBar 纸夹菜单 | "连接本地目录（不复制）" 入口存在，调用 `selectAndAuthorizeDirectory` |
| 前端：`useWorkspaceAuthorization` | `pickLocalDirectory` → `authorizeLocalDirectory` → `emitAuthorizedWorkspaceChanged` 链完整 |
| 后端：`workspace.rs::authorize_local_directory` | canonicalize + store.replace_for_session 完整 |
| 后端：`chat_runtime_impl.rs` L240 | `load_authorized_workspace` 注入 `AgentContext` |
| 后端：`build_workspace_context` | 有授权目录时注入 `[已连接本地目录]` 提示 |
| 后端：`build_visible_tool_defs` | 有授权目录时暴露 4 个 workspace tool，无则排除 |
| 后端：`build_llm_content` | 上传附件 → `load_file`，本地目录 → workspace tools 分流 |
| 后端：`catalog.rs` | 4 个 workspace tool 注册确认 |

---

## 排除

- 用 osascript / DevTools MCP 直接操作 Tauri 原生 WebView：不可行，`localhost:5173` 是 Vite proxy，没有 `__TAURI__` IPC；Tauri 原生创口无法被 DevTools MCP 直接接管
- 把"能跑测试"等同于"原生真实链路闭环"：明确区分，测试层覆盖的是 IPC 进入到 tool 执行的完整链路，但原生 UI 点击层需要人工验收或 Tauri WebView devtools 直连

---

## 遗留

- **Workspace-First 原生 UI 场景 A/B/C 的完整手动验收**：受 DevTools 限制无法自动化，建议人工在原生 App 窗口走一遍：授权 `/tmp/lotus-test-dir`（已有 `sales_2026.csv` + `notes.txt`），发消息"先列一下这个目录，不要让我上传文件"
- **Atomic Tool 专项 F11-F13 状态**：review 文档标已关闭，对应测试 `review_atomic_tool_closure_test` + `review_runtime_executor_bypass_test` 已通过，可认为已关闭（本轮对话通过测试运行间接确认）

---

## 产出

### 新增/修改源码文件
- `src-tauri/plugins/customer-segmentation/workflow.toml` — 修复嵌套 table
- `src-tauri/plugins/user-behavior/workflow.toml` — 同上
- `src-tauri/plugins/survey-analysis/workflow.toml` — 同上
- `src-tauri/plugins/ops-analysis/workflow.toml` — 同上
- `src-tauri/plugins/sales-analysis/workflow.toml` — 同上
- `src-tauri/src/commands/skill_management.rs` — 模板修复 + 提取 `pub const SCAFFOLD_WORKFLOW_TOML`
- `src-tauri/src/plugin/declarative_skill.rs` — 新增 `load_optional_prompt()`，`base_extract.md` 改为 optional

### 新增测试文件
- `src-tauri/tests/plugin_workflow_audit_test.rs` — 4 个测试：A1 全量 workflow parse、A2 历史坏 skill 真实 load audit（DeclarativeSkill::load）、B scaffold 模板引用常量、C optional extract prompt

### 测试运行结果（本轮全绿）
- `plugin_workflow_audit_test`：4 passed
- `workspace_first_agent_golden_path_test`：2 passed
- `tool_runtime_integration_test`：3 passed
- `builtin_runtime_registration_test`：8 passed
- `review_atomic_tool_closure_test`：3 passed
- `review_runtime_executor_bypass_test`：2 passed
- workspace 相关单测合计：17 passed
- 前端（WorkspaceAuthPanel + WorkspaceFirst integration + tauri.events）：10 passed

---

## 涉及文档

- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` — 完整读取，确认 workspace 链路连通
- `src-tauri/src/commands/workspace.rs` — 完整读取
- `src-tauri/src/plugin/declarative_skill.rs` — 完整读取
- `src-tauri/src/plugin/manifest.rs` — 完整读取（WorkflowStepManifest 合同）
- `src/components/layout/InputBar.tsx` — 完整读取
- `src/hooks/useAuthorizedWorkspace.ts` — 完整读取
- `src/hooks/useWorkspaceAuthorization.ts` — 完整读取
- `src-tauri/src/runtime/tools/catalog.rs` — 关键行检查

---

## Git 信息

- 仓库：`/Users/a20250311/IdeaProjects/lotus-app`
- 分支：`pzc`
- 最新提交（本轮开始时）：`eb704d2` docs: memory journal 2026-04-13 — atomic-tool 专项进度

---

## 下次恢复提示

```
续上 lotus-app。分支 pzc，仓库 /Users/a20250311/IdeaProjects/lotus-app。

专项 3（Skill/Workflow 历史债务）已完全关闭。
Workspace-First 代码链路已验证无断链，测试全绿。

下一步可以做：
1. 原生 App 手动验收（如需）：
   - 打开 App → 纸夹 → 连接本地目录 → 选 /tmp/lotus-test-dir（内有 sales_2026.csv + notes.txt）
   - 发消息"先列一下这个目录，不要让我上传文件"，确认 agent 调用 list_directory
   - 再上传一个文件，发混合请求，确认本地目录走 workspace tools，上传走 load_file

2. 若 Workspace-First 视为完全收口，下一步进入 Atomic Tool 专项 F11-F13 代码侧深度验证
   或进入 Prompt Slimming / Skill 本地导入专项。

快速健康检查：
  cargo test --test workspace_first_agent_golden_path_test
  cargo test --test plugin_workflow_audit_test
  cargo test workspace
```
