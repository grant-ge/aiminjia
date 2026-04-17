# 设计债系统清理计划（Plan-F）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 清理 7 项核心设计债，提升代码可维护性：schema 一致性保证、权限管线去重、prompt 构建统一、settings 缓存、前端 store 拆分、listener 泄露修复、SessionId 推广。

**Architecture:** 7 个子任务完全独立，可按任意顺序执行，每个独立 commit。F1-F4 是 Rust 后端，F5-F6 是前端 TypeScript，F7 横跨前后端（主要后端）。

**Tech Stack:** Rust, TypeScript/React, Zustand

**Worktree branch:** `refactor/design-debt`

---

## 改造视角

> 设计债清理是重构，不是新功能。每个子任务的目标是改善代码结构，使其更易维护和演进，**不应改变可观察行为**。现有测试全绿是验收标准。

### 重构原则

1. **行为不变**：重构后所有现有测试必须继续通过
2. **独立 commit**：每个子任务独立 commit，出问题可单独回滚
3. **最小改动**：不引入新功能，只改结构

---

### F1：Schema/注册一致性校验

**当前问题**：`TOOL_CATALOG` 和 `runtime_tools HashMap` 独立维护，不一致时静默丢失 schema，LLM 可能收到无法执行的工具。

**改造目标**：启动期（`ToolRegistry::new()` 或 `setup_builtin_tools`）加一致性校验，不一致时 panic（快速失败，而非运行时静默错误）。

**改造范围**：`plugin/registry.rs`，不影响运行时行为。

---

### F2：权限管线 scope 匹配逻辑去重

**当前问题**：`CapabilityPermissionPipeline` 和 `StorePolicyPipeline` 各自内联 scope 匹配逻辑，新增 scope 需改两处。

**改造目标**：提取 `fn check_scope_capability(scope, ctx) -> Option<PermissionDecision>` 共享函数，两个 pipeline 调用它。**注意**：unknown scope 的处理差异（Deny vs Ask）保持不变，不统一。

**改造范围**：`runtime/tools/permission.rs`，接口不变，只是内部重构。

---

### F3：Prompt 构建统一入口

**当前问题**：`llm/prompts.rs` 和 `chat_turn_driver.rs` 各有一处 prompt 构建，文档中职责不清晰。

**改造目标**：明确 `llm/prompts.rs` 是片段仓库（只返回字符串片段），`context_builder.rs` 是唯一组装入口，用注释和文档标明职责。不需要移动代码，只需确认并文档化。

---

### F4：settings 每步重读优化

**当前问题**：`run_llm_step` 每次调用都 `get_all_settings()` + 解密 API key，一次 turn 最多 30 次重复 I/O。

**改造目标**：turn 入口读一次，存入 `TurnConfig`，`run_llm_step` 从 config 取。

**改造范围**：`transport/tauri_commands/chat.rs` + `turn_config.rs`，不改 turn 逻辑。

---

### F5：chatStore 拆分

**当前问题**：`chatStore.ts` 同时管理会话 CRUD 和流式状态，`deriveLegacy` 是两者混合的证明。

**改造目标**：拆分为 `sessionStore.ts`（会话 CRUD）和 `streamingStore.ts`（流式状态），`chatStore.ts` 变为薄的组合层，向后兼容现有调用方。

**改造范围**：`src/stores/`，不改组件和 hooks 的使用接口（向后兼容）。

---

### F6：useTauriEvent listener 泄露修复

**当前问题**：`useTauriEvent.ts` 的 `setup()` reject 路径无 `.catch`，listener 泄露无感知。

**改造目标**：加 error boundary，cleanup 确保 unlisten 被调用。行为不变，只是更健壮。

---

### F7：SessionId newtype 推广

**当前问题**：runtime 层 `conversation_id` 大量用裸 `String`，类型系统无法防止误传。

**改造目标**：`QueryEngine`、`TurnConfig`、`ToolExecutionContext` 的接口改用 `SessionId`，编译期保证类型安全。不需要全量替换，优先改 runtime 层核心接口。

---

### 整体回归验证

每个子任务完成后：
```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
pnpm test
```

---

## 背景与问题诊断

在阅读代码后，识别出以下实际存在的设计债：

| ID | 文件 | 具体问题 |
|----|------|---------|
| D1 | `plugin/registry.rs` `get_all_schemas` | `TOOL_CATALOG` 条目和 `runtime_tools` HashMap 独立维护。`get_all_schemas` 中对 runtime_tools 迭代：若某工具在 `runtime_tools` 注册但 catalog 缺失条目，`TOOL_CATALOG.get_entry(id)` 返回 `None`，该工具的 schema 被静默丢弃，LLM 看不到它。反向亦然：catalog 有条目但无 runtime 注册时，schema 不会暴露给 LLM（除非通过 REQUEST_SCOPED 列表覆盖）。启动期无任何校验。 |
| D4 | `runtime/tools/permission.rs` | `CapabilityPermissionPipeline::authorize` 和 `StorePolicyPipeline::authorize` 各自内联了相同的 scope 匹配逻辑（`workspace:read/write`、`browser`、`python:exec`、`network`、unknown）。两段逻辑高度相似但有细微差异（`StorePolicyPipeline` 将 `python:exec` 与 workspace 合并为同一分支；`CapabilityPermissionPipeline` 单独处理），未来新增 scope 时需同步修改两处。 |
| D8 | `llm/prompts.rs` + `runtime/chat/context_builder.rs` | `llm/prompts.rs` 的 `build_system_prompt_parts` 承担完整的 system prompt 组装（base + 工具偏好 + persona + mode-specific prompt）。`context_builder.rs` 负责 dynamic context（iteration context + env info）。分工已经较清晰，但 `prompts.rs` 既是文本片段仓库又是组装入口，职责未分层。`get_system_prompt` shim 是向后兼容的外观，可保留但需标注。 |
| D11 | `transport/tauri_commands/chat.rs` `run_llm_step` (L148-165) | `TauriLegacyTurnExecutor::run_llm_step` 每次被调用时都执行 `db.get_all_settings()` + 解密 API key。该函数在 agent loop 的每一次 LLM step 都会被调用（最多 30 次）。settings 在一个 turn 期间不会改变，重复读取是纯粹的性能浪费，也是不必要的 I/O。`TurnConfig` 结构体中有 `conversation_id`、`run_id`，但没有 `resolved_settings`。 |
| F1 | `src/stores/chatStore.ts` | 单一 store 管理两类关注点：会话/消息 CRUD（`conversations`、`messages`、`activeConversationId`、`setConversations`、`setMessages`、`addMessage`、`updateMessage`）和流式/工具执行状态（`streamStates`、`busyConversations`、`taskStates`）。`deriveLegacy` 函数混杂两者。随功能增长，测试和维护难度上升。 |
| F3 | `src/hooks/useTauriEvent.ts` | `setup().then(fn => ...)` 的 Promise resolve 路径在组件已 unmount 时会立即调用 `fn()` 清理。但若 `setup()` reject（例如 Tauri listen 失败），`mounted = false` + `unlisten = undefined`，cleanup 函数中 `unlisten?.()` 是空操作，错误被静默吞掉。`useStreaming.ts` 中 11 个独立的 `useTauriEvent` 调用串行注册，任一失败均无 error boundary。 |
| R2 | `runtime/ids.rs` + 多处使用方 | `SessionId` newtype 已在 `runtime/ids.rs` 定义，并在 `ToolExecutionContext` 中使用。但 `ChatTurnRequest.conversation_id`、`TurnConfig.conversation_id`、`LlmStepInput.conversation_id` 均是裸 `String`，`BrowserDeps.conversation_id`、`DefaultFileOperations.conversation_id` 也是裸 `String`。运行时关键路径上混用两套类型，类型系统无法防止 run_id 和 conversation_id 意外传反。 |

---

## Task F1：Schema/注册一致性校验（对应 D1）

**问题根源：** `register_builtin_tools` 在 `plugin/builtin/tools/mod.rs` 中注册 4 个 `RuntimeTool`（list_directory、read_workspace_file、search_files、get_file_info），但无校验确保每个注册的 runtime tool id 都在 `TOOL_CATALOG` 中有对应条目。反向也无校验。若 catalog 和注册表脱节，`get_all_schemas` 静默丢弃 schema，LLM 无法调用该工具。

**修复目标：** 在 `register_builtin_tools` 函数末尾（或 `ToolRegistry` 提供的新校验方法中），遍历所有已注册 runtime tool id，确保每个 id 在 `TOOL_CATALOG` 中有对应条目，不一致时 `panic!`（启动期快速失败）。

**测试文件：** `src-tauri/tests/review_schema_registry_consistency_test.rs`

### 步骤

- [ ] **F1-1 写失败测试**

  在 `src-tauri/tests/review_schema_registry_consistency_test.rs` 创建测试：

  ```rust
  // 测试：ToolRegistry 提供的校验方法，对已注册 runtime tool 验证 catalog 完整性
  #[test]
  fn review_all_runtime_tools_have_catalog_entries() {
      // 使用 TOOL_CATALOG 的 all_ids() 和模拟的 runtime_tools 注册
      // 断言：registry 的 validate_catalog_consistency() 在注册了不在 catalog 中的工具时 panic
      // 断言：正常注册（catalog 中存在的工具）不 panic
  }

  #[test]
  fn review_registered_workspace_tools_all_in_catalog() {
      // 直接查询 TOOL_CATALOG 中是否存在 list_directory/read_workspace_file/
      // search_files/get_file_info 四个条目
      use lotus_app::runtime::tools::catalog::TOOL_CATALOG;
      for id in &["list_directory", "read_workspace_file", "search_files", "get_file_info"] {
          assert!(
              TOOL_CATALOG.get_entry(id).is_some(),
              "TOOL_CATALOG missing entry for '{}' — add it before registering as RuntimeTool",
              id
          );
      }
  }
  ```

  运行确认测试失败（如果 `validate_catalog_consistency` 还不存在）：
  ```bash
  cd src-tauri && cargo test review_schema_registry_consistency -- --nocapture 2>&1 | tail -20
  ```

- [ ] **F1-2 实现 `ToolRegistry::validate_catalog_consistency`**

  在 `src-tauri/src/plugin/registry.rs` 的 `ToolRegistry` impl 块中新增：

  ```rust
  /// 校验所有已注册 RuntimeTool 在 TOOL_CATALOG 中有对应条目。
  /// 调用时机：register_builtin_tools 完成后，应用启动时。
  /// 不一致时 panic，确保启动期快速失败。
  pub async fn validate_catalog_consistency(&self) {
      use crate::runtime::tools::catalog::TOOL_CATALOG;
      let runtime_tools = self.runtime_tools.read().await;
      for id in runtime_tools.keys() {
          assert!(
              TOOL_CATALOG.get_entry(id).is_some(),
              "ToolRegistry consistency error: RuntimeTool '{}' is registered but has no \
               entry in TOOL_CATALOG. Add the catalog entry before registering the tool.",
              id
          );
      }
      log::info!(
          "ToolRegistry catalog consistency check passed ({} runtime tools verified)",
          runtime_tools.len()
      );
  }
  ```

  在 `src-tauri/src/plugin/builtin/tools/mod.rs` 的 `register_builtin_tools` 函数末尾调用：

  ```rust
  pub async fn register_builtin_tools(registry: &ToolRegistry) {
      // ... existing registrations ...
      registry.validate_catalog_consistency().await;
  }
  ```

- [ ] **F1-3 验证测试通过**

  ```bash
  cd src-tauri && cargo test review_schema_registry_consistency -- --nocapture
  cd src-tauri && cargo test review_all_runtime_tools -- --nocapture
  ```

- [ ] **F1-4 运行现有回归测试确认无破坏**

  ```bash
  cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
  cd src-tauri && cargo test tool_catalog -- --nocapture
  ```

- [ ] **F1-5 commit**

  ```bash
  git add src-tauri/src/plugin/registry.rs src-tauri/src/plugin/builtin/tools/mod.rs \
          src-tauri/tests/review_schema_registry_consistency_test.rs
  git commit -m "feat(registry): validate_catalog_consistency — startup panic on RuntimeTool/catalog mismatch (D1)"
  ```

---

## Task F2：权限管线 scope 匹配逻辑去重（对应 D4）

**问题根源：** `CapabilityPermissionPipeline::authorize`（L97-170）和 `StorePolicyPipeline::authorize`（L226-279）各有一套 `match scope.as_str()` 分支。两处逻辑相似但不完全一致：
- `CapabilityPermissionPipeline` 中 `python:exec` 单独处理，检查 `ctx.capability.storage`
- `StorePolicyPipeline` 中 `python:exec` 与 `workspace:read/write` 合并为同一 match arm

未来若新增 scope（如 `clipboard`、`shell`），需同步修改两处，极易遗漏。

**修复目标：** 提取 `fn check_scope_capability(scope: &str, ctx: &ToolExecutionContext, tool_id: &str) -> Option<PermissionDecision>` 共享函数，两个 pipeline 的 scope 匹配分支均调用它。`StorePolicyPipeline` 在共享函数之外保留其 stored-policy 查询逻辑（不受影响）。

**测试文件：** `src-tauri/tests/review_permission_scope_dedup_test.rs`

### 步骤

- [ ] **F2-1 写失败测试**

  在 `src-tauri/tests/review_permission_scope_dedup_test.rs` 创建测试，验证两个 pipeline 对所有已知 scope 的行为完全一致（无 stored policy 时）：

  ```rust
  use lotus_app::runtime::tools::permission::{
      CapabilityPermissionPipeline, PermissionDecision, PermissionPipeline, StorePolicyPipeline,
  };
  use lotus_app::runtime::tools::definition::ToolDefinition;
  use lotus_app::runtime::tools::ToolExecutionContext;
  use lotus_app::runtime::store::permission_store::PermissionStore;
  use lotus_app::runtime::ids::{SessionId, RunId};

  fn make_ctx_with_storage() -> ToolExecutionContext { /* ... */ }
  fn make_ctx_without_storage() -> ToolExecutionContext { /* ... */ }
  fn make_def(id: &str, scopes: &[&str]) -> ToolDefinition { /* ... */ }

  #[test]
  fn review_capability_and_store_pipelines_agree_on_workspace_read_with_storage() {
      let ctx = make_ctx_with_storage();
      let def = make_def("test_tool", &["workspace:read"]);
      let cap = CapabilityPermissionPipeline;
      let store_pipeline = StorePolicyPipeline::new(Arc::new(PermissionStore::new_empty()));
      assert!(matches!(cap.authorize(&def, &serde_json::Value::Null, &ctx), PermissionDecision::Allow { .. }));
      assert!(matches!(store_pipeline.authorize(&def, &serde_json::Value::Null, &ctx), PermissionDecision::Allow { .. }));
  }

  #[test]
  fn review_capability_and_store_pipelines_agree_on_workspace_read_without_storage() {
      // 无 storage → 两者均 Deny
  }

  #[test]
  fn review_capability_and_store_pipelines_agree_on_python_exec_without_storage() {
      // python:exec 无 storage → 两者均 Deny
  }

  #[test]
  fn review_capability_and_store_pipelines_agree_on_network_always_allow() {
      // network → 两者均 Allow
  }
  ```

  此时测试应通过（验证当前行为一致性），用于防止重构后引入回归。

- [ ] **F2-2 提取 `check_scope_capability` 共享函数**

  在 `src-tauri/src/runtime/tools/permission.rs` 中，在两个 pipeline impl 块之前新增模块级私有函数：

  ```rust
  /// 检查单个 capability scope 是否满足。
  /// 返回 `Some(Deny)` 表示缺少对应 capability，`None` 表示 capability 满足（或 scope 不需要 capability 检查）。
  /// unknown scope 返回 `Some(Deny)`（fail-closed）；调用方可选择覆盖为 `Ask`。
  fn check_scope_capability(
      scope: &str,
      ctx: &ToolExecutionContext,
      tool_id: &str,
  ) -> Option<PermissionDecision> {
      match scope {
          "workspace:read" | "workspace:write" | "python:exec" => {
              if ctx.capability.as_ref().and_then(|c| c.storage.as_ref()).is_none() {
                  Some(PermissionDecision::Deny {
                      message: format!(
                          "Tool '{}' requires workspace capability (scope: {}). \
                           Authorize a workspace directory first.",
                          tool_id, scope
                      ),
                      reason: PermissionReason::Capability,
                  })
              } else {
                  None
              }
          }
          "browser" => {
              let has_browser = ctx.capability.as_ref()
                  .map(|c| c.has_browser_capability())
                  .unwrap_or(false);
              if !has_browser {
                  Some(PermissionDecision::Deny {
                      message: format!(
                          "Tool '{}' requires browser capability. \
                           A browser connector must be active.",
                          tool_id
                      ),
                      reason: PermissionReason::Capability,
                  })
              } else {
                  None
              }
          }
          "network" => None,
          other => {
              log::debug!("Unknown capability scope '{}' for tool '{}' — denying.", other, tool_id);
              Some(PermissionDecision::Deny {
                  message: format!(
                      "Tool '{}' requests unknown capability scope '{}'. Deny by default.",
                      tool_id, other
                  ),
                  reason: PermissionReason::UnknownScope,
              })
          }
      }
  }
  ```

  **注意：** `StorePolicyPipeline` 对 unknown scope 的行为是 `Ask`（而非 `Deny`）。这是两者的合理差异，不应统一。`check_scope_capability` 返回 `Deny` 作为默认，`StorePolicyPipeline` 调用后检查 `PermissionReason::UnknownScope`，若匹配则替换为 `Ask`。

- [ ] **F2-3 重构两个 pipeline 的 scope 循环使用共享函数**

  `CapabilityPermissionPipeline::authorize` 中的 for 循环替换为：
  ```rust
  for scope in &definition.capability_scope {
      if let Some(decision) = check_scope_capability(scope.as_str(), ctx, &definition.id) {
          return decision;
      }
  }
  ```

  `StorePolicyPipeline::authorize` 中 stored policy 查询之后的 scope capability 检查替换为：
  ```rust
  // (stored policy 查询逻辑不变)
  None => {}
  }
  // 共享 capability 检查
  if let Some(mut decision) = check_scope_capability(scope.as_str(), ctx, &definition.id) {
      // Unknown scope → StorePolicyPipeline 升级为 Ask
      if matches!(decision, PermissionDecision::Deny { reason: PermissionReason::UnknownScope, .. }) {
          decision = PermissionDecision::Ask {
              message: format!(
                  "Tool '{}' requests capability scope '{}' which is not recognized. \
                   Do you want to allow it?",
                  definition.id, scope
              ),
              suggestions: vec!["Allow once".into(), "Always allow".into(), "Deny".into()],
              reason: PermissionReason::UnknownScope,
          };
      }
      return decision;
  }
  ```

- [ ] **F2-4 验证行为一致性测试通过，且现有权限测试全部通过**

  ```bash
  cd src-tauri && cargo test review_permission_scope_dedup -- --nocapture
  cd src-tauri && cargo test permission -- --nocapture
  cd src-tauri && cargo test review_permission -- --nocapture
  ```

- [ ] **F2-5 commit**

  ```bash
  git add src-tauri/src/runtime/tools/permission.rs \
          src-tauri/tests/review_permission_scope_dedup_test.rs
  git commit -m "refactor(permission): extract check_scope_capability — dedup scope match logic between two pipelines (D4)"
  ```

---

## Task F3：Prompt 构建职责分层（对应 D8）

**现状澄清：** 经过阅读代码，`llm/prompts.rs` 和 `runtime/chat/context_builder.rs` 的职责实际上已经较清晰分工：
- `prompts.rs`：负责 system prompt（base + 工具偏好 + persona + mode-specific）的组装，`build_system_prompt_parts` 是组装入口
- `context_builder.rs`：负责 dynamic context（每次迭代注入的上下文），`build_iteration_context` 和 `build_env_info` 是入口

真正的问题是 `prompts.rs` 把"片段存储"（`PromptStore`、`get_base_prompt`、`get_browser_agent_prompt`）和"组装逻辑"（`build_system_prompt_parts`）混在同一文件中，没有明确的分层边界。

**修复目标：**
1. 在 `prompts.rs` 中以 doc comment 明确标注 `PromptStore` 的"纯片段仓库"职责和 `build_system_prompt_parts` 的"唯一组装入口"职责
2. 将 `TOOL_PREFERENCE_SECTION` 常量单独注释说明其静态内容性质，确保未来新增工具偏好时只修改此处
3. 添加架构约束测试：确认不存在其他路径直接拼接 base prompt 而绕过 `build_system_prompt_parts`

**测试文件：** `src-tauri/tests/review_prompt_single_assembly_point_test.rs`

### 步骤

- [ ] **F3-1 写架构约束测试**

  在 `src-tauri/tests/review_prompt_single_assembly_point_test.rs` 创建测试，验证 `build_system_prompt_parts` 是 system prompt 的唯一组装入口，且 `context_builder` 不负责组装 system prompt：

  ```rust
  /// 验证 system prompt 组装只通过 llm::prompts 模块，
  /// context_builder 只处理 dynamic context（iteration context + env info）。
  #[test]
  fn review_prompt_assembly_is_in_prompts_module_not_context_builder() {
      // build_iteration_context 的返回值必须以 "[动态上下文" 开头
      // 不应包含 "base" prompt 或 TOOL_PREFERENCE_SECTION 的内容
      use lotus_app::runtime::chat::context_builder::build_iteration_context;
      let result = build_iteration_context("mem", "ws", "files", "notes", None, None, None);
      assert!(result.starts_with("[动态上下文"), "context_builder output must be dynamic context, not system prompt");
      assert!(!result.contains("工具选择偏好"), "context_builder must NOT include TOOL_PREFERENCE_SECTION");
  }

  #[test]
  fn review_build_system_prompt_parts_is_sole_assembler() {
      // build_system_prompt_parts 必须包含 base + TOOL_PREFERENCE_SECTION + mode content
      use lotus_app::llm::prompts::{build_system_prompt_parts, PromptMode};
      let parts = build_system_prompt_parts(PromptMode::Daily, None, None);
      assert!(parts.static_section.contains("工具选择偏好"), 
          "static_section must contain TOOL_PREFERENCE_SECTION");
      // dynamic_section 不应重复 static_section 的内容
      assert!(!parts.dynamic_section.contains("工具选择偏好"),
          "dynamic_section must NOT duplicate TOOL_PREFERENCE_SECTION");
  }
  ```

- [ ] **F3-2 在 `prompts.rs` 中添加职责分层注释**

  在 `src-tauri/src/llm/prompts.rs` 文件顶部的模块注释更新，以及在关键函数上方添加 doc comment，明确：
  - `PromptStore` 是纯文本片段的持久化存储，不应在此之外新增 prompt 组装逻辑
  - `build_system_prompt_parts` 是 system prompt 的**唯一**组装入口
  - `get_system_prompt` 是向后兼容的外观，不应新增使用方

  同时在 `TOOL_PREFERENCE_SECTION` 常量上方添加注释：
  ```rust
  // 工具选择偏好章节 — 静态内容，所有 mode 均包含。
  // 新增工具偏好时只在此处修改，勿在 daily.md / base.md 重复。
  ```

- [ ] **F3-3 验证测试通过**

  ```bash
  cd src-tauri && cargo test review_prompt_single_assembly -- --nocapture
  cd src-tauri && cargo test -- -p lotus-app test_build_system_prompt --nocapture
  ```

- [ ] **F3-4 commit**

  ```bash
  git add src-tauri/src/llm/prompts.rs \
          src-tauri/tests/review_prompt_single_assembly_point_test.rs
  git commit -m "docs(prompts): clarify assembly responsibility boundary + add arch constraint test (D8)"
  ```

---

## Task F4：settings 每步重读优化（对应 D11）

**问题根源：** `TauriLegacyTurnExecutor::run_llm_step`（`transport/tauri_commands/chat.rs` L148-165）：

```rust
// --- Load settings from DB ---
let settings: AppSettings = {
    let settings_map = self.services.db.get_all_settings().unwrap_or_default();
    let mut s = if settings_map.is_empty() { AppSettings::default() } else { AppSettings::from_string_map(&settings_map) };
    if let Some(ss) = self.services.crypto.as_ref() {
        s.primary_api_key = decrypt_api_key(ss, &s.primary_api_key);
        // ...
    }
    s
};
```

`run_llm_step` 是 agent loop 的核心，每次 LLM iteration（最多 30 次）都执行一次完整的 DB 读+解密。`AppSettings` 在单次 turn 期间不会变化。

**修复目标：** 在 `TurnConfig` 中新增 `resolved_api_key: String`（或 `resolved_settings: ResolvedSettings` 结构体），在 turn 入口（`run_chat_turn_s4` 或等价的 turn 构建处）读取一次并存入 config，`run_llm_step` 从 `input` 或 config 取，不再重复读 DB。

**注意：** 当前 `TurnConfig` 在 `runtime/chat/turn_config.rs`，而 settings 加载在 `transport/tauri_commands/chat.rs`。为避免将 `transport` 层的 `AppSettings` 漏入 `runtime` 层（违反架构约束），使用简单的原始类型存储解密后的 key：`resolved_primary_api_key: String`。

**测试文件：** `src-tauri/tests/review_settings_single_read_per_turn_test.rs`

### 步骤

- [ ] **F4-1 写失败测试**

  在 `src-tauri/tests/review_settings_single_read_per_turn_test.rs` 验证 `LlmStepInput` 包含解密后的 API key，而不依赖 executor 自己读取：

  ```rust
  use lotus_app::runtime::chat::turn_config::LlmStepInput;

  #[test]
  fn review_llm_step_input_carries_resolved_api_key() {
      // LlmStepInput 必须有 primary_api_key 字段（&str），
      // 表明 settings 已在 turn 入口读取并传入，而非每步重读
      // 若字段不存在，此测试编译失败
      let _check: fn(&LlmStepInput) -> &str = |input| input.primary_api_key;
      assert!(true, "LlmStepInput.primary_api_key field exists");
  }
  ```

  运行确认编译失败（field 尚不存在）：
  ```bash
  cd src-tauri && cargo test review_settings_single_read -- --nocapture 2>&1 | head -20
  ```

- [ ] **F4-2 在 `LlmStepInput` 添加 `primary_api_key` 字段**

  在 `src-tauri/src/runtime/chat/turn_config.rs` 的 `LlmStepInput` struct 中新增：

  ```rust
  pub struct LlmStepInput<'a> {
      // ... existing fields ...
      /// 解密后的主 API key，由 turn 入口一次性读取并传入。
      /// executor 使用此字段，不再每步重读 DB。
      pub primary_api_key: &'a str,
  }
  ```

- [ ] **F4-3 在 turn 入口读取 settings 并传入**

  在 `transport/tauri_commands/chat.rs` 的 `TauriLegacyTurnExecutor` 中：

  1. 新增辅助结构（仅限 transport 层内部）：
     ```rust
     struct ResolvedSettings {
         primary_api_key: String,
         tavily_api_key: String,
         bocha_api_key: String,
     }

     impl TauriLegacyTurnExecutor {
         fn load_and_decrypt_settings(&self) -> ResolvedSettings {
             let settings_map = self.services.db.get_all_settings().unwrap_or_default();
             let mut s = if settings_map.is_empty() {
                 AppSettings::default()
             } else {
                 AppSettings::from_string_map(&settings_map)
             };
             if let Some(ss) = self.services.crypto.as_ref() {
                 s.primary_api_key = decrypt_api_key(ss, &s.primary_api_key);
                 s.tavily_api_key = decrypt_api_key(ss, &s.tavily_api_key);
                 s.bocha_api_key = decrypt_api_key(ss, &s.bocha_api_key);
             }
             ResolvedSettings {
                 primary_api_key: s.primary_api_key,
                 tavily_api_key: s.tavily_api_key,
                 bocha_api_key: s.bocha_api_key,
             }
         }
     }
     ```

  2. 在 `run_chat_turn_s4`（或构建 `TurnConfig` 的位置）调用一次 `load_and_decrypt_settings()`，将结果存入局部变量，然后通过 `LlmStepInput` 传给 `run_llm_step`。

  3. `run_llm_step` 中删除 L148-165 的 settings 读取块，改为 `input.primary_api_key`。

- [ ] **F4-4 验证测试通过，且 `run_llm_step` 不再有 `get_all_settings` 调用**

  ```bash
  cd src-tauri && cargo test review_settings_single_read -- --nocapture
  # 验证 run_llm_step 内不再有 get_all_settings
  grep -n "get_all_settings" src-tauri/src/transport/tauri_commands/chat.rs
  # 应只在 load_and_decrypt_settings helper 中出现，不在 run_llm_step 内
  ```

- [ ] **F4-5 运行回归测试**

  ```bash
  cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
  cd src-tauri && cargo test s4_driver -- --nocapture
  ```

- [ ] **F4-6 commit**

  ```bash
  git add src-tauri/src/runtime/chat/turn_config.rs \
          src-tauri/src/transport/tauri_commands/chat.rs \
          src-tauri/tests/review_settings_single_read_per_turn_test.rs
  git commit -m "perf(chat): settings 每 turn 读取一次 — 消除 run_llm_step 每步重读 DB (D11)"
  ```

---

## Task F5：chatStore 拆分（对应前端 F1）

**问题根源：** `src/stores/chatStore.ts` 330 行，混管两类关注点：
1. **会话/消息 CRUD**：`conversations`、`activeConversationId`、`messages`、`setConversations`、`setActiveConversation`、`setMessages`、`addMessage`、`updateMessage`
2. **流式/工具执行状态**：`streamStates`、`busyConversations`、`taskStates`、所有 per-conv streaming actions

**修复目标：**
- 新建 `src/stores/sessionStore.ts`：仅包含会话/消息 CRUD 状态和 actions
- 新建 `src/stores/streamingStore.ts`：仅包含流式状态、busy 状态、task 状态和对应 actions
- `src/stores/chatStore.ts` 变为薄的组合层，re-export 两个 store 的全部内容，向后兼容（现有消费方无需修改）
- `deriveLegacy` 移入 `streamingStore.ts`

**测试文件：** `src/stores/sessionStore.test.ts`、`src/stores/streamingStore.test.ts`

### 步骤

- [ ] **F5-1 写失败测试**

  在 `src/stores/sessionStore.test.ts` 写测试，验证 `useSessionStore` 的 CRUD 行为：

  ```typescript
  import { act, renderHook } from '@testing-library/react'
  import { useSessionStore } from './sessionStore'

  describe('useSessionStore', () => {
    it('setConversations updates conversations list', () => {
      const { result } = renderHook(() => useSessionStore())
      act(() => result.current.setConversations([{ id: 'c1', title: 'Test' } as any]))
      expect(result.current.conversations).toHaveLength(1)
    })

    it('addMessage appends message to list', () => {
      const { result } = renderHook(() => useSessionStore())
      act(() => result.current.addMessage({ id: 'm1', role: 'user', content: 'hi' } as any))
      expect(result.current.messages).toHaveLength(1)
    })

    it('updateMessage patches existing message', () => {
      const { result } = renderHook(() => useSessionStore())
      act(() => result.current.addMessage({ id: 'm1', role: 'user', content: 'hi' } as any))
      act(() => result.current.updateMessage('m1', { content: 'updated' }))
      expect(result.current.messages[0].content).toBe('updated')
    })
  })
  ```

  在 `src/stores/streamingStore.test.ts` 写测试：

  ```typescript
  import { act, renderHook } from '@testing-library/react'
  import { useStreamingStore } from './streamingStore'

  describe('useStreamingStore', () => {
    it('setConversationStreaming sets per-conv isStreaming', () => {
      const { result } = renderHook(() => useStreamingStore())
      act(() => result.current.setConversationStreaming('c1', true))
      expect(result.current.streamStates['c1']?.isStreaming).toBe(true)
    })

    it('appendConversationStreamingContent accumulates delta', () => {
      const { result } = renderHook(() => useStreamingStore())
      act(() => result.current.appendConversationStreamingContent('c1', 'hello'))
      act(() => result.current.appendConversationStreamingContent('c1', ' world'))
      expect(result.current.streamStates['c1']?.streamingContent).toBe('hello world')
    })

    it('clearConversationStreamState resets streaming but preserves toolExecutions', () => {
      const { result } = renderHook(() => useStreamingStore())
      act(() => {
        result.current.setConversationStreaming('c1', true)
        result.current.addConversationToolExecution('c1', { toolName: 't1', toolId: 'id1', status: 'executing' })
        result.current.clearConversationStreamState('c1')
      })
      expect(result.current.streamStates['c1']?.isStreaming).toBe(false)
      expect(result.current.streamStates['c1']?.toolExecutions).toHaveLength(1)
    })
  })
  ```

  运行确认失败（文件不存在）：
  ```bash
  pnpm exec vitest run src/stores/sessionStore.test.ts src/stores/streamingStore.test.ts 2>&1 | tail -20
  ```

- [ ] **F5-2 创建 `src/stores/sessionStore.ts`**

  ```typescript
  /**
   * sessionStore — 会话列表与消息 CRUD 状态。
   * 职责：conversations、messages、activeConversationId 的增删改查。
   * 不包含流式状态（见 streamingStore）。
   */
  import { create } from 'zustand'
  import type { Conversation, Message } from '@/types/message'

  interface SessionState {
    conversations: Conversation[]
    activeConversationId: string | null
    messages: Message[]
    setConversations: (conversations: Conversation[]) => void
    setActiveConversation: (id: string | null) => void
    setMessages: (messages: Message[]) => void
    addMessage: (message: Message) => void
    updateMessage: (id: string, updates: Partial<Message>) => void
  }

  export const useSessionStore = create<SessionState>((set) => ({
    conversations: [],
    activeConversationId: null,
    messages: [],
    setConversations: (conversations) => set({ conversations }),
    setActiveConversation: (id) => set({ activeConversationId: id }),
    setMessages: (messages) => set({ messages }),
    addMessage: (message) => set((s) => ({ messages: [...s.messages, message] })),
    updateMessage: (id, updates) =>
      set((s) => ({
        messages: s.messages.map((m) => (m.id === id ? { ...m, ...updates } : m)),
      })),
  }))
  ```

- [ ] **F5-3 创建 `src/stores/streamingStore.ts`**

  将 `chatStore.ts` 中的 streaming/busy/task 相关状态和 actions 移入此文件。保留 `deriveLegacy` 逻辑（用于为 `chatStore.ts` 向后兼容层派生 `isStreaming`、`streamingContent`、`toolExecutions` 字段）。

  核心 state：`streamStates`、`busyConversations`、`taskStates`
  核心 actions：所有 `setConversationStreaming`、`appendConversationStreamingContent`、`clearConversationStreamState` 等 per-conv actions，以及 `addBusyConversation`、`removeBusyConversation` 等。

- [ ] **F5-4 重构 `src/stores/chatStore.ts` 为薄组合层**

  `chatStore.ts` 改为从两个 store 导入并组合，保留所有原有导出（向后兼容）：

  ```typescript
  // chatStore.ts — 向后兼容的组合层
  // 新代码应直接使用 sessionStore 或 streamingStore
  export { useSessionStore } from './sessionStore'
  export { useStreamingStore } from './streamingStore'

  // useChatStore 保持向后兼容：组合两个 store 的状态
  export const useChatStore = {
    getState: () => ({
      ...useSessionStore.getState(),
      ...useStreamingStore.getState(),
    }),
    // ... 向后兼容实现
  }
  ```

  **重要：** `useChatStore` 必须保持向后兼容，`useStreaming.ts` 等现有消费方无需修改。使用 Zustand 的 `create` 组合或简单 re-export 均可，以不破坏现有测试为准。

- [ ] **F5-5 运行所有前端测试验证无破坏**

  ```bash
  pnpm exec vitest run src/stores/sessionStore.test.ts src/stores/streamingStore.test.ts
  pnpm exec vitest run src/stores/chatStore.test.ts
  pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx
  ```

- [ ] **F5-6 commit**

  ```bash
  git add src/stores/sessionStore.ts src/stores/sessionStore.test.ts \
          src/stores/streamingStore.ts src/stores/streamingStore.test.ts \
          src/stores/chatStore.ts
  git commit -m "refactor(stores): 拆分 chatStore → sessionStore + streamingStore，保持向后兼容 (F1)"
  ```

---

## Task F6：useTauriEvent listener 泄露修复（对应前端 F3）

**问题根源：** `src/hooks/useTauriEvent.ts` 当前实现：

```typescript
setup().then((fn) => {
  if (mounted) {
    unlisten = fn
  } else {
    fn()  // ✓ unmount 时清理
  }
})
// reject 路径：setup() 失败时 .then 不执行，unlisten 永远是 undefined
// 但错误被静默吞掉，没有 .catch
```

若 `setup()` reject（Tauri listen 系统错误），错误静默丢失，listener 未注册，也没有 error log。`useStreaming.ts` 中 11 个独立的 `useTauriEvent` 调用，任一注册失败都无法被感知。

**修复目标：**
1. 在 `useTauriEvent.ts` 中添加 `.catch` 处理，至少做 `console.error` 记录
2. 在 `useStreaming.ts` 中，将 11 个独立 `useTauriEvent` 改为单个批量注册的 `useTauriEvent`，用 `Promise.all` 等待所有 listener 就绪，任一失败时整体 error boundary 报告

**测试文件：** `src/hooks/useTauriEvent.test.ts`

### 步骤

- [ ] **F6-1 写失败测试**

  在 `src/hooks/useTauriEvent.test.ts` 验证：

  ```typescript
  import { renderHook } from '@testing-library/react'
  import { useTauriEvent } from './useTauriEvent'

  describe('useTauriEvent', () => {
    it('does not throw when setup resolves normally', async () => {
      const unlisten = vi.fn()
      const setup = vi.fn().mockResolvedValue(unlisten)
      expect(() => renderHook(() => useTauriEvent(setup))).not.toThrow()
    })

    it('calls unlisten on unmount', async () => {
      const unlisten = vi.fn()
      const setup = vi.fn().mockResolvedValue(unlisten)
      const { unmount } = renderHook(() => useTauriEvent(setup))
      await vi.runAllTimersAsync()
      unmount()
      expect(unlisten).toHaveBeenCalledTimes(1)
    })

    it('logs error and does not throw when setup rejects', async () => {
      const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {})
      const setup = vi.fn().mockRejectedValue(new Error('Tauri listen failed'))
      expect(() => renderHook(() => useTauriEvent(setup))).not.toThrow()
      await vi.runAllTimersAsync()
      expect(consoleError).toHaveBeenCalledWith(
        expect.stringContaining('[useTauriEvent]'),
        expect.any(Error),
      )
      consoleError.mockRestore()
    })
  })
  ```

  运行确认第三个测试失败（当前无 `.catch`）：
  ```bash
  pnpm exec vitest run src/hooks/useTauriEvent.test.ts 2>&1 | tail -20
  ```

- [ ] **F6-2 修复 `useTauriEvent.ts` 添加 `.catch`**

  ```typescript
  export function useTauriEvent(setup: () => Promise<() => void>) {
    useEffect(() => {
      let unlisten: (() => void) | undefined
      let mounted = true

      setup()
        .then((fn) => {
          if (mounted) {
            unlisten = fn
          } else {
            fn()
          }
        })
        .catch((err) => {
          console.error('[useTauriEvent] Failed to register Tauri event listener:', err)
        })

      return () => {
        mounted = false
        unlisten?.()
      }
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
  }
  ```

- [ ] **F6-3 在 `useStreaming.ts` 中添加整体 error boundary**

  `useStreaming.ts` 中 11 个 `useTauriEvent` 调用无法统一为单个 `useTauriEvent`（每个 listener 的回调不同，且 Tauri 的 unlisten 需要分别管理）。合理的修复是：保持 11 个 `useTauriEvent` 调用，但新增一个统一的错误上报机制。

  更实际的改进：在 `useStreaming` 顶部增加一个 `useEffect`，用 `Promise.allSettled` 验证所有 listener 能正常注册（以测试 mock 的方式），并在生产环境中依赖 `useTauriEvent` 的 `.catch` 记录。

  **实际修复步骤：**
  1. 保持 `useTauriEvent.ts` 的修复（已在 F6-2 完成）
  2. 在 `useStreaming.ts` 的每个 `useTauriEvent` 调用的 setup 函数中，确保内部的 Tauri listen 错误被 `useTauriEvent` 的 catch 捕获（通过 `throw` 或 Promise reject 传播）
  3. 验证 Tauri 的 `listen()` 返回的 Promise 在失败时确实 reject（查阅 `@tauri-apps/api/event` 的类型定义确认）

- [ ] **F6-4 验证测试通过**

  ```bash
  pnpm exec vitest run src/hooks/useTauriEvent.test.ts
  pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx
  ```

- [ ] **F6-5 commit**

  ```bash
  git add src/hooks/useTauriEvent.ts src/hooks/useTauriEvent.test.ts
  git commit -m "fix(hooks): useTauriEvent 添加 .catch — 防止 listener 注册失败静默丢失 (F3)"
  ```

---

## Task F7：SessionId newtype 关键路径推广（对应 R2）

**问题根源：** `runtime/ids.rs` 已定义 `SessionId`、`RunId`、`AgentId`、`ToolCallId`，但以下核心结构体仍使用裸 `String`：

| 结构体 | 字段 | 文件 |
|--------|------|------|
| `ChatTurnRequest` | `conversation_id: String` | `runtime/chat/chat_turn_driver.rs:22` |
| `TurnConfig` | `conversation_id: String`, `run_id: String` | `runtime/chat/turn_config.rs:22,23` |
| `LlmStepInput<'a>` | `conversation_id: &'a str`, `run_id: &'a str` | `runtime/chat/turn_config.rs:69,70` |
| `BrowserDeps` | `conversation_id: String` | `runtime/tools/builtin/browser.rs:48` |
| `DefaultFileOperations` | `conversation_id: String` | `runtime/tools/capability.rs:255` |

**修复范围：** 只替换 `runtime/` 层内部的结构体字段。不替换 `transport/` 层（它们从 Tauri IPC 接收 `String`，边界处做转换）。不替换 `PluginContext`（遗留类型，正在迁移中）。

**修复目标：** 将 `ChatTurnRequest.conversation_id`、`TurnConfig.conversation_id` 替换为 `SessionId`；将 `TurnConfig.run_id` 替换为 `RunId`。`LlmStepInput` 是借用类型，考虑是否替换为 `&'a SessionId` 或保持 `&'a str`（更灵活）。

**测试文件：** `src-tauri/tests/review_session_id_newtype_propagation_test.rs`

### 步骤

- [ ] **F7-1 写失败测试**

  在 `src-tauri/tests/review_session_id_newtype_propagation_test.rs` 验证关键路径使用 `SessionId` 类型：

  ```rust
  use lotus_app::runtime::chat::chat_turn_driver::ChatTurnRequest;
  use lotus_app::runtime::chat::turn_config::TurnConfig;
  use lotus_app::runtime::ids::{SessionId, RunId};

  #[test]
  fn review_chat_turn_request_uses_session_id_type() {
      // 若 conversation_id 是 SessionId 类型，以下代码编译通过
      let req = ChatTurnRequest::new("conv-1", "hello", vec![]);
      let _: &SessionId = &req.conversation_id;
  }

  #[test]
  fn review_turn_config_uses_session_id_and_run_id() {
      // TurnConfig 的 conversation_id 应是 SessionId，run_id 应是 RunId
      // 使用 std::any::type_name 或直接字段访问触发类型检查
      fn assert_session_id(_: &SessionId) {}
      fn assert_run_id(_: &RunId) {}
      // 这些断言通过编译即为通过
  }
  ```

  运行确认编译失败（字段类型不匹配）：
  ```bash
  cd src-tauri && cargo test review_session_id_newtype -- --nocapture 2>&1 | head -30
  ```

- [ ] **F7-2 替换 `ChatTurnRequest.conversation_id` 为 `SessionId`**

  在 `src-tauri/src/runtime/chat/chat_turn_driver.rs`：

  ```rust
  use crate::runtime::ids::{RunId, SessionId};

  pub struct ChatTurnRequest {
      pub conversation_id: SessionId,  // 原为 String
      pub content: String,
      pub file_ids: Vec<String>,
      pub run_id: RunId,
      pub is_analysis: bool,
  }

  impl ChatTurnRequest {
      pub fn new(
          conversation_id: impl Into<SessionId>,  // 接受 String 或 &str
          content: impl Into<String>,
          file_ids: Vec<String>,
      ) -> Self {
          Self {
              conversation_id: conversation_id.into(),
              // ...
          }
      }
  }
  ```

  修复所有调用方（`transport/tauri_commands/chat.rs` 等），边界处加 `.into()` 转换。

- [ ] **F7-3 替换 `TurnConfig.conversation_id` 和 `run_id`**

  在 `src-tauri/src/runtime/chat/turn_config.rs`：

  ```rust
  pub struct TurnConfig {
      // ...
      pub conversation_id: SessionId,  // 原为 String
      pub run_id: RunId,               // 原为 String
  }
  ```

  `LlmStepInput` 的 `conversation_id: &'a str` 和 `run_id: &'a str` 保持不变（`&'a str` 作为通用借用类型，调用方从 `SessionId::as_str()` 和 `RunId::as_str()` 取值）。这保持了 executor 接口的简洁性，不需要引入生命周期约束的 `&'a SessionId`。

- [ ] **F7-4 编译验证所有使用方正确**

  ```bash
  cd src-tauri && cargo build 2>&1 | grep -E "error|warning: unused" | head -30
  ```

- [ ] **F7-5 验证测试通过，运行回归测试**

  ```bash
  cd src-tauri && cargo test review_session_id_newtype -- --nocapture
  cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
  cd src-tauri && cargo test s4_driver -- --nocapture
  cd src-tauri && cargo test chat_runtime -- --nocapture
  ```

- [ ] **F7-6 commit**

  ```bash
  git add src-tauri/src/runtime/chat/chat_turn_driver.rs \
          src-tauri/src/runtime/chat/turn_config.rs \
          src-tauri/src/transport/tauri_commands/chat.rs \
          src-tauri/tests/review_session_id_newtype_propagation_test.rs
  git commit -m "refactor(ids): ChatTurnRequest + TurnConfig 使用 SessionId/RunId newtype (R2)"
  ```

---

## 完成检查清单

执行完所有 task 后，运行完整验证：

```bash
# Rust 全部测试
cd src-tauri && cargo test 2>&1 | tail -20

# Rust review_ 系列回归（架构约束）
cd src-tauri && cargo test review_ --tests --no-fail-fast

# 前端单测
pnpm exec vitest run

# 前端关键集成测试
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts

# 类型检查
pnpm build 2>&1 | tail -20
```

## 依赖关系与执行顺序

所有 7 个 task 完全独立，无相互依赖，可并行执行或按任意顺序执行。推荐执行顺序（按难度和风险从低到高）：

```
F3（注释 + 约束测试，最安全）
→ F6（小改动，降低泄露风险）
→ F1（新增校验方法，无破坏性）
→ F2（逻辑提取，有充分测试保护）
→ F4（添加字段，中等改动量）
→ F5（前端拆分，需要细心处理向后兼容）
→ F7（类型替换，影响面最广）
```

## 关键约束提醒

1. **F2** 的 `check_scope_capability` 提取：`StorePolicyPipeline` 对 unknown scope 的行为（`Ask`）与 `CapabilityPermissionPipeline`（`Deny`）不同，这是设计意图，不应统一。共享函数返回 `Deny`，`StorePolicyPipeline` 调用后检测 `UnknownScope` reason 并升级为 `Ask`。

2. **F4** 的 `ResolvedSettings` 结构体必须仅限于 `transport/` 层内部（`pub(super)` 或模块私有），不能将 `AppSettings` 漏入 `runtime/` 层，否则违反架构约束（`runtime/` 不能 `use tauri::*`，也不应依赖 transport 层的 settings 类型）。

3. **F5** 的 `chatStore.ts` 重构必须保证 `useChatStore.getState()` 返回的对象接口不变，因为 `useStreaming.ts` 直接调用 `useChatStore.getState().appendConversationStreamingContent()` 等方法。

4. **F7** 中 `LlmStepInput.conversation_id` 保持 `&'a str` 类型，不替换为 `&'a SessionId`。原因：`LlmStepInput` 是跨层接口（`RuntimeLlmExecutor` trait 的参数），保持 `&str` 避免 executor 实现依赖 `runtime::ids` 模块。边界处 `TurnConfig.conversation_id.as_str()` 提供转换。
