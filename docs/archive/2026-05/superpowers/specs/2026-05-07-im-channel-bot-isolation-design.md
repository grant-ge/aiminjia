# IM 频道：机器人隔离 + 会话 hydrate 修复（切换式）

- 日期：2026-05-07
- 范围：钉钉 IM 频道（已接入），不涉及飞书 / 微信 / 企业微信
- 状态：未上线，老数据可丢

## 背景

当前 IM 频道（钉钉）有两个独立但相关的问题：

1. **侧边栏首次进入"频道"看不到老对话**。`ChannelManager.conversations` 是纯内存 `Vec`，启动时初始化为空，只有 stream worker 收到一条新消息才会 push。前端 `loadConversations` 又只在 `ChannelPage` mount 时触发。结果是：重启 App 后，必须先点进"频道"页面或者收到一条新消息，侧边栏才有内容。
2. **换机器人后对话串号**。`ChannelSessionRouter` 的 key 仅 `group:{cid}` / `private:{userId}`，不带机器人维度。配 A 机器人 → 跟用户聊天 → 移除 A → 配 B 机器人 → 同一用户从 B 发消息 → router 命中老 key → 复用 A 时期的 `session_id` → 新消息接到旧对话里。

本设计同时修复这两个问题。"切换式"指同一时刻只有一个钉钉机器人在线（与现有 UI 单实例配置一致）；老机器人的对话以"历史会话"形态折叠在侧边栏，可读不可写。

## 非目标

- 不支持多机器人并存（同时连多个钉钉应用）
- 不做任何老数据迁移：未上线，启动检测到 v1 sessions.json 时直接清掉所有相关 conversation 目录
- 不动飞书 / 微信 / 企业微信占位实现
- 不改 `ChannelConfig`、`ChannelConfigDetails`、扫码 / manual 注册流程

## 数据模型

### sessions.json（schema_version 2）

```json
{
  "schemaVersion": 2,
  "sessions": {
    "group:{robotCode}:{openConversationId}": "{sessionId}",
    "private:{robotCode}:{userId}": "{sessionId}"
  }
}
```

key 格式从 `group:{cid}` / `private:{userId}` 升级为带 `robotCode` 维度。

### `ChannelConversation`（`connector/channel/types.rs`）

```rust
pub struct ChannelConversation {
    pub session_id: String,
    pub platform: Platform,
    pub conversation_type: ConversationType,
    pub external_id: String,
    pub display_name: String,
    pub unread_count: u32,
    pub robot_code: String,        // 新增
    pub is_active_robot: bool,     // 新增（== 当前 config.bot.robot_code）
}
```

前端 TS 类型同步加 `robotCode: string` + `isActiveRobot: boolean`。

### 机器人身份 key

用 `DingtalkStoredBot.robot_code`（无论 source 是 `Registration` 还是 `AppKeyFallback`）。原因：
- 它是当前系统里"机器人身份"的事实字段（`reply_manager` / 卡片回复都用它定位）
- `AppKeyFallback` 场景下值等于 `app_key`，但 router key 加了 `robot_code:` 前缀依然能正确隔离不同应用
- 钉钉如果未来给更精确的机器人 ID，迁移面更小

## 后端改动

### `connector/channel/router.rs`

**结构**

```rust
struct SessionsState {
    schema_version: u32,
    sessions: HashMap<String, String>,
}
```

**API 变化**

- `make_key(conv_type, robot_code, external_id)` — 新增 `robot_code` 参数，生成 `{group|private}:{robot_code}:{external_id}`
- `get_or_create_session(conv_type, robot_code, external_id, create_session)` — 签名增加 `robot_code: &str`
- `entries() -> Vec<RouterEntry>` — 新增；返回 `(conversation_type, robot_code, external_id, session_id)` 元组列表，给 hydrate 用
- `migrate_or_load(path, conversation_store) -> Result<Self>` — 新增；替换原 `load`：
  - 读出 `SessionsState` 后判断 `schema_version != 2`（包括字段缺失）→ 收集所有 session_id → 逐个 `conversation_store.delete(id)`（忽略 NotFound）→ 写入空的 `{schema_version: 2, sessions: {}}` → 返回空 router
  - `schema_version == 2` → 正常返回
- 原 `load` 保留为 internal helper 或删除（无外部调用）

**测试**（扩展现有 mod tests）
- `migrate_or_load_drops_legacy_v1_data`
- `migrate_or_load_preserves_v2_data`
- `key_includes_robot_code`（不同 robot_code 产生不同 key）
- `entries_returns_parsed_tuples`
- 现有 4 个 `get_or_create_session` 测试更新签名

### `connector/channel/types.rs`

`ChannelConversation` 加 `robot_code: String` + `is_active_robot: bool`，`#[serde(rename_all = "camelCase")]` 已有。

### `connector/channel/manager.rs`

**新方法 `hydrate_conversations(&self) -> Result<()>`**

调用时机：`ChannelManager::new` 完成后立即调一次（或在 `lib.rs` `app.manage(Arc<ChannelManager>)` 之后调）。逻辑：

1. `ChannelSessionRouter::migrate_or_load(&self.sessions_path, &self.conversation_store)`（这一步如果是 v1 自动清掉孤儿对话）
2. 读 `config_store.dingtalk_state()`，拿当前 `robot_code`（platform 未配置时为 `None`）
3. `router.entries()` 遍历，每条：
   - `display_name` ← `conversation_store.get(session_id).title`（拿不到时回退到一个占位串，比如 "未知会话"，并打 warn）
   - `is_active_robot` ← `Some(rc) == current_robot_code.as_ref()`
   - `unread_count: 0`（不持久化未读）
4. `*self.conversations.write().await = vec`

**新方法 `refresh_active_robot_flags(&self, current: Option<&str>)`**

遍历内存里的 `conversations`，每条 `is_active_robot = current.map(|rc| rc == &c.robot_code).unwrap_or(false)`。然后 emit `channel:platform-state`（让前端感知到平台态变化）。注意 `conversations` 数组是后端内存改动，前端不会自动同步，所以前端要在收到 `channel:platform-state` 时主动 `loadConversations()` 拉一次（见 §前端 stores/channelStore.ts）。

调用时机：
- `hydrate_conversations` 完成后
- platform 切到 `Connected` 时（在现有 `set_connection_state` 进入 connected 分支处）
- `removePlatform` 完成 config 清空后

**stream worker（约 line 357-460）**

- `migrate_or_load` 替换 `load`
- `get_or_create_session` 调用处加 `current_robot_code` 参数（从 `current_dingtalk_state().bot.robot_code` 取，断流前已经验证过）
- `convs_lock.push(ChannelConversation { ..., robot_code: current_robot_code.clone(), is_active_robot: true })`

**`removePlatform`（约 line 540）**

不再 `clear()`。改成调 `refresh_active_robot_flags(None)`，让所有对话变成 inactive。`reply_manager.clear()` 行为不变。

**测试（`tests/` 集成测试，新增 `channel_hydrate_test.rs`）**
- `hydrate_populates_conversations_from_router`
- `hydrate_marks_only_current_robot_as_active`（router 里有 robot-A 和 robot-B 的会话，config 是 robot-A）
- `remove_platform_keeps_conversations_marks_all_inactive`
- `refresh_active_robot_flags_after_reconnect_same_robot`
- `legacy_v1_sessions_trigger_full_wipe_on_startup`

测试要 mock `AppHandle`。如果仓库里没有现成 fixture，把 hydrate 核心逻辑（router → entries → ChannelConversation 列表）拆成独立纯函数 `build_conversation_snapshot(entries, conv_store, current_robot_code)`，对它做单测；`hydrate_conversations` 自身只做 IO + 调用纯函数 + 写锁。

### `commands/channel.rs`

`channel_get_conversations` 行为不变（直接返回 `manager.get_conversations()`，前端按 `is_active_robot` 分组）。无新命令。

## 前端改动

### `lib/tauri.ts`

`ChannelConversation` TS 类型加 `robotCode: string` + `isActiveRobot: boolean`。

### `stores/channelStore.ts`

两处改动：

1. `initChannelListeners` 末尾加 `await useChannelStore.getState().loadConversations()`。这是问题 1 的最终修复——后端 hydrate 之后前端要拉一次才能显示出来。
2. `onChannelPlatformState` 订阅回调里，除了 `setPlatformState(state)`，再触发一次 `loadConversations()`。原因：后端 `refresh_active_robot_flags` 改了 `is_active_robot` 字段后只 emit `platform-state`，没有专门的 conversations 事件，前端要靠这个回调拉新快照。

### `components/sidebar/AppSidebar.tsx`

按 `isActiveRobot` 拆两组：

```
钉钉                                [已连接]
├─ 姚斌权                           ← active
├─ 钉钉群 abcd1234                  ← active
└─ ▸ 历史会话 (3)                   ← 折叠按钮，默认折叠
   ├─ ding-old-001                  ← 二级分组 = robotCode
   │   ├─ 张三 · 已下线              ← 灰色，可点
   │   └─ 李四 · 已下线
   └─ ding-old-002
       └─ 钉钉群 xyz · 已下线
```

具体：
- 折叠状态用本地 `useState<boolean>`，刷新重置
- 折叠区按 `robotCode` 二级分组，每组标题用 `robotCode`（截断显示，例如前 12 字符 + …）
- 折叠区对话项点击仍然 `selectChannelSession(sessionId)` → 进 ChannelPage 看历史
- 折叠区对话项有 `text-muted-foreground` 灰色样式，与活跃项区分
- "未配置 / 等待新消息" 文案逻辑：
  - platform 未配置 → "未配置，点击右侧设置"
  - platform 已配置 + 活跃对话数 0 → 不显示文案（留白即可，避免和 "等待新消息" 这种 placeholder 拉扯）
  - 活跃对话数 > 0 → 渲染列表
- 折叠区独立判定：`legacyConversations.length > 0` 就显示折叠按钮，与上方活跃区状态无关

颜色样式遵守 CLAUDE.md UI 规范：用 `text-muted-foreground` / `text-sidebar-foreground/60` 等语义变量，不写硬编码 `text-gray-500`。

### `features/channel/ChannelPage.tsx`

当前 `activeSessionId` 对应的 conversation `isActiveRobot === false` 时：
- 输入区 disabled
- 顶部加 banner：「该会话来自已下线的机器人，无法发送新消息」
- 用语义颜色变量（`bg-muted` + `text-muted-foreground` 一类）

判断：从 `channelStore.conversations` 里查 `activeSessionId`，看它的 `isActiveRobot`。

频道概览页（没选 session 时）逻辑不动。

### 测试

- `stores/channelStore.test.ts`：扩展，加 "loadConversations is called by initChannelListeners"
- `components/sidebar/AppSidebar.test.tsx`（不存在则新建）：覆盖 4 个分支
  - 活跃 0 + legacy 0 → 显示 "未配置"
  - 活跃 0 + legacy >0 → 显示 "未配置" + 折叠按钮
  - 活跃 >0 + legacy 0 → 仅活跃列表
  - 活跃 >0 + legacy >0 → 活跃列表 + 折叠按钮
  - 折叠展开后按 robotCode 二级分组
- `features/channel/ChannelPage.test.tsx`：扩展
  - 选中 inactive session → 输入区 disabled + banner
  - 选中 active session → 输入区正常

## 数据流

### 启动

```
ChannelManager::new
  ↓
hydrate_conversations
  ├─ ChannelSessionRouter::migrate_or_load
  │    └─ if v1 → 清掉 conversation 目录 + 重写空 v2
  ├─ 读 config 拿 current_robot_code
  ├─ router.entries() → 查 conv_store 拿 title
  └─ 写 self.conversations + emit platform-state

—————— 前端 ——————

App.tsx mount
  └─ initChannelListeners
       ├─ loadPlatforms
       ├─ loadConversations  ← 新增（修复问题 1）
       └─ 订阅 platform-state / message
```

### 收到新消息

```
DingtalkStream → ChannelMessage
  ↓
router.get_or_create_session(type, current_robot_code, external_id, ...)
  ↓ key = "group:{robot_code}:{cid}"
  ↓
push ChannelConversation { robot_code: current, is_active_robot: true }
  ↓
emit channel:message → 前端 incrementUnread
```

### 移除机器人（UI 主动）

```
removePlatform(Dingtalk)
  ├─ config_store 清 config.json
  ├─ stop stream / reply_manager.clear
  ├─ refresh_active_robot_flags(None)   ← 不 clear
  └─ emit channel:platform-state
       ↓ 前端徽标变 "未配置"
       ↓ 前端 conversations 不 reload，但触发 selector 重新分组
       ↓ 所有对话进折叠区
```

### 重新配置同机器人

```
重新走 ChannelConfig 扫码 / manual → 同 robot_code 写回 config.json
  ↓ config_store.save_dingtalk_config 后
  ↓ 启动 stream，set_connection_state(Connected)
  ↓ refresh_active_robot_flags(Some(new_robot_code))
  ↓ 老 robot_code 对话回到活跃区（is_active_robot 由 false 翻 true）
  ↓ emit channel:platform-state（前端 selector 重算）
```

### 配新机器人

```
（先 removePlatform 清 config / refresh_active_robot_flags(None)）
  ↓
新机器人 connect → set_connection_state(Connected)
  ↓ refresh_active_robot_flags(Some(new_robot_code))
  ↓ 老 robot_code 保持 is_active_robot=false（折叠区）
  ↓ 新机器人活跃区为空，等待第一条消息
```

## 边界情况

| 场景 | 处理 |
|---|---|
| 启动时 platform 未配置（无 robot_code）| `current_robot_code = None`，所有 hydrate 出来的对话 `is_active_robot=false`（全部折叠区）|
| `migrate_or_load` 删 conversation 目录时 NotFound | 忽略，继续 |
| `conversation_store.get(session_id)` 在 hydrate 时返回 None | warn log + display_name 用 "未知会话"，不阻塞 hydrate |
| 同一 robot_code 有数百条历史对话 | 折叠区分组渲染没问题，`AppSidebar` 用 `overflow-auto` 已有 |
| 用户在折叠区点对话进入 ChannelPage 后又收到新消息 | stream worker 用的是 `current_robot_code`，新消息只会落到当前活跃机器人的会话；折叠区那条不会变 unread |
| `is_active_robot=false` 的对话被 `incrementUnread` | stream worker 不会调到它（router key 不同），不会发生 |

## 测试策略汇总

后端：
- `connector/channel/router.rs` 单测（新 + 改）
- `tests/channel_hydrate_test.rs` 集成测试（新建）
- 可选：`tests/review_channel_router_keys_are_robot_scoped.rs`

前端：
- `stores/channelStore.test.ts` 扩展
- `components/sidebar/AppSidebar.test.tsx` 新建
- `features/channel/ChannelPage.test.tsx` 扩展

手测（必须）：
1. 干净启动 → 配机器人 → 发消息 → 侧边栏看到对话
2. 重启 App → 立即点频道 tab → 看到老对话（不需要先进 ChannelPage）
3. 钉钉发消息建对话 → UI 移除机器人 → 对话进折叠区，灰色 → 点开能看历史，输入框 disabled
4. 重新填同 app_key/secret → connect → 折叠区对话回到活跃区
5. 注册另一个机器人 → 老对话留在折叠区 → 新机器人发消息建新对话在活跃区
6. 用现在带 v1 sessions.json 的本地环境启动新代码 → sessions.json 变 v2 空 + conversation 目录被删 + 侧边栏显示"未配置"

## 实施顺序建议

1. router.rs（含迁移）+ 它的单测 → 跑通
2. types.rs 字段扩展 + manager hydrate / refresh + 集成测试 → 跑通
3. 前端 store + selector + 类型同步
4. 前端 sidebar 折叠区 + ChannelPage banner + 前端测试
5. 手测六步全过

## 风险

- 老 sessions.json 迁移会**永久删除** conversation 目录。文档明确这是已接受的行为（未上线，可丢）。代码里保留 `log::info!("[channel] migrating sessions.json v? → v2, dropping N legacy conversations")` 便于事后排查。
- `refresh_active_robot_flags` 的三个时机如果有遗漏，会出现"机器人已 connect 但对话还显示在折叠区"的诡异状态。集成测试覆盖 reconnect + remove + new robot 三条路径。
- 折叠区 robotCode ���断显示如果两个机器人前缀相同，UI 上会看起来重复。可接受，用户能从对话内容区分；后续若需要可以让用户给机器人起 alias，超出本 spec 范围。
