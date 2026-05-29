# 配置分层 + Per-conversation 模型 Override（Plan-AE）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:executing-plans`
> 所有子任务有测试约束，必须先写测试再改实现（TDD）。

**Goal:** 以 `claude-code-best` 的 settings layering 机制为基线，补齐 lotus 的“全局安全配置 + workspace 非敏感覆写 + per-conversation 模型 override”三段式解析链，并明确只有 layering 思路对标，具体会话态 override 属于 lotus 扩展能力。
**Architecture:** AE 主线参考 `claude-code-best/src/utils/settings/constants.ts` 的 source layering 与 `claude-code-best/src/utils/settings/settings.ts::loadSettingsFromDisk()` 的合并思路；lotus 本期不完整复刻 `user/project/local/flag/policy`，仅实现：`config.json + secure storage` 作为全局可信层、`.lotus/settings.json` 作为 workspace 非敏感偏好层、`ConversationMeta.model_override` 作为 resolved settings 之后的会话态覆盖。
**Tech Stack:** Rust, TypeScript/React
**Worktree branch:** pzc
**Test file:** `src-tauri/tests/plan_ae_config_layers_test.rs`

---

## 对标修订（2026-04-19）

- `claude-code-best` 的真实主线是 `user/project/local/flag/policy` 多源合并，不是 `.lotus/settings.json` 两层覆盖；lotus 本期只对齐“分层解析机制”，不追求 source 数量同构。
- `ConversationMeta.model_override` 属于 lotus UX 扩展；文档和实现都要明确它不是对标仓库的原生模型选择机制。
- `.lotus/settings.json` 只能承载非敏感字段，不能引入 `primaryApiKey` / `tavilyApiKey` / `bochaApiKey` / `workspacePath` 等 secret-bearing 或路径控制字段。
- subagent 模型选择不自动继承 `ConversationMeta.model_override`；若未来需要子 agent 单独换模型，必须新增显式 agent 参数，不能复用会话元数据。

---

## 当前状态分析

### 关键文件

| 文件 | 作用 |
|---|---|
| `src-tauri/src/storage/file_store/types.rs` | `ConversationMeta` 定义，当前无 `model_override` 字段 |
| `src-tauri/src/storage/file_store/config.rs` | 全局 `config.json` 读写，flat key-value map |
| `src-tauri/src/models/settings.rs` | `AppSettings` 结构体，当前有 `primary_model`/`primary_api_key` 等字段，但没有 `default_persona` |
| `src-tauri/src/runtime/chat/turn_config.rs` | `TurnConfig`/`ResolvedLlmSettings`，turn 级只读配置 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | `ChatTurnRequest`，发起一次 turn 的请求体 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | `load_llm_settings()` — 从 DB 读全局设置并组装 `ResolvedLlmSettings` |
| `src-tauri/src/storage/file_store/conversations.rs` | `create_conversation` — 写 `conv.json`（`ConversationMeta`） |
| `src/lib/tauri.ts` | `createConversation()` — IPC 封装（无参数，返回 id） |
| `src/hooks/useChat.ts` | 调用 `createConversation()`，前端会话创建入口 |

### 模型读取链路（现状）

```
send_message
  → TauriChatCommandAdapter::load_llm_settings()
      → DB/file get_all_settings() → AppSettings::from_string_map()
      → ResolvedLlmSettings { primary_model, ... }
  → TurnConfig { llm_settings: ResolvedLlmSettings }
  → chat_turn_driver → LLM 调用
```

当前链路没有 per-conversation override 注入点，`model_override` 只能在 `load_llm_settings` 之后叠加一层选择逻辑。

### AppStorage 初始化（现状）

`AppStorage::new(base_dir)` 当前初始化的是应用数据目录（见 `src-tauri/src/lib.rs`），而不是 workspace 目录；workspace 路径由全局 settings 的 `workspacePath` 驱动 `FileManager`。因此 workspace 级配置必须通过“读取全局 settings → 解析 workspacePath → 再加载 `.lotus/settings.json`”的两阶段方式接入。

---

## 任务列表

### AE1 — `ConversationMeta.model_override` 字段 + store 支持

**范围：** `src-tauri/src/storage/file_store/types.rs`、`conversations.rs`

**背景：** `ConversationMeta` 存储于 `conversations/{id}/conv.json`，新增 `model_override: Option<String>`，仅用于记录该对话使用的模型名 override。它不是配置层，不承载 provider、API key、endpoint、workspace 或 persona 以外的任何设置。旧文件反序列化时缺省 `null`，向后兼容。

**实现步骤：**

1. 在 `ConversationMeta` 添加字段：
   ```rust
   #[serde(skip_serializing_if = "Option::is_none")]
   pub model_override: Option<String>,
   ```
2. `create_conversation` 保持现有签名不变，创建时默认 `model_override = None`；通过独立的 `set_conversation_model_override` 完成后续写入，避免扩大调用点改动面。
3. 新增 `get_conversation_model_override(base_dir, id) -> StorageResult<Option<String>>` — 读取 `conv.json` 并返回字段。
4. 新增 `set_conversation_model_override(base_dir, id, model_override: Option<String>) -> StorageResult<()>` — 更新 `conv.json` 单字段。
5. `AppStorage` 暴露同名公共方法，供上层调用。

**测试（`plan_ae_config_layers_test.rs`）：**
- `ae1_model_override_persisted` — create → set_override → get_override 验证读写一致
- `ae1_model_override_default_none` — 旧格式 conv.json（无此字段）反序列化后值为 `None`
- `ae1_model_override_clear` — 设置后再设回 `None`，写文件后 JSON 中不包含该 key

**不做：** 不修改 `ConversationIndexEntry`（index.json 无需存 override）。

---

### AE2 — turn 解析链读取 conversation model override（优先级高于全局）

**范围：** `src-tauri/src/transport/tauri_commands/chat.rs`、`src-tauri/src/runtime/chat/chat_turn_driver.rs`

**背景：** `ResolvedLlmSettings.primary_model` 是一次 turn 最终使用的模型名。会话级 override 应发生在全局/workspace settings 已解析完成之后，再用 `conversation_id` 对应的 `model_override` 覆盖 `primary_model`。这一步属于 lotus 的会话态选择逻辑，不属于 settings layering source。

**实现步骤：**

1. `ResolvedLlmSettings` 无需新增字段，override 在解析阶段直接合并进 `primary_model`。
2. `RuntimeLlmExecutor` 增加 request-aware 的 settings 解析入口（例如 `load_llm_settings_for_turn(request)`），默认实现仍复用旧的 `load_llm_settings()`，避免批量修改测试 executor。
3. `RuntimeChatTurnDriver` 在 Step 1 构建 `TurnConfig` 时，调用 request-aware loader，让生产 executor 能基于 `request.conversation_id` 读取 `ConversationMeta.model_override`。
4. `TauriLegacyTurnExecutor` 在 request-aware loader 中执行：
   - 先读取全局 settings
   - 再按 `workspacePath` 合并 workspace 非敏感设置
   - 最后读取 `storage.get_conversation_model_override(conversation_id)`
   - 若 override 非空，则覆盖 `ResolvedLlmSettings.primary_model`
5. `TurnConfig` 结构保持不变，driver / subagent / tool runtime 只消费最终解析后的 `llm_settings.primary_model`。

**边界：** subagent 默认继承父 turn 已解析出的最终模型；本期不让 subagent 自动读取/传播 `ConversationMeta.model_override`。若未来要给子 agent 单独换模型，必须新增显式 agent 参数，不复用会话元数据。

**测试：**
- `ae2_model_override_applied_to_resolved_settings` — conversation override = `Some("claude")`，全局 / workspace 最终 primary_model = `deepseek-v3`，验证 resolved primary_model == `claude`
- `ae2_no_override_falls_back_to_effective_settings` — `model_override = None`，验证 `primary_model` 保持全局/workspace 合并后的值
- `ae2_empty_override_treated_as_none` — `model_override = Some("")` 应被忽略（视为 None）

---

### AE3 — 前端对话头部模型选择下拉（TypeScript）

**范围：** `src/lib/tauri.ts`、`src/hooks/useChat.ts`、新建 `src/components/chat/ModelOverrideSelector.tsx`

**背景：** 前端需要能在对话级别设置/清除 `model_override`。最小可行：对话头部提供模型下拉，调用新 IPC 命令持久化。该 UI 只编辑 conversation metadata，不写全局 settings，也不触碰 API key。

**IPC 新增（Rust transport 侧）：**

```
get_conversation_model_override(conversation_id: String) -> Result<Option<String>, String>
set_conversation_model_override(conversation_id: String, model: Option<String>) -> Result<(), String>
```

**前端实现步骤：**

1. `src/lib/tauri.ts` 新增：
   ```typescript
   export function getConversationModelOverride(conversationId: string): Promise<string | null>
   export function setConversationModelOverride(conversationId: string, model: string | null): Promise<void>
   ```
2. 新建 `src/components/chat/ModelOverrideSelector.tsx`：
   - Props：`conversationId: string`
   - 下拉选项：`null`（"使用全局设置"）+ 支持的模型列表（与 `SettingsModal` 中已有的 provider 列表保持一致）
   - 初始化时 `getConversationModelOverride(conversationId)` 加载当前值
   - 变更时调用 `setConversationModelOverride(conversationId, model)` 并本地更新状态
3. 在对话界面的适当位置（如 `src/components/chat/` 头部 bar）集成该组件。位置优先级：对话已有 header 区域 > 对话设置侧栏。

**边界：** 下拉仅修改 `model_override`；不会写入 `config.json` / `.lotus/settings.json`，也不会修改 `primaryApiKey` 或 per-provider key cache。

**测试（Vitest）：**
- `ModelOverrideSelector.test.tsx` — 渲染测试（mock IPC）；选择模型后调用 `setConversationModelOverride`；显示"使用全局设置"选项

**不做：** 不在 `createConversation` 时传入 model_override（对话创建时不做预设，仅在对话界面后续修改）。

---

### AE4 — `.lotus/settings.json` 非敏感覆写层 + 与全局 config 合并

**范围：** `src-tauri/src/storage/file_store/mod.rs`（`AppStorage`）、`src-tauri/src/storage/file_store/config.rs`

**背景：** claude-code-best 的做法是 global/project/local 三层；lotus 本期只实现一个受限的 workspace 偏好层：workspace 级 `.lotus/settings.json` 覆盖全局 `config.json` 的**非敏感字段**。workspace 路径仍来自全局 settings 的 `workspacePath`，workspace 文件不能反向控制路径或 secret。

**`.lotus/settings.json` 格式（仅覆盖以下字段）：**
```json
{
  "primaryModel": "claude"
}
```

**实现步骤：**

1. 新增 `src-tauri/src/storage/file_store/workspace_settings.rs`：
   - `WorkspaceSettings` 结构体（当前只包含非敏感 allowlist 字段）：
     ```rust
     #[derive(Debug, Clone, Default, Serialize, Deserialize)]
     #[serde(rename_all = "camelCase")]
     pub struct WorkspaceSettings {
         pub primary_model: Option<String>,
     }
     ```
   - `workspace_settings_path(workspace_dir: &Path) -> PathBuf` — 返回 `{workspace_dir}/.lotus/settings.json`
   - `load_workspace_settings(workspace_dir: &Path) -> WorkspaceSettings` — 文件不存在返回 `Default`，反序列化失败记 warn 并返回 `Default`（静默容错）
   - `save_workspace_settings(workspace_dir: &Path, settings: &WorkspaceSettings) -> StorageResult<()>` 仅允许写 allowlist 字段；本期不提供通用 workspace settings 编辑入口

2. `AppStorage` 新增方法 `get_effective_settings(&self, workspace_path: Option<&Path>) -> HashMap<String, String>`：
   - 先调用 `config::get_all_settings(&self.base_dir)` 得到全局 map
   - 若 `workspace_path` 存在，加载 `WorkspaceSettings`，将非 None 字段写入 map（覆盖全局同名 key）
   - 返回合并后 map

3. `TauriChatCommandAdapter::load_llm_settings` 中替换 `self.services.db.get_all_settings()` 为 `self.services.db.get_effective_settings(Some(&workspace_path))`，其中 `workspace_path` 来自全局 settings 的 `workspacePath` 字段。

   **注意顺序：** 先读全局 settings 拿到 `workspace_path`，再用该路径加载 workspace settings 合并。避免循环：workspace settings 不覆盖 `workspacePath` 本身，也不允许覆盖 `primaryApiKey` / `tavilyApiKey` / `bochaApiKey`。

4. `AppStorage` 的 `base_dir` 是全局存储目录（`~/.renlijia`），workspace 目录可能不同；合并逻辑在 `get_effective_settings` 内做，不修改 `AppStorage` 的 `base_dir`。

**测试：**
- `ae4_workspace_settings_loaded` — workspace dir 下有 `.lotus/settings.json` 设 `primaryModel = claude`，全局设 `primaryModel = deepseek-v3`，`get_effective_settings` 返回 `claude`
- `ae4_workspace_settings_absent` — 无 `.lotus/settings.json`，`get_effective_settings` 返回全局值
- `ae4_workspace_settings_partial_override` — workspace 只设 `primaryModel`，未声明字段仍用全局值
- `ae4_workspace_settings_malformed` — `.lotus/settings.json` JSON 非法，不 panic，静默回退全局值
- `ae4_workspace_settings_ignores_sensitive_keys` — `.lotus/settings.json` 即使出现 `primaryApiKey`，也不会进入 effective settings map

---

### AE5 — review 约束测试

**范围：** `src-tauri/tests/plan_ae_config_layers_test.rs`（review_ 系列）

**背景：** review_ 系列回归测试用于验证架构约束，防止后续修改破坏本期约束。

**约束测试：**

1. `review_ae_conversation_meta_has_model_override_field` — 验证 `ConversationMeta` 结构体含 `model_override: Option<String>`，反序列化旧 JSON（无此字段）不 panic 且值为 None

2. `review_ae_workspace_settings_does_not_expose_workspace_path` — 验证 `WorkspaceSettings` 没有 `workspace_path` 字段（防止循环依赖）

3. `review_ae_model_override_none_does_not_touch_primary_model` — `model_override = None` 时，`ResolvedLlmSettings.primary_model` 严格等于全局 primary_model，不被修改

4. `review_ae_workspace_settings_only_merges_allowed_keys` — `WorkspaceSettings` 只有 allowlist 字段，额外字段不进入 effective settings map

5. `review_ae_workspace_settings_never_override_api_keys` — `.lotus/settings.json` 中即使出现 `primaryApiKey` 等敏感键，也不会影响最终 `ResolvedLlmSettings.primary_api_key`

6. `review_ae_runtime_does_not_import_tauri` — 验证 `src-tauri/src/runtime/` 下所有 `.rs` 文件均不包含 `use tauri::` 字样（现有架构约束，新代码不得破坏）

---

## 依赖关系

```
AE1 ──→ AE2 ──→ AE3
AE4 ──→ AE2（effective settings 进入 request-aware loader）
AE5 ──(depends on AE1, AE2, AE4 完成)──→ 可并行写 stub 后再填充
```

AE1 与 AE4 互相独立，可并行开发。

---

## 优先级与实施顺序

| 顺序 | 任务 | 估时 | 关键路径 |
|---|---|---|---|
| 1 | AE1 | 1h | 是（AE2 依赖） |
| 2 | AE4 | 1.5h | 是（AE2 依赖） |
| 3 | AE2 | 1h | 是（AE3 依赖） |
| 4 | AE3 | 2h | 否 |
| 5 | AE5 | 1h | 最后 |

---

## 边界 / 不做

- 不实现 `.lotus/settings.local.json`（本地忽略层）
- workspace settings 不支持 `workspacePath` 字段（防循环）
- workspace settings 不支持 MCP 配置（MCP 有独立管理器）
- 不修改前端 `settingsStore`（全局设置存储），model override 是对话粒度，不影响全局持久化
- workspace settings 的前端面板（查看/编辑 `.lotus/settings.json`）不在本期，仅实现 Rust 读路径与受限写接口
- `.lotus/settings.json` 不承载任何明文 API key / token / workspacePath / MCP / hook / permission 配置
- 子 agent 不自动读取或传播 `ConversationMeta.model_override`
