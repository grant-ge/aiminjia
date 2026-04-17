# lotus-app 设计问题审计报告

**日期**：2026-04-17  
**调查方式**：三路并行 agent 深度代码审计（Runtime/编排层、工具/权限层、LLM/存储层）  
**性质**：设计问题（架构反模式、会阻碍演进的决策），区别于功能差距

> 参见功能差距文档：`docs/2026-04-17-full-gap-assessment.md`

---

## 一、问题总览

| # | 问题 | 层次 | 严重程度 |
|---|------|------|---------|
| D1 | Schema / 注册 / 执行三者不一致（静默 bug 风险） | 工具 | **严重** |
| D2 | `build_env_info` 同步阻塞 tokio 工作线程 | Runtime | **严重** |
| D3 | Executor 持有 `tauri::AppHandle`（打破分层约束） | Runtime | **严重** |
| D4 | 权限管线 scope 匹配逻辑重复，不可组合 | 权限 | 设计债（重） |
| D5 | `CapabilityContext` 同时承担权限凭证和服务注入两个职责 | 工具 | 设计债（重） |
| D6 | LegacyToolAdapter 捕获全量 PluginContext，迁移逃生门持续开放 | 工具 | 设计债（重） |
| D7 | LLM 层直接持有 `AppStorage`，存储与业务逻辑硬耦合 | LLM/存储 | 设计债（重） |
| D8 | Prompt 构建散落两处，无清晰单一构建点 | LLM | 设计债（重） |
| D9 | `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 硬编码，需手动与执行逻辑同步 | 工具 | 设计债（中） |
| D10 | `TOOL_CATALOG` 静态固化，动态工具（MCP）无路可走 | 工具 | 设计债（中） |
| D11 | 每次 LLM step 重复读取 DB settings + 解密（turn 内应读一次） | Runtime | 设计债（中） |
| D12 | `TurnConfig.workspace_path` 用空 `PathBuf` 作 sentinel | Runtime | 轻微 |
| D13 | `CapabilityContext` 构建代码两处重复（约 20 行） | Runtime | 轻微 |
| D14 | `delete_conversation` 直接 `app.emit()` 绕过 RuntimeEventBus | Runtime | 轻微 |
| D15 | `authorize()` 同步 trait，异步权限检查扩展困难 | 权限 | 轻微 |
| D16 | ConversationStore trait 未全面覆盖（runtime 内 19 处仍直接用 AppStorage） | 存储 | 设计债（中） |
| D17 | 存储格式无版本迁移机制（conversation/messages 无 schema version） | 存储 | 轻微 |

---

## 二、严重问题详解

### D1：Schema / 注册 / 执行三者不一致

**位置**：`src-tauri/src/plugin/registry.rs:153-173`

**问题**：runtime 工具的 schema 来自 `TOOL_CATALOG`，执行注册在 `runtime_tools HashMap`，两者完全独立维护。`get_all_schemas()` 遍历注册表从 catalog 取 schema——若工具在 catalog 但未注册到 registry（或反之），会静默丢失 schema 或暴露无法执行的工具给 LLM。

**影响**：LLM 收到的工具列表与实际可执行集合可能不一致，导致工具调用失败。这是现实 bug 风险，不只是设计债。

**修复方向**：注册一个工具时同时更新 catalog entry，或在注册时做一致性校验（compile-time 或 startup assert）。

---

### D2：`build_env_info` 同步阻塞 tokio 工作线程

**位置**：`src-tauri/src/runtime/chat/context_builder.rs:115`

**问题**：`build_env_info` 是同步函数，内部调用 `std::process::Command::new("git").output()`（阻塞 syscall）。在 async 上下文中从 `get_env_info` 调用时，会阻塞整个 tokio 工作线��，影响并发性能。

**影响**：每次 turn 构建上下文时阻塞线程池，高并发场景下延迟显著上升。

**修复方向**：改为 `tokio::process::Command`，或用 `tokio::task::spawn_blocking` 包裹。

---

### D3：Executor 持有 `tauri::AppHandle`（打破分层约束）

**位置**：`src-tauri/src/transport/tauri_commands/chat.rs:124, 812, 871`

**问题**：`TauriLegacyTurnExecutor` 内嵌 `app: tauri::AppHandle`，在 `build_user_message_content`/`load_history`/`get_env_info` 里调用 `load_authorized_workspace(&self.services.app, ...)`。Executor 本应是纯 provider adapter，但实际上持有 Tauri handle，导致 runtime 层无法在不启动 Tauri 的情况下测试。

**影响**：违反 CLAUDE.md 明确的约束（`runtime/` 禁止 `use tauri::*`）；集成测试必须带 Tauri 运行，测试成本高。

**对标**：claude-code-best 的 `ToolUseContext` 通过构造器注入所有依赖，无框架耦合。

**修复方向**：将 `AppHandle` 的职责（workspace 加载、路径解析）抽为 trait，由构造时注入。

---

## 三、设计债（重）详解

### D4：权限管线 scope 匹配逻辑重复

**位置**：`src-tauri/src/runtime/tools/permission.rs:84-171`（CapabilityPermissionPipeline）和 `185-286`（StorePolicyPipeline）

**问题**：两个 pipeline 重复了全部 scope 匹配逻辑（`workspace:read/write`、`browser`、`python:exec`、`network`、unknown scope），差异仅在"unknown scope → Deny vs Ask"。三个 pipeline 平行实现，无法链式组合——若想"先查 store 再做 capability 检查"，只能在 StorePolicyPipeline 内联 capability 逻辑（现状）。

**影响**：新增 capability scope（如 `audio:capture`）需在两处同步修改，极易遗漏。

**修复方向**：将 scope 匹配逻辑提取为共享函数；pipeline 改为责任链模式（可组合）。

---

### D5：`CapabilityContext` 同时承担权限凭证和服务注入

**位置**：`src-tauri/src/runtime/tools/capability.rs:82-98`

**问题**：`CapabilityContext` 同时承担两个职责：
1. **权限凭证**：`browser_available`、`storage`（决定工具是否有权执行）
2. **服务注入**：`file_ops`、`read_file_state`、`notification_sink`（工具运行时需要的服务）

`file_ops` 注释明确写"only for load_file"——每增加一个需注入服务的工具，就往 struct 加一个新 `Option<Arc<dyn Trait>>`，是开放的服务定位器，会无限膨胀。

**影响**：权限判断与依赖注入边界模糊；测试构造成本随字段增加而上升。

**修复方向**：拆分为 `CapabilityCredential`（权限凭证，小而稳定）和 `ServiceContainer`（服务注入，可按需扩展）。

---

### D6：LegacyToolAdapter 捕获全量 PluginContext

**位置**：`src-tauri/src/runtime/tools/legacy_adapter.rs:63-88`

**问题**：`from_plugin()` 将整个 `PluginContext`（含 `gateway`、`auth_manager` 等编排层对象）一起注入 handler 闭包。旧工具通过 adapter 可访问任意编排层能力，没有 `CapabilityContext` 约束。

**影响**：只要 LegacyToolAdapter 存在，工具永远没有迁移压力，无法 audit 哪些工具在滥用全局上下文；CapabilityContext 的隔离约束对 legacy 工具完全失效。

**修复方向**：逐步收窄 adapter 传入的 context（即使是 PluginContext 也只传最小字段集），并追踪哪些工具实际用了哪些字段，作为迁移优先级输入。

---

### D7：LLM 层直接持有 `AppStorage`

**位置**：`src-tauri/src/llm/gateway.rs:112`、`llm/tool_executor/*.rs`

**问题**：`LlmGateway` 持有 `Arc<AppStorage>`；`tool_executor` 中工具直接操作 `AppStorage`。LLM 层应该面向编排层（runtime），不应知道存储实现。

**影响**：无法切换存储后端；测试时难以 mock 存储；LLM 层与存储格式强耦合，存储演进会波及 LLM 层。

---

### D8：Prompt 构建散落两处

**位置**：`src-tauri/src/llm/prompts.rs` + `src-tauri/src/runtime/chat/chat_turn_driver.rs:265-266`

**问题**：system prompt 在两个地方并行构建：`llm/prompts.rs:get_system_prompt()` 返回完整 prompt；`chat_turn_driver.rs:build_system_prompt()` 再构建一遍。

**影响**：prompt 改动需同时改两处；无清晰的 prompt 构建责任边界；容易引入 prompt 漂移 bug（两处不一致）。

**修复方向**：确立单一的 prompt 构建入口，`llm/prompts.rs` 降级为纯文本片段仓库，构建逻辑收归 `context_builder.rs`。

---

## 四、中等设计债

### D9：`REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 硬编码

**位置**：`registry.rs:25-33` + `try_build_request_scoped_tool():479-545`

新增 request-scoped 工具需同时改两处，遗漏无编译错误，导致 schema/执行静默不一致。

**修复方向**：工具自声明 `fn is_request_scoped() -> bool`，由 registry 在注册时自动归类。

---

### D10：`TOOL_CATALOG` 静态固化

**位置**：`src-tauri/src/runtime/tools/catalog.rs`

`LazyLock<ToolCatalog>` 进程启动时一次性构建，不可变。MCP 工具、用户自定义工具无法注册到 catalog，只能走 legacy 路径。与未来"动态工具系统"目标直接冲突。

**修复方向**：将 `TOOL_CATALOG` 改为可在 runtime 注册的 `Arc<RwLock<ToolCatalog>>`，或引入 `DynamicToolRegistry` 层。

---

### D11：每次 LLM step 重复读取 DB settings

**位置**：`transport/tauri_commands/chat.rs:149-165 run_llm_step`

`run_llm_step` 每次调用都执行 `db.get_all_settings()` 并对 3 个 API key 做解密。Turn 内这些设置是稳定的，浪费 I/O 和 CPU。

---

### D16：ConversationStore trait 未全面覆盖

**位置**：`runtime/` 内 19 处直接 `use crate::storage::file_store::AppStorage`

`runtime/store/conversation_store.rs` 是好设计（trait 隔离），但同时还有 19 处代码直接访问 `AppStorage`。两套 API 并存，测试一致性难以保证。

---

## 五、轻微问题

| 问题 | 位置 | 修复成本 |
|------|------|---------|
| D12：`workspace_path` 用空 `PathBuf` 作 sentinel | `chat_turn_driver.rs:285` | 改为 `Option<PathBuf>` |
| D13：`CapabilityContext` 构建代码两处重复 | `query_engine.rs:196-217, 339-358` | 提取私有方法 |
| D14：`delete_conversation` 直接 `app.emit()` 绕过 bus | `chat.rs:1216-1229` | 改发 RuntimeEvent |
| D15：`authorize()` 同步 trait | `permission.rs:46-53` | 改为 `async fn`，破坏性改动 |
| D17：存储格式无 schema version | `conversations.rs`、`messages.rs` | 加 version 字段 + 迁移函数 |

---

## 六、修复优先级建议

```
立即（影响正确性）
  D1  Schema/注册/执行不一致（现实 bug 风险）
  D2  build_env_info 阻塞 tokio（性能）
  D3  Executor 持有 AppHandle（分层约束违反）

短期（影响可维护性）
  D4  权限管线重复逻辑 → 责任链重构
  D8  Prompt 构建两处 → 统一入口
  D11 settings 每步重读 → turn 入口读一次

中期（影响演进能力）
  D5  CapabilityContext 拆分（权限凭证 vs 服务注入）
  D7  LLM 层解耦 AppStorage
  D10 TOOL_CATALOG 动态化（MCP 前置条件）
  D16 ConversationStore 全面覆盖

长期（技术债清理）
  D6  LegacyToolAdapter 逐步收窄
  D9  REQUEST_SCOPED 工具自声明
  D12-D15 轻微修复
  D17 存储 schema version
```
