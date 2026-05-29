# 会话来源统一 + Workspace 存储清理

**日期**：2026-05-20
**作者**：oayzz + Claude
**状态**：设计稿，待实施

## 背景

桌面端目前有四份"会话相关的元数据"分散在四条不一致的存储链路上：

| 数据 | 现在的位置 | 用途 | 问题 |
|---|---|---|---|
| 首页 task composer 选中的 workspace | `localStorage['aijia-home-workspace']` | 首页 workspace 切换器当前选中项 | 不持久化到文件，多设备 / 多账号丢 UX 偏好 |
| 首页 task composer 最近 workspace 列表 | `localStorage['aijia-home-recent-workspaces']` | 首页切换器下拉的最近选项 | 同上 |
| 会话 ↔ 专家团映射 | `localStorage['aijia-expert-team-registry']` | 点开会话时识别"它属于哪个专家团" | 同上 + LLM 改标题后用户无法识别归属 |
| 会话 ↔ workspace 绑定 | `~/.renlijia/users/{scope}/shared/memory/memory.jsonl`，key = `authorized_workspace:{session_id}` | 后端 runtime 找授权目录 + 侧边栏会话分组 | 借用了本来给"企业记忆"功能准备的 KV 设施（`AppStorage::set_memory` / `get_memory`），每次 `get_memory` 都全量读 jsonl，且 append-only 不 compact，121 条历史 entry 已经存在；KV 设施除了这一处再无生产消费者 |

侧边栏渲染时为了知道每条会话属于哪个 workspace，后端 `get_conversations` 对每条会话循环调一次 `get_memory("authorized_workspace:{conv_id}")` → 每次都把整个 memory.jsonl 全量读一遍 → O(N × M)，其中 N = 会话数、M = memory 文件总行数。这是 index.json 设计本意（"一次读完元数据，避免 fan-out 读每个 conv.json"）的反模式。

会话来源（普通用户对话 / 数字员工派活 / 专家团 / IM 渠道）的表达也不统一：

- 数字员工：`ConversationMeta.employee_id` + mirror 到 `ConversationIndexEntry.employee_id`，已经稳定（这次保留）
- 专家团：localStorage 旁路映射，跟 conv 元数据脱钩
- IM：未来接入时还没有元数据字段
- 每加一种来源都要给 `ConversationMeta` / `ConversationIndexEntry` 加一对 `xxx_id` 字段 + 一个 mirror 同步分支，膨胀

## 目标

1. **首页 UI 偏好持久化**：`selectedWorkspace` + `recentWorkspaces` 从 localStorage 搬到 `~/.renlijia/users/{scope}/config.json`（复用现有 `AppSettings` 链路）。
2. **会话来源统一表达**：`ConversationMeta` 加 tagged union `source`（kind + 各自 id），`ConversationIndexEntry` 加 `kind` + `sourceLabel`。专家团从 localStorage 迁到这里。`employee_id` 字段过渡期保留兼容。
3. **workspace 存储搬家**：会话绑定的 workspace 从 memory.jsonl 搬到 `ConversationMeta.authorizedWorkspace`（完整对象） + `ConversationIndexEntry.workspaceName` mirror。侧边栏从此读 index.json 一次完事，不再 fan-out。
4. **清理 memory KV 设施**：删 `AppStorage::set_memory` / `get_memory` / `get_memories_by_prefix` / `delete_memories_by_prefix` 整套 public API + `file_store/notes.rs` 模块 + `MemoryEntry` 类型 + `FileAuthorizedWorkspaceStore`。
5. **`sourceLabel` 字段**：人类可读副标题（员工 display name / 团名 / IM 平台-对端），LLM 改 title 也改不动，保证用户始终能识别会话来源。

## 非目标

- **不做老数据迁移**。memory.jsonl 现有 121+ 条 `authorized_workspace:*` entry 视作沉默冻结：不读、不写、不删（文件留在磁盘）；localStorage 三个 key 同理。老用户的 workspace 分组 + 专家团归属会丢，进默认文件夹，但应用不崩、所有功能可用，用户重新选一次即恢复。
- **不做 schema 版本化迁移机制**。仓库现有的 `state.json + migrations.{taskName} = true` boolean 打勾模式不适合做链式迁移；本次不引入新机制，但本 spec 的 schema 变化都用 `#[serde(default)]` 兼容（老 conv.json 反序列化时 `source` 默认 `User`、`authorizedWorkspace` 默认 `None`、`sourceLabel` 默认 `None`）。
- **不改 IM 渠道接入**。`ConversationSource::Im` 是占位 variant，本次不写入；amazing-chatelet 上 IM 实际接入时再补字段定义。
- **不改侧边栏 UI / 不加新 tab**。"sidebar tab 改图标 + 加数字员工 tab"是独立 UI spec，不在本次范围。
- **不改 `get_conversations` 的性能修复路径之外的逻辑**。本次只移除"for each conv 调一次 get_memory"那段，改读 index.json 一次。其余 conversation_service 逻辑保持。

## 设计

### §1 数据结构

#### §1.1 `ConversationSource`（新）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConversationSource {
    User,
    Employee { employee_id: String },
    ExpertTeam { expert_team_id: String },
    Im,  // 占位 variant；IM 接入时再补字段（platform / channel_id 等）
}

impl Default for ConversationSource {
    fn default() -> Self { Self::User }
}
```

序列化样例：

```json
{ "kind": "user" }
{ "kind": "employee", "employeeId": "emp-001" }
{ "kind": "expertTeam", "expertTeamId": "marketing-team" }
{ "kind": "im" }
```

未知 variant 兜底：serde 对带 struct variant 的 enum 不直接支持 `#[serde(other)]`。实现上用自定义 `Deserialize`：反序列化失败时（如 `kind` 取值不在已知集合）落到 `User`，配套写一个 `tests/conversation_source_unknown_kind.rs` 锁住行为。这样老版本桌面端打开新版本写入的 conv.json（未来 IM variant 加字段后）仍可读。

#### §1.2 `ConversationMeta`（修改）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_archived: bool,

    // 既有字段：保留过渡期，新写入由 source 表达
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,

    // 新增：会话来源
    #[serde(default)]
    pub source: ConversationSource,

    // 新增：会话当前授权的本地工作目录（轻量 ref：id / rootPath / displayName / authorizedAt，不含 session_id）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_workspace: Option<PersistedAuthorizedWorkspace>,

    // 新增：人类可读副标题，LLM 改 title 不影响这个字段
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,

    // 既有字段（保留）
    #[serde(default, skip_serializing)]
    pub model_override: Option<String>,
}
```

**`employee_id` 字段的过渡处理**：

- 读：dispatch 时 / 顶栏徽章渲染时优先看 `source`：`Employee { employee_id }` 取里面的 id；如果 `source == User` 且 `employee_id` 字段有值 → 仍按数字员工渲染（兼容老数据）。
- 写：dispatch 时同时写 `source = Employee { employee_id }` + `employee_id = Some(id)`（双写过渡期）。
- 后续删：另开 PR，确认所有 employee dispatch 路径都已走 `source` 后再删 `employee_id` 字段。本 spec 不在此 PR。

**新类型 `PersistedAuthorizedWorkspace`**：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedAuthorizedWorkspace {
    pub id: String,
    pub root_path: PathBuf,
    pub display_name: String,
    pub authorized_at: String,
}
```

故意**不含 `session_id`**——session_id 是 runtime 内部 ID 概念，不应该被 disk 格式吃住。`AuthorizedWorkspace`（带 session_id 那个）保留给 runtime 内部用，写盘时手动映射成 `PersistedAuthorizedWorkspace`、读盘后由调用方按需补 session_id。

#### §1.3 `ConversationIndexEntry`（修改）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationIndexEntry {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub created_at: String,
    pub updated_at: String,
    pub is_archived: bool,

    // 既有：mirror 自 ConversationMeta.employee_id（保留过渡期）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employee_id: Option<String>,

    // 新增 mirror：来源 kind（不含 id；点开会话才需要 id）
    #[serde(default = "default_kind")]
    pub kind: ConversationKind,

    // 新增 mirror：人类可读副标题
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,

    // 新增 mirror：会话授权目录的 displayName（用于侧边栏分组）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConversationKind {
    #[default]
    User,
    Employee,
    ExpertTeam,
    Im,
}
```

**index.json 故意不存 id**（无论是 employeeId / expertTeamId / workspaceId）：

- 侧边栏渲染只需要"图标 / 分组 / 标签"——`kind` + `sourceLabel` + `workspaceName` 三个字段就够
- 点开会话时再读 conv.json 才需要具体 id 来跳转 / 反查
- 保持 index.json 轻量（启动一次性加载性能优先）

#### §1.4 `AppSettings`（修改）

在 `src-tauri/src/models/settings.rs` 加两个 String 字段（值是 JSON-stringified 对象，复用 `update_settings` 现有的 KV string 链路）：

```rust
pub struct AppSettings {
    // ... 既有字段 ...

    /// JSON-stringified AuthorizedWorkspaceRef。首页 task composer 当前选中。
    /// 空字符串视为未选中。
    #[serde(default)]
    pub ui_home_selected_workspace: String,

    /// JSON-stringified AuthorizedWorkspaceRef[]。首页切换器最近列表。
    /// 空字符串或 "[]" 视为空列表。**前端限定最多 10 条**：超出时 LRU 截断（新加入的在前，超过 10 截尾）。
    #[serde(default)]
    pub ui_home_recent_workspaces: String,
}
```

为什么用 string + JSON 双层序列化而不是 typed struct：`AppSettings` 的持久化走 `AppStorage::set_setting(key, value)` KV string 链路（见 `commands/settings.rs:96`），所有字段都走 string 转换。引入第一个 typed 复杂字段会要求改 `update_settings` / `from_string_map` 两套链路。本次保持现有约定（chat_width_mode / font_scale 等也是用 string 表达枚举）。前端 hydrate 时拿到字符串后 `JSON.parse`。

### §2 前端改动

#### §2.1 `src/stores/homeStore.ts`：localStorage → AppSettings

`useHomeStore`：

- 删 `loadFromStorage` / `loadRecentFromStorage` / `readJson` / `writeJson`（不再读 localStorage）
- 初始值：从 React 顶层（`App.tsx`）调一次 `getSettings()` 后调 `hydrateHomeStore(settings)` 注入；store 内部不直接 invoke
- `setSelectedWorkspace` / `removeRecentWorkspace`：
  - 内存改 store 状态
  - **recent 列表更新规则**：新加入项移到列表头，去重，**截尾保留前 10 条**（LRU + cap）。`MAX_RECENT_WORKSPACES = 10` 常量定义在 `homeStore.ts` 顶部
  - 同步调 `updateSettings({ uiHomeSelectedWorkspace, uiHomeRecentWorkspaces })`（fire-and-forget，错只 log）
- 老 localStorage key（`aijia-home-workspace` / `aijia-home-recent-workspaces`）**不主动清**，桌面端代码里彻底删除字符串字面量后它们就是沉默垃圾

#### §2.2 `src/features/expert-teams/expertTeamRegistry.ts`：localStorage → conv.json

整个模块改写：

- 删 `loadFromStorage` / `persist` / `STORAGE_KEY`
- 删模块级 `map` 单例 + `listeners`
- 改造为薄壳，所有读写转发到后端 conv.json：
  - `setExpertTeam(convId, teamId, teamLabel)` → invoke `set_conversation_expert_team(convId, teamId, teamLabel)`，后端写 `ConversationMeta.source = ExpertTeam{...}` + `source_label = Some(teamLabel)` + mirror 到 index.json
  - `getExpertTeamId(convId)` → invoke `get_conversation_source(convId)` 读 conv.json 拿到 `expert_team_id`。**注意**：index.json 故意不存 id，所以这条路径必须读 conv.json，不能从 `useChatStore.conversations` 反查���调用方应是低频路径（点开会话时一次性读取并 cache）
  - `hasExpertTeam(convId)`（新增）→ 从 `useChatStore.conversations` 反查 `kind === 'expertTeam'`，返回 `boolean`。高频渲染场景用这个（侧边栏 chip 等）
  - `clearExpertTeam(convId)` → invoke `clear_conversation_source(convId)`，后端 `source = User` + 清 mirror
  - `useExpertTeamForConversation(convId)`：改为 zustand selector hook，返回 `boolean`（订阅 `useChatStore.conversations`，看 entry `kind === 'expertTeam'`）。**返回类型从原来的 `ExpertTeamId | undefined` 改为 `boolean`**——调用方如果需要具体 teamId 必须调 `getExpertTeamId(convId)` 异步读

老 localStorage key (`aijia-expert-team-registry`) 不主动清。

**调用点审计**：审计所有 `getExpertTeam(...)` / `useExpertTeamForConversation(...)` 调用点：
- 只判断"是不是专家团"的（决定 UI 分组、显示 chip）→ 用 `hasExpertTeam` / hook 返回 boolean
- 真的要 teamId 进而跳转 / 显示团队信息的 → 改为 `await getExpertTeamId(convId)`，配套加 loading 态

#### §2.3 `src/lib/tauri.ts`：新增 IPC wrapper

```ts
export function setConversationExpertTeam(
  conversationId: string,
  expertTeamId: string,
  teamLabel: string,
): Promise<void>
export function clearConversationSource(conversationId: string): Promise<void>
export function getConversationSource(conversationId: string): Promise<ConversationSourceDto | null>
// ConversationSourceDto = { kind: 'user' } | { kind: 'employee', employeeId } | { kind: 'expertTeam', expertTeamId } | { kind: 'im' }
```

`AppSettings` 类型补两个新字段（string）。

#### §2.4 `src/types/message.ts` / 类型层

`Conversation` / index entry 的前端类型补 `kind` / `sourceLabel` / `workspaceName` 字段（mirror 自 `ConversationIndexEntry`）。`source` / `authorizedWorkspace` 等"详细身份"字段不上前端的列表类型，只在"点开会话"那条路径上用。

#### §2.5 删除字符串字面量

代码中 grep `'aijia-expert-team-registry'` / `'aijia-home-workspace'` / `'aijia-home-recent-workspaces'` 三个 key 全部删除。`docs/superpowers/plans/2026-04-24-homepage-workspace-selection.md` 里提到这些 key 的段落更新（不重写历史 plan，加一个 "**已被 2026-05-20 spec 取代**" 的顶部 banner）。

### §3 后端改动

#### §3.1 新增 Tauri 命令

`src-tauri/src/transport/tauri_commands/chat.rs`：

```rust
#[tauri::command]
pub async fn set_conversation_expert_team(
    services: State<'_, TauriChatServices>,
    conversation_id: String,
    expert_team_id: String,
    team_label: String,
) -> Result<(), String>

#[tauri::command]
pub async fn clear_conversation_source(
    services: State<'_, TauriChatServices>,
    conversation_id: String,
) -> Result<(), String>

#[tauri::command]
pub async fn get_conversation_source(
    services: State<'_, TauriChatServices>,
    conversation_id: String,
) -> Result<Option<ConversationSource>, String>
```

实现都委托给一个新 helper：

```rust
// src-tauri/src/storage/file_store/conversations.rs

pub fn set_conversation_source(
    base_dir: &Path,
    conversation_id: &str,
    source: ConversationSource,
    source_label: Option<String>,
) -> Result<()>
```

行为：
1. 读 `conversations/{id}/conv.json` → 修改 `meta.source` + `meta.source_label` → atomic 写回
2. 读 `index.json` → 找对应 entry → 更新 `kind` + `source_label` mirror → atomic 写回
3. 用 `with_state_json_write_lock` 串行化避免 race

`clear_conversation_source` 是 `set_conversation_source` 的特例：`source = User`、`source_label = None`。

#### §3.2 workspace 写路径调整

现有 `commands/workspace.rs::authorize_local_directory`：

- **保留**：仍然写到 `AuthorizedWorkspaceStore`（用作 runtime 工具上下文的 source-of-truth）
- **新增**：写完 `AuthorizedWorkspaceStore` 后，同步把 `AuthorizedWorkspace` 整对象写进 `ConversationMeta.authorized_workspace` + `displayName` mirror 到 `ConversationIndexEntry.workspace_name`

新 helper：

```rust
// src-tauri/src/storage/file_store/conversations.rs
pub fn set_conversation_workspace(
    base_dir: &Path,
    conversation_id: &str,
    workspace: Option<&PersistedAuthorizedWorkspace>,
) -> Result<()>
```

`AuthorizedWorkspaceStore::replace_for_session` 实现负责把 runtime 的 `AuthorizedWorkspace`（带 session_id）映射成 `PersistedAuthorizedWorkspace`（不含 session_id）再写盘。

`revoke_authorized_workspace` 同样的两份写：清 `AuthorizedWorkspaceStore` + 清 conv.json + 清 index.json。

#### §3.3 `AuthorizedWorkspaceStore` 替换为 conv.json 直读

替换 `FileAuthorizedWorkspaceStore`（走 memory.jsonl 那条线）为新的 `ConvJsonAuthorizedWorkspaceStore`。

**前置：改 trait 签名让 conversation_id 显式化**

现 trait 的几个方法都用 `session_id` 寻址，依赖隐式约定"session_id == conversation_id"。新实现里这个等式需要落到具体文件路径上（conv.json 路径用 conversation_id 拼），把这个等式藏在实现里很危险——未来 session_id 和 conversation_id 真要解耦时定位 bug 困难。

**显式修改 trait 签名**：增加 `conversation_id: &str` 参数：

```rust
pub trait AuthorizedWorkspaceStore: Send + Sync {
    fn replace_for_session(
        &self,
        conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()>;
    fn get_current_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>>;
    fn clear_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<()>;
}
```

调用方（`commands/workspace.rs::authorize_local_directory` 等）必须显式传 conversation_id。当前 `lib.rs` 的约定下，调用方仍然把 conversation_id 和 session_id 传成同一个值，但这是**调用方的选择**，不再是 trait 隐式假设。所有现有调用点需要审计并加上这个参数。

实现：

```rust
pub struct ConvJsonAuthorizedWorkspaceStore {
    pub storage: Arc<AppStorage>,
}

impl AuthorizedWorkspaceStore for ConvJsonAuthorizedWorkspaceStore {
    fn replace_for_session(
        &self,
        conversation_id: &str,
        ws: &AuthorizedWorkspace,
    ) -> Result<()> {
        // 映射 AuthorizedWorkspace → PersistedAuthorizedWorkspace（去掉 session_id）
        let persisted = PersistedAuthorizedWorkspace {
            id: ws.id.clone(),
            root_path: ws.root_path.clone(),
            display_name: ws.display_name.clone(),
            authorized_at: ws.authorized_at.clone(),
        };
        set_conversation_workspace(self.storage.base_dir(), conversation_id, Some(&persisted))
    }

    fn get_current_for_session(
        &self,
        conversation_id: &str,
        session_id: &SessionId,
    ) -> Result<Option<AuthorizedWorkspace>> {
        // 读 ConversationMeta.authorized_workspace，回填 session_id 后返回 runtime 形态
        let persisted = read_conversation_workspace(self.storage.base_dir(), conversation_id)?;
        Ok(persisted.map(|p| AuthorizedWorkspace {
            id: p.id,
            session_id: session_id.clone(),
            root_path: p.root_path,
            display_name: p.display_name,
            authorized_at: p.authorized_at,
        }))
    }

    fn clear_for_session(
        &self,
        conversation_id: &str,
        _session_id: &SessionId,
    ) -> Result<()> {
        set_conversation_workspace(self.storage.base_dir(), conversation_id, None)
    }
}
```

`InMemoryAuthorizedWorkspaceStore`（tests 用）也按新签名更新；它内部仍用 `HashMap<String, AuthorizedWorkspace>`，但 key 改为 conversation_id（之前是 session_id.as_str()，语义相同但概念清晰）。

**`load_explicit_workspace` 函数返回值**：保持现有签名 `Option<AuthorizedWorkspaceRef>`（id / rootPath / displayName）。实现里调 `get_current_for_session(conv_id, &SessionId::new(conv_id))`（lib.rs 当前约定下仍然把两者传同一值），从返回的 `AuthorizedWorkspace` 中丢掉 `session_id` 和 `authorized_at` 字段后映射成 `AuthorizedWorkspaceRef`。

#### §3.4 `get_conversations` 性能修复

`transport/tauri_commands/chat.rs::get_conversations`：

```rust
// 删掉这一段
for conv in &mut convs {
    if let Some(ws) = chat_runtime_impl::load_explicit_workspace(&self.services.app, id) {
        conv["workspaceName"] = serde_json::Value::String(ws.display_name);
    }
}
```

`workspace_name` 已经在 index.json 里 mirror 过来了，`conversation_service::get_conversations` 读 index 时就一起带出来，N 次 `get_memory` 全部消失。

`load_explicit_workspace` 函数本身保留（其它地方还有调用），但它的实现改成读 `ConversationMeta.authorized_workspace`，**不再走 memory.jsonl**。

#### §3.5 删除 memory KV 设施

memory KV 设施除了 authorized_workspace 这条线外，其余 caller 经审计全部是 **dead code 链**（A 写 B 读，但 A、B 都没有真正的入口调它）。本节列出**完整删除清单**——既包括底层 API，也包括 dead 上层封装、dead caller、dead 测试。

**底层 KV 设施**（`storage/file_store/`）

- 删 `storage/file_store/notes.rs` 整个文件（含 `memory.jsonl` 读写、shard 切分、`save_note` / `read_note`、所有 `#[cfg(test)]` 测试）
- 删 `MemoryEntry` 类型（`storage/file_store/types.rs` line 166 起）
- 删 `AppStorage` 上的 4 个 pub fn：`set_memory` / `get_memory` / `get_memories_by_prefix` / `delete_memories_by_prefix`（`storage/file_store/mod.rs`）
- 删 `AppStorage::initialize()` 里 `fs::create_dir_all(self.base_dir.join("shared").join("memory"))?;` 那行（`mod.rs:107`）
- 删 `storage/file_store/io.rs` 里仅服务 memory.jsonl 的 shard 切分相关 helper（如 `append_jsonl_with_split` 仅 memory 用就一起删；如果别处也用则保留）

**MemoryStore trait 这条 dead 上层封装**

`runtime/store/memory_store.rs` 定义了 `MemoryStore` trait + `InMemoryMemoryStore` + `MemoryEntry`（同名但跟 `file_store::types::MemoryEntry` 是不同 struct）。`storage/file_store/mod.rs` 里 `FileMemoryStore` 实现这个 trait，`RuntimeRepositoryFacade` 持有 `memory_store: Arc<dyn MemoryStore>` 字段并暴露 `memory_store()` / `clone_memory_store()` 两个公共访问器。**审计结果：没有任何生产代码调用 `RuntimeRepositoryFacade::memory_store()` 或 `clone_memory_store()`**——这是个被装出来但没人用的"插槽"。

- 删 `runtime/store/memory_store.rs` 整个文件
- 删 `runtime/store/mod.rs` 里的 `pub mod memory_store;` + `pub use memory_store::{...}`
- 删 `storage/file_store/mod.rs` 里的 `FileMemoryStore` struct（line 998-1010）
- 删 `RuntimeRepositoryFacade::memory_store` 字段（line 822）
- 删 `RuntimeRepositoryFacade::memory_store()` 和 `clone_memory_store()` 访问器（line 891-897）
- 删 `RuntimeRepositoryFacade::from_storage()` / `default_for_tests()` 等构造里 memory_store 的初始化（line 839 / 860）

**`FileAuthorizedWorkspaceStore` 这条线**

`runtime/store/authorized_workspace_store.rs` 里 `FileAuthorizedWorkspaceStore` 整个 struct 删除——它的功能被 `ConvJsonAuthorizedWorkspaceStore`（§3.3）替代。`AuthorizedWorkspace` / `AuthorizedWorkspaceRef` / `AuthorizedWorkspaceStore` trait / `InMemoryAuthorizedWorkspaceStore` 保留。

**Dead caller 一并清理**

- `runtime/conversation_service.rs:154-155` 的 `db.delete_memories_by_prefix("loaded:...")` 和 `("note:...")` 两行删除。前置已审计：`loaded:*` 和 `note:*` 这两个前缀**没有任何生产代码 set_memory 写入**，只有测试 fixture 写入。这两个 delete 调用一直在删空集合，删掉无功能影响。
- `runtime/tools/capability.rs:317-320` 的 `DefaultFileOperations::is_loaded` 实现里的 `self.storage.get_memory(&key)` 调用。审计结果：`FileOperations::is_loaded` 这个 trait 方法**没有任何调用方**（`capability.rs` 里定义、`impl` 但全仓 0 处调用）。整个 `is_loaded` 方法（trait 定义 + impl）一并删除。
- `plugin/context.rs:141-150` 的 `loaded_key` / `loaded_prefix` / `load_failed_key` 3 个 helper。同样：定义了但 0 处调用。一并删除。

**测试**

- `storage/file_store/mod.rs` 里 line 1642-1652 那段 `set_memory("company", "Acme Corp", ...)` 风格的 unit test 删除（它们是验 KV 设施本身的测试，KV 设施都删了，测试失去意义）
- `tests/user_scope_migration_test.rs` 里 line 13-14 和 line 231-232 的 `shared/memory/memory.jsonl` fixture **保留**——它们测的是 `AiJiaHome::ensure_user_dirs` 创建用户目录时把根目录某些文件迁移过去。memory.jsonl 文件本身在用户目录下不再被任何代码读写，但创建它的目录树这个动作可以保留（无害），把测试 fixture 当回归案例对待
- 但 `AppStorage::initialize()` 里 `create_dir_all("shared/memory")` 删除后，`current_user_storage.rs:180` 和 `aijia_home.rs:254` 这两处仍然显式 mkdir 的位置一并删除（这是用户目录树初始化的另一条路径，跟 AppStorage::initialize 是两条独立调用链）
- 同步删 `aijia_home.rs:441` 和 `current_user_storage.rs:180` 里对 `shared/memory` 目录的断言（如果存在）
- 同步删 `storage/user_scoped_paths.rs:36` 的 `pub fn memory_dir(&self)` helper（如果没人调）和它的测试 `line 201`

**Review 验证**

`tests/review_no_memory_kv.rs` 新增，**仅扫描 `src-tauri/src/`** 下的所有 .rs 文件（**`src-tauri/tests/` 不在扫描范围**——`user_scope_migration_test.rs` 的 fixture 会用到 `"shared/memory"` 字符串构造路径，是允许的；要禁的是生产代码引入这种引用），断言以下字符串 grep 结果为 0：

- `set_memory(` / `get_memory(` / `get_memories_by_prefix(` / `delete_memories_by_prefix(`
- `MemoryEntry`
- `FileMemoryStore` / `MemoryStore` / `InMemoryMemoryStore`
- `FileAuthorizedWorkspaceStore`
- `loaded_key` / `loaded_prefix` / `load_failed_key`
- 字符串字面量 `"memory.jsonl"` / `"shared/memory"` / `"loaded:"` / `"note:"`（带前缀冒号的）

注：这条 review test 跑过一次确认全部清零后即可永久保留，未来如果有人想再引入这种"借用 KV"的反模式，CI 会立刻挂。

**磁盘文件**

`~/.renlijia/{shared,users/*/shared}/memory/memory.jsonl` **不删**。按本 spec 非目标 1，老数据沉默冻结。文件留在那里是僵尸文件，但既然空目录也不再创建，未来新用户 / 重装的用户根本不会出现这个目录。

### §4 启动 hydrate（前端）

`App.tsx` 顶层 effect：

```ts
// 一次性 hydrate AppSettings → homeStore
const settings = await getSettings()
hydrateHomeStore(settings)
```

`hydrateHomeStore`:

```ts
function hydrateHomeStore(settings: Settings) {
  const selected = parseJson<AuthorizedWorkspaceRef>(settings.uiHomeSelectedWorkspace)
  const recent = parseJson<AuthorizedWorkspaceRef[]>(settings.uiHomeRecentWorkspaces) ?? []
  useHomeStore.setState({ selectedWorkspace: selected, recentWorkspaces: recent })
}
```

不需要懒加载——`getSettings()` 已经是启动期必调，`AppSettings` 多两个字段对启动 IO 量影响可忽略。

### §5 错误处理

- 后端写 conv.json / index.json 用现有的 `atomic_write_json`（先写 `.tmp` 再 rename），崩了不会留半文件
- 前端 invoke 失败：toast 提示，store 内存态保留旧值，不强一致
- 反序列化失败 → `#[serde(default)]` 兜底：老 conv.json 没 `source` 字段 → `User`；没 `authorized_workspace` → `None`；没 `source_label` → `None`
- 不认识的 `kind` 字符串 → 通过自定义 `Deserialize`（见 §1.1）兜底为 `User`

### §6 测试

#### Rust 单元 / 集成

- `ConversationSource` 序列化 / 反序列化（4 个 variant + 未知 variant）
- `ConversationMeta` 老格式（无 source 字段）反序列化 → `source = User`
- `ConversationIndexEntry` 老格式（无 kind 字段）反序列化 → `kind = User`
- `set_conversation_source` 双写 conv.json + index.json 一致性
- `set_conversation_workspace` 双写 + revoke 双清
- `ConvJsonAuthorizedWorkspaceStore` trait 测试（用临时目录跑 replace / get / clear，含 `AuthorizedWorkspace` ↔ `PersistedAuthorizedWorkspace` 双向映射）
- review test：见 §3.5 末尾"Review 验证"段——`tests/review_no_memory_kv.rs` 锁住 memory KV 设施所有符号 / 字符串全部从 src 树消失

#### 前端 vitest

- `homeStore` hydrate：`getSettings` 返回带 `uiHomeSelectedWorkspace` → store 状态正确恢复
- `homeStore.setSelectedWorkspace` 触发 `updateSettings`（mock invoke 校验）
- `expertTeamRegistry.setExpertTeam` 触发 `set_conversation_expert_team` invoke
- `useExpertTeamForConversation` selector 在 conversations 更新时正确响应
- 老 localStorage 残留下应用启动正常（不读、不写、不挂）

### §7 实施 PR 拆分

| PR | 内容 | 风险 |
|---|---|---|
| PR1 | 后端：加 `ConversationSource` / `ConversationKind` / `PersistedAuthorizedWorkspace` 类型 + `ConversationMeta` / `ConversationIndexEntry` 新字段（带 `#[serde(default)]`） + 单元测试（含未知 kind 反序列化兜底） | 低，纯数据结构添加 |
| PR2 | 后端：加 `set_conversation_source` / `set_conversation_workspace` helper + 3 个 Tauri commands（set_conversation_expert_team / clear_conversation_source / get_conversation_source）；`ConvJsonAuthorizedWorkspaceStore` 实现并在 lib.rs 启动 wire 处替换 `FileAuthorizedWorkspaceStore` 的注入 | 中，触及 workspace 主链路 |
| PR3 | 后端：`get_conversations` 删 fan-out（不再 for each conv 调 load_explicit_workspace）；`load_explicit_workspace` 改读 conv.json | 中，需要确认 sidebar 分组仍正常 |
| PR4 | 后端：删 §3.5 列出的所有 dead code（memory KV API、dead caller、dead 上层封装、dead 测试）+ 新增 `tests/review_no_memory_kv.rs` 锁层 | 中，删代码量较大但都是 dead code |
| PR5 | 前端：`AppSettings` 两个新字段 + `homeStore` hydrate / persist 改造 + LRU cap 10 | 低 |
| PR6 | 前端：`expertTeamRegistry` 改造（拆 `hasExpertTeam` boolean hook + `getExpertTeamId` async）+ 类型层补 `kind` / `sourceLabel` / `workspaceName` + 所有调用点审计 | 中，触及多个使用 expert team 的组件 |
| PR7 | 文档：更新历史 plan banner + CLAUDE.md（如有相关条目） | 低 |

**PR 依赖图**：

- PR1 独立，先行
- PR2 依赖 PR1（用 PR1 引入的类型）
- PR3 依赖 PR2（用 PR2 写入 conv.json 的 `authorized_workspace` 字段）
- PR4 依赖 PR3（PR3 删 fan-out 把 `load_explicit_workspace` 改成不走 memory KV 后，KV API 才真正零生产 caller，PR4 才能删）
- PR5 独立
- PR6 依赖 PR2（用 PR2 的 3 个 Tauri 命令）
- PR7 独立收尾

**回滚约束**（关键）：

- PR2 + PR3 ship 后**必须捆绑回滚**——PR3 把 `load_explicit_workspace` 改成读 conv.json，依赖 PR2 写入 conv.json。如果 PR2 有 bug 想回滚但 PR3 已 ship，仅回 PR2 会让 `load_explicit_workspace` 读到空 conv.json 字段（PR2 引入的写入路径被回滚但读取路径仍指向 conv.json），workspaceName 全部消失。
- PR4 一旦 ship 后**不可单独回滚**——KV API 被删除，回滚 PR4 要找回那些代码。如果发现 PR4 有问题，前向修复（forward-fix）比反向回滚便宜很多。

**ship 节奏建议**：

- PR1 / PR5 可独立先 ship
- PR2 → PR3 应在同一发版周期一起进，验证后一起发
- PR4 等 PR2/PR3 在生产稳定运行至少一个 minor 版本后再 ship
- PR6 可与 PR2 同时 ship（需要 PR2 的命令先存在）

## 风险

- **老用户体验降级**：所有老会话进默认文件夹 + 失去专家团归属。可接受（按本 spec 非目标 1）。如果用户反馈强烈，再补一个一次性迁移命令（"扫描 memory.jsonl 反向填充到 conv.json"），但本 spec 不做。
- **`employee_id` 双写过渡期**：dispatch 路径要同时写 `source` + `employee_id`，漏写一处会让 employee 顶栏徽章渲染分裂。通过 review test 保证两路径并存时一致性。
- **session_id ≡ conversation_id 的隐式假设**：`AuthorizedWorkspaceStore` 用 `session_id` 寻址，新 `ConvJsonAuthorizedWorkspaceStore` 实现里用 `session_id.as_str()` 当 conversation_id 拼路径。如果未来 session_id 与 conversation_id 解耦，此处需要重做。本 spec 暂沿用现状（`lib.rs` 当前就是这么用的）。
- **index.json schema 变化跨版本**：本 spec 没有 schemaVersion 字段保护——老版本桌面端打开新版本写的 index.json，理论上反序列化时 `#[serde(default)]` 会兜住（未知字段忽略，缺失字段填默认）。但如果以后字段语义改变（不是新增是变形），就需要 schema 版本化机制；这是远期项，本 spec 不引入。

## 后续 / 关联

- **侧边栏 tab 改图标 + 加"数字员工" tab**：本 spec 完成后，侧边栏改为按 `kind` 渲染分组（user / employee / expertTeam / im），UI 调整另开 spec
- **schema 版本化迁移机制**：本 spec 不做。未来在 conv.json / index.json schema 真有 breaking change（不是加字段而是改语义）时再设计 chained `vN→v{N+1}` 迁移
- **删除过渡期 `employee_id` 字段**：本 spec PR 链跑完之后另开 PR，确认所有 dispatch 路径都已经走 `source = Employee{...}` 后再删
- **IM `kind` variant 落地**：amazing-chatelet 上 IM 渠道接入时，扩展 `ConversationSource::Im { platform, channel_id, ... }` 的具体字段
