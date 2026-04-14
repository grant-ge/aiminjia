# 2026-04-13 Atomic Tool Runtime Review

状态：**已关闭（2026-04-13）**  
评审对象：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md` 以及其后续代码实现  
对照代码：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/tools/`、`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/`、`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/`

## 当前总状态

### 第一阶段：方案评审 findings（Finding 1-6）

- 状态：已关闭

### 第二阶段：代码落地复审 findings（Finding 7-10）

- Finding 7：✅ 已关闭
- Finding 8：✅ 已关闭
- Finding 9：✅ 已关闭
- Finding 10：✅ 已关闭（`ab8b44a`）

### 第三阶段：深度链路复审 findings（Finding 11-13）

- Finding 11：✅ 已关闭（2026-04-13）— executor-backed 聊天路径已先经过 QueryEngine preflight，再委托 legacy executor
- Finding 12：✅ 已关闭（2026-04-13）— `to_runtime_dispatcher()` 已注册 7 个请求级工具，dispatcher 路径可直接执行它们
- Finding 13：✅ 已关闭（2026-04-13）— 7 个请求级工具的 schema 已从 `ToolCatalog` 暴露，不再依赖 legacy `plugin.input_schema()`

## 关闭条件

本专项关闭条件已满足：

1. Finding 11：executor-backed 聊天路径已接入 QueryEngine preflight
2. Finding 12：`to_runtime_dispatcher()` 已覆盖请求级工具
3. Finding 13：`get_all_schemas()` / `get_schemas_filtered()` 已对请求级工具走 catalog schema

---

## Findings

### Finding 1

- 标题：`[P1] A3 的 browser capability 设计会把 5 个浏览器 primitive 默认全部拒绝`
- 严重级别：P1
- 影响范围：`browse_navigate`、`read_page_content`、`page_execute_js`、`extract_table_data`、`extract_with_pagination`，以及后续任何声明 `browser` capability 的 runtime tool
- 真实使用路径：`LLM tool call -> ToolDispatcher -> CapabilityPermissionPipeline -> has_browser_capability() -> browser primitive`
- 问题描述：
  - 方案在 `atomic-tool-runtime-plan.md:393-447` 把 5 个浏览器工具都定义为 `capability_scope = ["browser"]`。
  - 同时又在 `atomic-tool-runtime-plan.md:1195-1225` 规定：带 `browser` scope 的工具必须通过 `ctx.capability.has_browser_capability()`。
  - 但 `atomic-tool-runtime-plan.md:1252-1264` 又把 `has_browser_capability()` 明确写成固定返回 `false`。
  - 当前实现里这件事也是真的：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/tools/capability.rs:64-70` 现在就是固定 `false`。
- 为什么这是问题：
  - 这不是“先保守一点”，而是**按设计把浏览器 primitive 永久拒绝**。
  - 文档后面又把“剩余 7 个 primitive 和 workspace tools 完全对称”写成迁移前提，但浏览器工具在 permission 层与 workspace tools 根本不对称。
  - 如果不先定义浏览器能力的真实注入方式，A3 一落地，浏览器工具即使迁成 `RuntimeTool` 也只能在权限层报错，根本进不到执行器。
- 现有/计划测试为什么不够：
  - 当前计划里的 permission TDD 只会证明“`browser` scope 在 `has_browser_capability() == false` 时会失败”，却**没有证明浏览器连接存在时会成功**。
  - 也没有测试驱动真实的 connector 注入路径。
- 修复建议：
  - 方案层先明确浏览器能力的真实 contract，至少二选一：
    1. 在 `CapabilityContext` 中加入一个**窄的 browser capability 句柄**，只表达“是否有可用 connector / 如何取 connector ref”，不要塞回完整 `PluginContext`。
    2. 不把浏览器 primitive 直接走 capability 判定，而是像 legacy bridge 一样，通过构造注入的 `BrowserDeps` 在 runtime wrapper 内部完成能力校验。
  - 同时补两类 TDD：
    - `browser primitive denied without connector`
    - `browser primitive allowed with injected connector capability`
- 相关文件定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:393`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:1195`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:1252`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/tools/capability.rs:64`

### Finding 2

- 标题：`[P1] 剩余 7 个工具没有可执行的依赖注入方案，不能按 workspace 模板对称迁移`
- 严重级别：P1
- 影响范围：`web_search`、5 个浏览器 primitive、`load_file`
- 真实使用路径：
  - `web_search -> handle_web_search()`
  - `browse_* / page_execute_js / extract_* -> handle_browse_*() / handle_page_execute_js()`
  - `load_file -> handle_load_file()`
- 问题描述：
  - 文档在 `atomic-tool-runtime-plan.md:1555-1557` 写“剩余 7 个工具的 RuntimeTool 迁移逻辑与 A2.1 workspace 工具完全对称，代码模式一致”。
  - 但真实代码并不是这样：
    - `web_search` 依赖 `use_cloud`、`auth_manager`、`bocha_api_key`、`tavily_api_key`：见 `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/search.rs:38-103`
    - 浏览器 primitive 依赖 `connector_engine`：见 `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/internal_system.rs:16-207`
    - `load_file` 依赖 `storage`、`file_manager`、`app_handle`、Python runner、解析/PII/memory 逻辑：见 `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/file_load.rs:453-680`
  - 与此同时，`CapabilityContext` 的设计恰恰是**故意不放**这些服务：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/tools/capability.rs:3-14`
  - `PluginContext` 的文档也明确说了不要把 `AuthManager`、`AgentRuntime` 之类继续塞进新 context：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/context.rs:24-29`
- 为什么这是问题：
  - 这说明剩余 7 个工具不是“复制 workspace.rs 模板”就能迁完。
  - 如果文档不先定义迁移 seam，Claude 很容易走两条错误路：
    1. 把 `CapabilityContext` 扩回第二个 `PluginContext`
    2. 名义上实现 `RuntimeTool`，实际上内部还是偷构完整 `PluginContext`
- 现有/计划测试为什么不够：
  - 当前计划的 A2 TDD 只覆盖了 4 个 workspace runtime tools 的纯文件读场景：`atomic-tool-runtime-plan.md:712-791`
  - 没有任何失败测试去证明：
    - `web_search` 的 cloud/bocha/bing/tavily 依赖怎么注入
    - 浏览器 primitive 怎么拿 connector
    - `load_file` 怎么保留 loaded-scope / masking / parse side effects
- 修复建议：
  - 方案必须显式改成“**构造注入 + RuntimeTool wrapper**”，至少补这 3 组 deps contract：
    - `SearchDeps`：`use_cloud`、`auth_manager`、`bocha_api_key`、`tavily_api_key`
    - `BrowserDeps`：`connector_engine`
    - `LoadFileDeps`：`storage`、`file_manager`、`session_manager`、`app_handle`（必要时再拆更窄）
  - 并明确：
    - 这些 deps 在哪里构造
    - 谁拥有生命周期
    - runtime wrapper 如何调用 legacy handler / helper
    - 哪些依赖绝对不能进入 `CapabilityContext`
- 相关文件定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:692`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:1555`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/search.rs:17`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/internal_system.rs:16`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/file_load.rs:453`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/tools/capability.rs:3`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/context.rs:24`

### Finding 3

- 标题：`[P1] 新 RuntimeTool 方案还没有接入真实生产注册链路`
- 严重级别：P1
- 影响范围：所有“已迁 RuntimeTool”的可达性，尤其是后续剩余 7 个工具
- 真实使用路径：`register_builtin_tools() -> ToolRegistry -> to_runtime_dispatcher() / execute() -> ToolPlugin or RuntimeTool`
- 问题描述：
  - 文档 A2 只新增 `src-tauri/src/runtime/tools/builtin/workspace.rs` 和对应单测：见 `atomic-tool-runtime-plan.md:696-1072`
  - 但没有同时规定**真实生产注册链路**怎么把这些 `RuntimeTool` 暴露给模型和 dispatcher。
  - 当前真实代码里：
    - builtin 注册仍然只注册 `ToolPlugin`：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/builtin/tools/mod.rs:38-72`
    - `ToolRegistry` 只接收 `ToolPlugin`：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/registry.rs:66-88`
    - 真实 dispatcher 构造时也是把 `ToolPlugin` 包成 `LegacyToolAdapter`：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/registry.rs:183-196`
- 为什么这是问题：
  - 如果方案不补这条接线，`runtime/tools/builtin/*.rs` 很容易只停留在“存在于代码库和单元测试中”，但真实聊天主链路根本没在用。
  - 这会制造最危险的一类假完成：catalog 是新的、测试是新的、生产调度仍是旧的。
- 现有/计划测试为什么不够：
  - 计划中的 `runtime_tool_registry_test` 实际并没有走 `ToolRegistry`，只是直接 new `ListDirectoryRuntimeTool` 后调用 `RuntimeTool::execute(...)`：见 `atomic-tool-runtime-plan.md:712-791`
  - 它证明不了：
    - builtin 注册是否真的换到 runtime path
    - `ToolRegistry::to_runtime_dispatcher()` 是否会拿到新工具
    - 真实 skill/tool filtering 后 LLM 能否看到这些工具
- 修复建议：
  - 方案必须明确下面两者之一：
    1. `ToolRegistry` 增加 `register_runtime()` / `register_runtime_builtin()`，真正成为运行时注册中心。
    2. builtin 工具模块在注册时直接构造 runtime wrapper，并让 `ToolRegistry` 同时持有 schema 来源与 runtime executor。
  - 必补的 TDD 不是 direct-call test，而是生产链路测试：
    - `register_builtin_tools_exposes_runtime_tool_to_registry`
    - `tool_registry_to_runtime_dispatcher_can_dispatch_runtime_tool`
    - `get_schemas_filtered_returns_runtime_tool_schema_from_single_source`
- 相关文件定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:696`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:712`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/builtin/tools/mod.rs:38`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/registry.rs:66`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/registry.rs:183`

### Finding 4

- 标题：`[P1] “不修改 llm/tool_executor” 与剩余 7 个工具迁移目标直接冲突`
- 严重级别：P1
- 影响范围：剩余 7 个 primitive 的实际落地方式
- 真实使用路径：`RuntimeTool wrapper -> 复用旧 handler / helper -> 保持行为一致`
- 问题描述：
  - 文档在 `atomic-tool-runtime-plan.md:1570-1577` 明确写了：
    - 不重写 `browse_data`
    - **不修改 `llm/tool_executor/` 中的执行器实现**
    - 不在 `CapabilityContext` 中加入 `LlmGateway`、`AuthManager`
  - 但真实剩余 7 个工具的难点，恰恰就集中在 `llm/tool_executor/search.rs`、`llm/tool_executor/internal_system.rs`、`llm/tool_executor/file_load.rs` 这几个旧 handler 的依赖与 side effects。
- 为什么这是问题：
  - 如果 `tool_executor` 完全不能动，那就只剩两条路：
    1. 强行继续用完整 `PluginContext`
    2. 在外层拼装一个“伪新接口，内核还是旧 service locator”的 wrapper
  - 这会让“迁移到 RuntimeTool”只剩 trait 形式变化，没有真正完成依赖边界收口。
  - 这里并不是要求大改 handler，而是文档至少要允许“**抽 helper / 加 wrapper seam / 拆 deps struct**”这一级别的修改。
- 现有/计划测试为什么不够：
  - 当前测试计划没有任何一条去约束“保留旧行为但拆出新 seam”。
  - 所以实现者会倾向于用最省事的方式绕过去，导致边界再次塌回 legacy。
- 修复建议：
  - 把 `Not Doing` 改细，而不是一刀切写“不能动 `llm/tool_executor/`”。
  - 建议改成：
    - 不重写业务逻辑
    - 允许抽出 pure helper / deps adapter / wrapper constructor
    - 不允许再新增对完整 `PluginContext` 的直接新依赖
- 相关文件定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:1570`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/search.rs:17`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/internal_system.rs:16`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/file_load.rs:453`

### Finding 5

- 标题：`[P2] load_file 被建模成 Primitive 过于乐观，权限和心智模型都会失真`
- 严重级别：P2
- 影响范围：`load_file` 的 ToolKind、权限设计、skill 暴露面、后续 TDD
- 真实使用路径：`load_file -> uploaded file ownership check -> parse -> masking -> cache/memory -> execute_python session preload`
- 问题描述：
  - 方案在 `atomic-tool-runtime-plan.md:362-375` 把 `load_file` 定义为 `ToolKind::Primitive`，只给了 `workspace:read` scope。
  - 但真实实现不是一个“低副作用单步读取文件”的 primitive：
    - 它检查 conversation/run 归属：`file_load.rs:497-506`
    - 解析文件并调用 Python runner：`file_load.rs:554-568`
    - 执行 PII masking，并把映射写入存储：`file_load.rs:595-680`
    - 影响后续 `execute_python` 的 session 变量注入语义：`file_load.rs:447-452`
- 为什么这是问题：
  - 如果它在 catalog 和 skill surface 上被当成 primitive，就会误导后续权限模型与提示词：
    - 它不是单纯 `workspace:read`
    - 它具有 session state / memory side effects
    - 它更接近 `Power`，至少也应明确标成“primitive at surface, power in runtime behavior”的特例
- 现有/计划测试为什么不够：
  - 当前计划只会验证它在 catalog 中存在、名字能解析，证明不了它的真实副作用边界是否被正确建模。
- 修复建议：
  - 在方案里先明确 `load_file` 的分类决策：
    - 如果继续保留 `Primitive`，必须加注释说明这是“对 LLM 暴露面上的 primitive”，不是执行语义上的 primitive。
    - 更稳妥的做法是把它调到 `Power`，并相应补权限与 tool-surface 设计说明。
  - 同时增加 TDD，验证 `load_file` 的 loaded-scope、masking 和 cache 语义不因迁移而回归。
- 相关文件定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:362`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/file_load.rs:447`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/context.rs:104`

### Finding 6

- 标题：`[P2] 自查结论把“模板完成”写成了“11 个工具迁移目标基本落地”，容易造成错误关卡`
- 严重级别：P2
- 影响范围：计划验收、Claude 执行节奏、后续 review gating
- 真实使用路径：`Spec Coverage / 自查 -> 判定专项是否可关闭 -> 进入代码实施`
- 问题描述：
  - 文档在 `atomic-tool-runtime-plan.md:1547-1557` 一边写“第一批 11 个工具迁 RuntimeTool”，一边又承认这版 plan 实际只迁了 4 个 workspace tools，剩余 7 个后置。
  - 同时还把“剩余 7 个工具与 A2.1 完全对称”写成简化结论，但这与前面实际依赖形态并不相符。
- 为什么这是问题：
  - 这会让实施者误以为本专项只剩机械复制工作，从而跳过架构修订。
  - 也会让后续 review 文档很难作为真正 gating 文档使用。
- 现有/计划测试为什么不够：
  - 自查表本身不是测试；当前表述会掩盖真实未完成项。
- 修复建议：
  - 把专项拆成两层结论：
    - 已完成：catalog / kind / workspace runtime template / permission skeleton
    - 未完成：7 个 legacy-heavy primitive 的 deps seam、注册接线、行为回归测试
  - 明确文档状态为“未闭合”，修完 Findings 1-4 后再进入代码实施。
- 相关文件定位：
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:1547`
  - `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-atomic-tool-runtime-plan.md:1555`

---

## 建议补充的 TDD（在修方案时一起写进计划）

下面这些测试建议不是“实现后再补”，而是建议直接写进方案，作为 Claude 修技术方案时的明确通过条件：

- `browser_runtime_tool_denied_without_connector_test`
  - 证明浏览器 primitive 在无 connector 时被拒绝。
- `browser_runtime_tool_allowed_with_connector_test`
  - 证明存在 connector capability / injected browser deps 时能通过 permission 并进入执行器。
- `web_search_runtime_wrapper_contract_test`
  - 证明 runtime wrapper 在不扩大 `CapabilityContext` 的前提下，仍能拿到 cloud/auth/api-key 依赖并调用旧 handler。
- `load_file_runtime_wrapper_preserves_loaded_scope_test`
  - 证明 run/session scope、cache key、masking side effects 没被迁移破坏。
- `runtime_tool_registry_production_path_test`
  - 真实驱动 `register_builtin_tools()` -> `ToolRegistry::to_runtime_dispatcher()`，证明新 RuntimeTool 真的进入生产调度链路。
- `catalog_registry_single_source_parity_test`
  - 证明 catalog、builtin 注册、dispatcher 可执行集合三者一致，不再出现“catalog 有、生产不可达”。

---

## 结论性建议

这份方案现在**适合先修文档，不适合直接开写剩余 7 个工具代码**。

建议 Claude 下一轮先做这 4 件事：

1. 修正 browser capability contract，不要让浏览器 primitive 默认永久失败。
2. 把“剩余 7 个工具”的迁移方案改成明确的 constructor injection / wrapper seam，而不是“和 workspace 对称”。
3. 补上真实生产注册链路设计，说明 RuntimeTool 如何进入 `ToolRegistry` / dispatcher。
4. 收紧 `Not Doing`，允许必要的 `tool_executor` seam 拆分，同时保持不重写业务逻辑。

完成后，再进入代码实现阶段会稳很多。

---

## 第二轮：代码落地复审（2026-04-13）

### 复审范围

- 启动注册链路：`register_builtin_tools()` / `lib.rs`
- 真实执行链路：`ToolRegistry::execute()`、`chat_runtime_impl.rs`、`llm/sub_agent.rs`
- 生产 dispatcher：`CapabilityPermissionPipeline` 与 capability 注入
- 代码级测试：`builtin_runtime_registration_test.rs` 与相关 runtime / registry 测试

### 代码复审结论

Atomic Tool 专项全部 10 个 findings 已关闭，生产链路已完全切换到 runtime-first。

### Finding 7

- 标题：`[P1] 仍只有 4 个 runtime 工具进入真实启动注册链路`
- 严重级别：P1
- 状态：**已关闭**
- 修复方式：
  - `register_builtin_tools()` 末尾加入 4 个无状态 workspace RuntimeTool 的 `register_runtime()` 调用（`d93c4bd`）
  - 7 个有会话级 deps 的工具通过请求级工厂（`try_build_request_scoped_tool()`）在 `execute()` 调用时按请求构建，无需启动时注册（`2fb2dbf`）
  - 新增 `builtin_runtime_registration_test.rs` 验证启动注册生产链路

### Finding 8

- 标题：`[P1] 全局 runtime registry 仍无法安全承载会话级 runtime wrapper`
- 严重级别：P1
- 状态：**已关闭**
- 修复方式：
  - 明确分层模型：无状态工具（4 个 workspace tools）走全局 `register_runtime()` 注册；带 `conversation_id`/`run_id` 的工具（`WebSearchRuntimeTool`、5 个浏览器工具、`LoadFileRuntimeTool`）走请求级工厂，从 `PluginContext` 按请求即时构建（`2fb2dbf`）
  - `ToolRegistry::execute()` 改为三步路由：全局 runtime → 请求工厂 → legacy fallback

### Finding 9

- 标题：`[P2] workspace runtime 工具仍会在未授权目录时回退到内部 workspace`
- 严重级别：P2
- 状态：**已关闭（语义明确化）**
- 修复方式：
  - `require_workspace_root()` 补完整 doc comment，明确回退到 `workspace_path` 是有意设计：这 4 个工具对内部 Lotus workspace 始终可用，存在授权外部目录时切换到授权目录（`402c6ef`）
  - 这与 workspace-first 专项”先授权再暴露 schema”的语义是正交的：schema 暴露由 `build_visible_tool_defs()` 控制，工具执行逻辑本身允许回退

### Finding 10

- 标题：`[P2] QueryEngine 仍然不会给 runtime tool 注入 capability`
- 严重级别：P2
- 状态：**已关闭**（`402c6ef` + `ab8b44a`）
- 修复方式：
  - `QueryEngine` 加 `with_workspace_path()` builder，`run_tool_with_bus()` 中有 workspace_path 时构建 `CapabilityContext` 并注入（`402c6ef`）
  - 生产路径 `TauriChatCommandAdapter::new()` 中改为 `QueryEngine::new().with_workspace_path(services.file_mgr.workspace_path().to_path_buf())`（`ab8b44a`）

### 当前测试结论

471 lib 单测 + 全部 integration tests，0 失败。测试全绿不等于生产链路完整 runtime-first。

### 第二阶段关闭条件核对（Finding 7-10）

1. ✅ 启动时真实注册 4 个无状态 workspace RuntimeTool，7 个有状态工具走请求级工厂
2. ✅ `ToolRegistry::execute()` 三步路由（全局 → 工厂 → legacy fallback），聊天主链路命中 RuntimeTool
3. ✅ 生产 dispatcher（`to_runtime_dispatcher()`）切换到 `CapabilityPermissionPipeline`
4. ✅ 真实启动链路集成测试（`builtin_runtime_registration_test.rs`）
5. ✅ `QueryEngine` 携带 workspace_path，capability 在 query-engine 路径注入（`ab8b44a`）

---

## 第三阶段：深度链路复审 findings（Finding 11-13）

### Finding 11

- 标题：`[P1] 真实聊天主链路仍由 legacy executor 驱动，query_engine.run() 未被到达`
- 严重级别：P1
- 状态：✅ 已关闭（2026-04-13）
- 真实使用路径：`send_message -> run_chat_request() -> turn_executor.run_chat_turn() -> legacy_send_message_impl`
- 问题描述：
  - `SessionRuntime::run_chat_request()` 在 `turn_executor` 存在时，line 96 直接返回 `executor.run_chat_turn(request)`，完全绕过 `query_engine.run()`。
  - `TauriChatCommandAdapter::new()` 传入的正是 `TauriLegacyTurnExecutor`，因此生产聊天主链路永远走 `legacy_send_message_impl`，不走 `QueryEngine`。
  - 即便 `QueryEngine` 现在携带了 workspace_path 和 capability 注入逻辑，在当前生产路径下也永远不会被执行。
- 代码证据：
  - `src-tauri/src/runtime/session_runtime.rs:83-96` — `turn_executor` 存在时直接 return
  - `src-tauri/src/transport/tauri_commands/chat.rs:150-155` — 生产路径传入 `TauriLegacyTurnExecutor`
- 修复方向：
  - 这是整个 runtime-first 路径尚未完成的根本问题。需要 `QueryEngine` 接管工具调用循环，或在 legacy executor 内部主动调用 `registry.execute()` 走三步路由。当前 `ToolRegistry::execute()` 的三步路由事实上已经生效（legacy executor 调用 `registry.execute()`），所以工具执行层是 runtime-first 的，但 `QueryEngine` 自身的 turn 驱动链路仍未接通。
- 修复说明：
  - `SessionRuntime::run_chat_request()` 现在在 executor-backed 路径中会先构造 `TurnState`，标记为 executor-backed，再调用 `query_engine.run()` 走 preflight。
  - `QueryEngine` 对 executor-backed turn 会发出 `StreamStarted` runtime event，但不会生成假的 assistant delta/message，从而避免与 legacy executor 的已有 `streaming:*` / `message:updated` 事件重复。
  - 这样既让生产聊天主链路不再完全绕过 QueryEngine，也保持现有前端 legacy 事件合同不变。
- 验证测试：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_runtime_executor_bypass_test.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_runtime_executor_duplicate_events_test.rs`

### Finding 12

- 标题：`[P2] to_runtime_dispatcher() 只包含 4 个全局注册工具，7 个请求级工具不在其中`
- 严重级别：P2
- 状态：✅ 已关闭（2026-04-13）
- 真实使用路径：`to_runtime_dispatcher(plugin_ctx)` → 只遍历 `self.runtime_tools`（4 个） + legacy fallback
- 问题描述：
  - `to_runtime_dispatcher()` 构建的 `ToolDispatcher` 只包含全局注册的 4 个 workspace RuntimeTool 和剩余 legacy ToolPlugin。
  - `try_build_request_scoped_tool()` 工厂逻辑只存在于 `execute()` 方法中，`to_runtime_dispatcher()` 不调用它。
  - 这意味着通过 `to_runtime_dispatcher()` 路径（未来 QueryEngine 主链路会走这里）执行 web_search / browse_* / load_file 时，这些工具会退化为 LegacyToolAdapter，而不是 RuntimeTool。
- 代码证据：
  - `src-tauri/src/plugin/registry.rs:316-338` — `to_runtime_dispatcher()` 只看 `self.runtime_tools`
- 修复说明：
  - `to_runtime_dispatcher()` 现在会基于 `PluginContext` 调用 request-scoped factory，把 `web_search`、5 个浏览器工具和 `load_file` 一并注册进 dispatcher。
  - 同时 legacy ToolPlugin 只在“既不是全局 RuntimeTool、也不是请求级 RuntimeTool”时才作为 fallback 注册。
  - 对无 browser connector 的场景，浏览器工具也会以 RuntimeTool 形式进入 dispatcher，并由 `CapabilityPermissionPipeline` 在权限层拒绝，而不是退回 legacy adapter。
- 验证测试：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_atomic_tool_closure_test.rs`
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/builtin_runtime_registration_test.rs`

### Finding 13

- 标题：`[P2] 7 个请求级工具的 schema 仍来自 legacy plugin.input_schema()，不是 catalog 单一真相源`
- 严重级别：P2
- 状态：✅ 已关闭（2026-04-13）
- 真实使用路径：`get_all_schemas() / get_schemas_filtered()` → runtime_tools（4 个走 catalog）+ legacy tools（其余走 plugin.input_schema()）
- 问题描述：
  - `get_all_schemas()` 和 `get_schemas_filtered()` 对于不在 `self.runtime_tools` 中的工具（包括 web_search、5 个浏览器工具、load_file）仍从 legacy `ToolPlugin::input_schema()` 取 schema。
  - 这 7 个工具在 `ToolCatalog` 中都有准确的 entry（含 ToolKind、capability_scope、json_schema），但当前 schema 暴露路径不读 catalog，而是读 legacy plugin 的 hardcoded JSON。
  - 专项 AC-1 验收标准"llm/tools.rs 不再作为 LLM schema 主来源"对于这 7 个工具仍未成立。
- 代码证据：
  - `src-tauri/src/plugin/registry.rs:133-143` — legacy tools 走 `rt.plugin.input_schema()`
  - `src-tauri/src/runtime/tools/catalog.rs` — 7 个工具的 catalog entry 存在但未被 schema 暴露路径使用
- 修复说明：
  - `ToolRegistry::get_all_schemas()` 与 `get_schemas_filtered()` 现在把 7 个请求级 RuntimeTool 视为 runtime schema source。
  - 这些工具即使不进入全局 `runtime_tools` map，也会直接从 `ToolCatalog` 暴露 schema 与 description。
  - legacy `plugin.input_schema()` 只保留给真正尚未迁移的工具。
- 验证测试：
  - `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_atomic_tool_closure_test.rs`
