# Per-Team 子目录化磁盘布局（方案 A）

**Status**: Draft v2
**Date**: 2026-05-14
**Owner**: pzc
**Scope**: 后端 `runtime/agent/team*`、`runtime/tools/builtin/{team_tools, send_message, task_tools, spawn_subagent, task_output}`、`runtime/agent/{output_writer, worker_runtime, lead_idle, name_registry, inbox_registry, cancellation_registry, task_notification_lead}`、`runtime/team_view`、`runtime/session_runtime`、`telemetry`、`storage/file_store/conv_meta`、Tauri commands、前端最小可用 UI。

> v2 修订基于 4 份并行 review（运行态 / 持久化 / 前端 / 横切关注点）合并，新增 §6.5/§6.6/§14（运行态变更清单）/§15（前端契约补充）/§16（out-of-scope 声明）。所有 P0 已就地展开。

## 1. 背景与目标

### 当前现状（实测代码）

一个 conversation 最多一个 Team：

- `runtime/agent/team.rs::TeamRegistry` 用 `HashMap<SessionId, Arc<Mutex<Team>>>` 索引，`create()` 检查 `TeamAlreadyExists`，第二次 `TeamCreate` 直接报错
- 磁盘上 team 相关数据全部平铺在 `<conv_dir>/`：`team.json` / `team-chat.jsonl` / `tasks/` / `teammates/{agent_id}.{jsonl,meta.json}`
- `tasks/` 池被 Lead 和所有 Teammate 共享（`task_tools.rs::store_for` 直接用 `conv_dir.join("tasks")`）

### 目标语义（用户已确认）

1. 一个 conversation 可以同时存在 **多个 team**
2. 每个 team **生于 conv，死于 conv**——TeamDelete 一次性清干净
3. **team_name 仅在当前 conv 内唯一**（不同 conv 可以重名）
4. 每个 team 的 tasks / inter-agent 消息 / teammate transcript 互不交叉

### 非目标

- 跨 conv 复用 team（明确排除）
- team 的全局命名空间（明确排除）
- 重构 subagents/（异步 fire-and-forget 子代理，跟 team 模式正交，本期不动）

---

## 2. 架构总览

### 目标磁盘布局

```
~/.renlijia/users/{scope}/conversations/{conv_id}/
├── conv.json
├── messages.N.jsonl                       # Lead 主对话历史（不变）
├── _current
├── compact_boundaries.jsonl
├── file_index.json
│
├── tasks/                                 # Lead 单飞时的 task 池
│   ├── 1.json
│   └── .highwatermark
│
├── subagents/                             # 与 team 无关（不变）
│   ├── {agent_id}.jsonl
│   └── {agent_id}.meta.json
│
└── teams/                                 # ← 新增 team 命名空间
    ├── {team_name_A}/
    │   ├── config.json                    # 原 team.json
    │   ├── team-chat.jsonl                # 该 team 内部消息
    │   ├── tasks/
    │   │   ├── 1.json
    │   │   └── .highwatermark
    │   └── teammates/
    │       ├── {agent_id}.jsonl
    │       └── {agent_id}.meta.json
    └── {team_name_B}/
        └── ...
```

### 命名规则

- **team_name 强制 ASCII**：必须匹配 `^[a-zA-Z0-9_-]{1,64}$`，TeamCreate 校验不通过直接返回错误（让 LLM 重起一个）
- **目录名 = team_name 本身**：不再做 sanitize/normalize，所见即所得；`ls teams/` 直接看出每个 team 是什么
- **唯一性约束**：同一 conv 内 `team_name` 不重复（大小写敏感比对）

**Why 强制 ASCII**：
- 避免中文/emoji/空格在跨平台（macOS NFC vs NFD、Windows GBK 控制台、git core.quotepath）下的 normalize 不一致
- 避免 `..` `team/alpha` 这类路径穿越
- 避免 `CON` `PRN` 等 Windows 保留名（保留名都是字母，正则不能直接挡，需要单独 deny-list，详见错误处理表）
- 调试友好：目录名 = team_name = LLM/用户嘴里说的那个名字，不需要 lookup 表

**校验拒绝清单**（TeamCreate 校验顺序）：
1. 长度 1–64
2. 正则 `^[a-zA-Z0-9_-]+$`
3. 不在 Windows 保留名集合（大小写不敏感）：`CON PRN AUX NUL COM1..9 LPT1..9`
4. 不是纯 `.` / `..` / 全 `-`

### Lead 归属

- Lead 属于 **他当前创建/进入的 team**
- Lead 调 task tool 时：若 ctx 有 `team_name` → 写进 `teams/{name}/tasks/`；ctx 无 team → 写进 conv 根 `tasks/`
- 一个 Lead 同一时刻只能在一个 team 内"活跃"；进入新 team 不删除老 team，老 team 仍可被 reattach（语义见 §6）

### 关键边界原则

- **conv 根目录**只放"Lead 自己的、与 team 无关的"数据
- **`teams/{name}/`** 是 team 的全部数据，TeamDelete 时 `rm -rf` 该目录
- 任何持久化路径都从一个**单一 dispatcher** 派生（避免分散的 `conv_dir.join("...")`，详见 §3）

---

## 3. 路径派生函数（单一真相源）

新建 `runtime/agent/team_paths.rs`：

```rust
pub struct TeamPaths<'a> {
    conv_dir: &'a Path,
    team_name: Option<&'a str>,
}

impl<'a> TeamPaths<'a> {
    pub fn for_conv(conv_dir: &'a Path) -> Self { ... }
    pub fn for_team(conv_dir: &'a Path, team_name: &'a str) -> Self { ... }

    pub fn team_root(&self) -> Option<PathBuf>;       // teams/{team_name}
    pub fn config_json(&self) -> PathBuf;             // teams/{name}/config.json
    pub fn team_chat_jsonl(&self) -> PathBuf;
    pub fn tasks_dir(&self) -> PathBuf;
    pub fn teammates_dir(&self) -> PathBuf;
    pub fn teammate_transcript(&self, agent_id: &str) -> PathBuf;
    pub fn teammate_meta(&self, agent_id: &str) -> PathBuf;
}

/// 校验 team_name 是否符合命名规则。返回 Ok(()) 或具体错误。
pub fn validate_team_name(raw: &str) -> Result<(), TeamNameError>;

pub enum TeamNameError {
    TooShort,                  // 空串
    TooLong { len: usize },    // > 64
    InvalidChars,              // 含 [^a-zA-Z0-9_-]
    WindowsReserved,           // CON / PRN / ... 保留名
    DegenerateName,            // 全 - / . / ..
}
```

**所有调用方都必须通过 `TeamPaths` 取路径**——禁止再写裸 `conv_dir.join("team.json")` 等。Review 时 grep `team.json` / `team-chat.jsonl` / `teammates/` 字面量应仅出现在 `team_paths.rs` 内。

---

## 4. 运行态数据结构

### `runtime/agent/team.rs::TeamRegistry`

```rust
pub struct TeamRegistry {
    // 外层 SessionId，内层 team_name → Team
    teams: Mutex<HashMap<SessionId, HashMap<String, Arc<Mutex<Team>>>>>,
}

impl TeamRegistry {
    pub async fn create(
        &self,
        session_id: SessionId,
        lead: Member,
        team_name: String,                            // ASCII 校验过的合法名
    ) -> Result<Arc<Mutex<Team>>, TeamError>;

    pub async fn get(
        &self,
        session_id: &SessionId,
        team_name: &str,
    ) -> Option<Arc<Mutex<Team>>>;

    pub async fn list(&self, session_id: &SessionId) -> Vec<(String, Arc<Mutex<Team>>)>;

    /// 删除一个具名 team。**用于 `TeamDelete` 工具**。
    /// 内层 HashMap 中只移除 team_name 这一项；session entry 即使变空也不删除。
    pub async fn delete_team(
        &self,
        session_id: &SessionId,
        team_name: &str,
    ) -> Option<Arc<Mutex<Team>>>;

    /// 删除整个 session 下的所有 team。**用于 `cancel_session` / conv 关闭路径**。
    /// 与 `delete_team` 严格区分：cancel_session 必须用本方法，否则只删一个
    /// team 会让其它 team 的运行态残留。
    pub async fn drop_session(
        &self,
        session_id: &SessionId,
    ) -> Vec<(String, Arc<Mutex<Team>>)>;

    pub async fn persist(
        &self,
        session_id: &SessionId,
        team_name: &str,
        conv_dir: &Path,
    ) -> Result<(), TeamPersistError>;

    pub fn delete_persisted_team(conv_dir: &Path, team_name: &str) -> std::io::Result<()>;

    /// 冷启动 / resume 时从磁盘扫描 `teams/*/config.json` 重建 in-memory map。
    /// **幂等**：已存在的 team 跳过；损坏的 config.json 只 log warn 不阻塞。
    /// 详见 §6.6。
    pub async fn hydrate_from_disk(
        &self,
        session_id: &SessionId,
        conv_dir: &Path,
    ) -> Result<usize, TeamHydrateError>;
}
```

唯一性：`create()` 内查 `inner_map.contains_key(&team_name)`，已存在返回 `TeamError::NameAlreadyTaken`。**create 之前必须先调 `validate_team_name`，校验失败直接返回错误，不进 registry。**

**API 语义对照表**（新增，避免误用）：

| 调用点 | 用哪个方法 | 原因 |
|---|---|---|
| `TeamCreate` 工具 | `create(session, lead, team_name)` | 单 team 添加 |
| `TeamDelete` 工具 | `delete_team(session, team_name)` | 仅删一个 team，其它 team 保留 |
| `cancel_session` / conv 关闭 | `drop_session(session)` | 清空整个 session 下所有 team |
| 工具内查询当前 team | `get(session, team_name)` | 不接受 None — 调用方必须有 active team |
| UI / view 列表 | `list(session)` | 返回所有 team 的不可变快照 |
| 重启 hydration | `hydrate_from_disk(session, conv_dir)` | 由 SessionRuntime 在 conv 打开时调用一次 |

### `Team` 本身

无需改动结构——`team_name` 字段已有，目录名 = team_name 本身，不需要额外的 sanitized 副本。

### `ToolExecutionContext`

`runtime/tools/context.rs` 新增字段：

```rust
pub struct ToolExecutionContext {
    // ...
    pub conv_dir: Option<PathBuf>,        // 已有
    pub active_team_name: Option<String>, // ← 新增，ASCII 校验过的合法名
}
```

**注入点**：`SessionRuntime` 构造 ctx 时按下列优先级推导 `active_team_name`：
1. **Teammate 调 tool**：从 `TeammateWorkerCtx.team_name`（见 §6.5）直接取——这是 Teammate 自己 spawn 时被锁定的 team，不受 Lead active_team 切换影响
2. **Lead 主对话调 tool**（首轮）：从 `conv.json::active_team_name` 读
3. **Lead Path C wake 后的 continuation turn**：见 §6.6 wake 路径专门规则
4. **都没有**：`None`（Lead 单飞场景）

**禁止**用"反查 TeamRegistry.list 找 lead 在哪"的方式推导——Lead 在多个 team 都是 lead，反查会有歧义（v1 错误描述）。

### `TeammateWorkerCtx`

`runtime/agent/worker_runtime.rs::TeammateWorkerCtx` 必须新增 `team_name: String` 字段（spawn 时锁定，永不变更）：

```rust
pub struct TeammateWorkerCtx {
    pub session_id: SessionId,
    pub agent_id: AgentId,
    pub team_name: String,             // ← 新增，spawn 时填入
    pub agent_names: Arc<AgentNameRegistry>,
    pub inbox_registry: Option<Arc<InboxRegistry>>,
    pub cancellation_registry: Option<Arc<CancellationRegistry>>,
    // ...
}
```

`cleanup_teammate` 同步改：

```rust
async fn cleanup_teammate(ctx: &TeammateWorkerCtx, name: &str) {
    // 三元 key 下必须传 team_name 才能定位到正确 entry
    ctx.agent_names.unregister(&ctx.session_id, &ctx.team_name, name).await;
    if let Some(reg) = ctx.inbox_registry.as_ref() {
        reg.unregister(&ctx.session_id, &ctx.team_name, &ctx.agent_id).await;
    }
    if let Some(reg) = ctx.cancellation_registry.as_ref() {
        reg.unregister(&ctx.session_id, &ctx.team_name, &ctx.agent_id).await;
    }
}
```

实现细节见 §7 改造点。

---

## 5. 主要改造点（按文件清单）

| # | 文件 | 改造内容 |
|---|---|---|
| 5-1 | `runtime/agent/team.rs` | `TeamRegistry` 内层加 team_name 维度；新增 `delete_team` / `drop_session` / `hydrate_from_disk` 三个方法；`persist`/`delete_persisted_team` 签名加 `team_name`；`write_atomic_team` 改为调用 `storage::fs_atomic::write_atomic`（已实现 EXDEV fallback，避免 Windows / 跨设备问题）；`create()` 之前调 `validate_team_name` |
| 5-2 | `runtime/agent/team_paths.rs` | **新建**——`TeamPaths` + `validate_team_name` |
| 5-3 | `runtime/agent/team_context.rs` | `render(...)` 接收 team_name，把 prompt 里 `团队配置` 路径替换为 `teams/{name}/config.json`、`tasks/` 替换为 `teams/{name}/tasks/`；**便捷函数 `render_for_conv_dir` 签名改为 `render_for_conv_dir(team_name, agent_name, conv_dir)`，内部用 `TeamPaths::for_team` 派生**；同步更新单测断言；`worker_runtime.rs:1204` 调用点同步 |
| 5-4 | `runtime/agent/output_writer.rs` | `transcript_path_for_kind` / `meta_path_for_kind` 增加 `team_name: &str` 参数（Teammate kind 必传，Subagent kind 仍走 `conv_dir/subagents/`）；`AgentTranscriptMeta::team_id` 字段语义改为 team_name（不再 = session_id）；调用点 `worker_runtime.rs:1185 / 1380 / 1430` 同步 |
| 5-5 | `runtime/tools/context.rs` | 加 `active_team_name: Option<String>` 字段 + `with_active_team` builder |
| 5-6 | `runtime/tools/builtin/team_tools.rs::TeamCreate` | 唯一性校验（`validate_team_name` + `inner_map.contains_key`）；建 `teams/{name}/` 目录；写 `config.json`；`agent_names` / `inbox_registry` 用三元 key `(session, team_name, *)` 注册；conv.json 写入 `active_team_name` |
| 5-7 | `runtime/tools/builtin/team_tools.rs::TeamDelete` | 改用 `TeamRegistry::delete_team(session, team_name)`；磁盘上 `rm -rf teams/{name}/`；从 `agent_names` / `inbox_registry` / `cancellation_registry` 按三元 key 批清；cancel 顺序见 §7.4 |
| 5-8 | `runtime/tools/builtin/team_tools.rs::TeamSwitch` | **新建工具**：单参数 `team_name`，把 `conv.json::active_team_name` 改为目标值；推 RuntimeEvent `TeamActiveChanged`；目标不存在返回错误 |
| 5-9 | `runtime/tools/builtin/send_message.rs::append_team_chat_entry` | 走 `TeamPaths::team_chat_jsonl()`；sender 必有 active team 才能 SendMessage，无则 `ExecutionFailed("SendMessage requires active team")`；`peer-messages` 路由按三元 key resolve |
| 5-10 | `runtime/tools/builtin/task_tools.rs::store_for` | `ctx.active_team_name = Some(name)` 走 `teams/{name}/tasks/`；为 None 走 `conv_dir/tasks/`（Lead 单飞）；`task_notification_lead::emit_to_lead` 签名加 `team_name: &str`，本文件三处调用点从 `ctx.active_team_name` 取 |
| 5-11 | `runtime/tools/builtin/task_output.rs` | `candidates` 列表必须支持新路径：当 ctx 有 `active_team_name` 时优先 `teams/{name}/teammates/{task_id}.jsonl`，再 fallback `conv_dir/teammates/`、`conv_dir/subagents/`、全局 `subagent_transcripts/`。**这是当前 spec v1 漏项，会导致 Teammate 输出读不到** |
| 5-12 | `runtime/tools/builtin/spawn_subagent.rs` | 派 teammate 时把 `team_name` 写进 `TeammateWorkerCtx` 和返回的 transcript meta；`spawn_subagent.rs:522` 行 `team_id: Some(launch_ctx.session_id...)` 改为 `team_id: Some(team_name.clone())`；`team_registry().get(session, team_name)` 精确匹配，不存在返回错误（移除 v1 的 "mismatch but proceed" 容错） |
| 5-13 | `runtime/agent/worker_runtime.rs` | Teammate boot prompt 拼装走 `TeamPaths::for_team`；**`build_teammate_permission_ctx` 把 `additional_working_dirs` 收窄到 `teams/{own_team}/`，禁止整个 `conv_dir` 放行**（安全边界，防 team-A 越权读写 team-B）；`TeammateWorkerCtx` 加 `team_name` 字段；`cleanup_teammate` 三处 `unregister` 改三元 key 调用 |
| 5-14 | `runtime/agent/inbox_registry.rs` | key 从 `(SessionId, AgentId)` 扩为 `(SessionId, TeamName, AgentId)`；新增 `unregister_team(session, team_name)` 批量清理方法 |
| 5-15 | `runtime/agent/name_registry.rs` | 同 5-14——key 从 `(SessionId, Name)` 扩为 `(SessionId, TeamName, Name)`；`name_for(session, team_name, agent_id)` 反查；`unregister_team` 批量方法 |
| 5-16 | `runtime/agent/cancellation_registry.rs` | 同 5-14；新增 `cancel_team(session, team_name)` 一次取消该 team 所有 Teammate |
| 5-17 | `runtime/agent/lead_idle.rs` | `LeadKey` 保持 `(SessionId, AgentId)` 不变；wake_fn 触发的 continuation turn 的 `active_team_name` 推导见 §6.5 |
| 5-18 | `runtime/agent/task_notification_lead.rs` | `emit_to_lead` 签名加 `team_name: &str`；内部 `team_registry.get(session, team_name)` 精确匹配；`inbox_registry.get(session, team_name, lead_id)` 三元 key 投递 |
| 5-19 | `runtime/session_runtime.rs` | `cancel_session` 调 `TeamRegistry::drop_session(session)`（不是 `delete_team`）；conv 打开时调 `hydrate_from_disk` 重建 in-memory team map（见 §6.6）；ctx 注入按 §4 优先级推导 `active_team_name` |
| 5-20 | `storage/file_store/conv_meta.rs`（或对应文件） | `ConversationMeta` 加 `active_team_name: Option<String>`，`#[serde(default)]` 零破坏 |
| 5-21 | `runtime/team_view.rs` | 改为扫 `teams/` listdir 后 per-team 构建 `TeamSession`；`append_events_from_team_chat_jsonl` 签名加 team_name，路径走 `TeamPaths::team_chat_jsonl()`；`load_teammates` 从 `teams/{name}/teammates/` 读 |
| 5-22 | `transport/tauri_commands/...` | 新增 `team_chat_messages(conv_id, team_name)` 和 `team_switch_active(conv_id, team_name)`；`team_overview(conv_id)` 签名不变但返回多 team |
| 5-23 | `telemetry.rs::DiagnosticEvent` | 新增字段 `team_name: Option<String>` + `.team_name(v)` builder；TeamCreate/Delete/Switch + spawn_subagent + lead_idle.* + task_notification.* 全部透传（从 `ctx.active_team_name` 取） |
| 5-24 | 前端 `src/components/teams/` + `src/lib/tauri.ts` + `src/i18n/{zh-CN,en-US}.json` | 团队按钮 + 抽屉 + team-chat 面板 + 4 个 i18n key（详见 §11） |

**禁止新增** `LegacyToolAdapter`、`compat_*` 字段或"兼容模式开关"——一次性完成。

---

## 6. 多 team 共存的运行态语义

### 6.1 Lead 的"active team" 概念

Lead 同一时刻只能在一个 team 内活跃。状态机：

```
Lead 启动 conv → active_team = None（单飞）
  ↓ TeamCreate("alpha")
active_team = "alpha"
  ↓ TeamCreate("beta")
active_team = "beta"          # alpha 还在磁盘上，可 reattach
  ↓ TeamSwitch("alpha")       # 新工具，本期实现
active_team = "alpha"
  ↓ TeamDelete("beta")
active_team = "alpha"（不变）  # 删的不是 active 那个
  ↓ TeamDelete("alpha")
active_team = None
```

**新工具 `TeamSwitch`**：单参数 `team_name`，把 ctx 注入路径上的 active_team 切到目标。如果目标 team 不存在返回错误。

**注**：`active_team` 状态需要在 SessionRuntime 内持久化（重启 conv 后能恢复）。落盘位置：`<conv_dir>/conv.json` 加一个字段 `active_team_name: Option<String>`。

### 6.2 Teammate 的归属

Teammate spawn 时被锁定到一个 team——`AgentTranscriptMeta.team_id = team_name`（直接使用，无 sanitize）。Teammate 永不切换 team。Teammate 调 tool 时 ctx 的 `active_team_name` 由其归属 team 推导，**不是** Lead 的 active team——这样即使 Lead 已经切到别的 team，原来 team 里 idle 的 Teammate 收到消息后仍能正确读写自己 team 的 tasks/。

### 6.3 SendMessage 路由

- 解析 `to: "team-lead"`：解析到 sender 所在 team 的 Lead（Lead 不一定 active 在该 team，但身份仍然是该 team 的 Lead）
- `to: "*"`：sender 所在 team 内广播
- `to: "<peer_name>"`：仅在 sender 所在 team 内 resolve；跨 team 不可达

`AgentNameRegistry` 当前是 `HashMap<SessionId, HashMap<Name, AgentId>>`，需要扩成 `HashMap<SessionId, HashMap<TeamName, HashMap<Name, AgentId>>>`。或者更简单：注册时 name 自动加上 `@team_name` 后缀变全限定。本期选**前者**（嵌套 HashMap），保留 user-facing `to` 字符串干净。

### 6.4 LeadIdle / Inbox

每个 team 的 Lead inbox / lead_idle supervisor 独立。当前 `LeadIdleSupervisor` 用 `(SessionId, AgentId)` 做 key，已经天然按 agent_id 隔离，**LeadKey 不变**——因为同一 Lead 在不同 team 内是同一个 agent_id（Lead 就是主对话 LLM 自己）。

但 inbox 必须 per-team：

`InboxRegistry` key 从 `(SessionId, AgentId)` 改为 `(SessionId, TeamName, AgentId)`。理由：Lead 在 team-alpha 收到的消息和在 team-beta 收到的消息属于不同对话上下文，混进同一 inbox 会让 Lead 看到跨 team 串流。同步改 `AgentNameRegistry` 和 `CancellationRegistry` 三元 key（详见 §5-14 ~ §5-16）。

### 6.5 Path C wake → continuation turn 的 active_team_name 推导

**问题**：当 Teammate 通过 `SendMessage(to: "team-lead")` 把消息送进 Lead 在 team-alpha 的 inbox，Lead 此时可能 active 在 team-beta（或单飞）。`LeadIdleSupervisor::wake_fn` 触发的新 turn，`active_team_name` 该取什么？

**推导规则**（按优先级）：

1. **wake 来源 team 优先**：触发 wake 的 inbox 投递发生在 team-alpha，wake_fn 把 `team_name` 一并传进 callback；continuation turn 的 ctx 用 **wake 来源 team_name**，不是 conv.json 里持久化的 active team
2. wake_fn 的签名扩展：`Fn(LeadKey, team_name: String)`，inbox 投递路径在调用 wake 时把自己的 team_name 透传
3. **continuation turn 不修改 conv.json::active_team_name**——这只是临时跨入响应消息，不是用户/Lead 主动切换；turn 结束后 active_team 回到原值
4. 如果同时有多个 team 都给 Lead 发了消息（极少见），按 inbox 投递时间最早的优先；其它 team 的消息在原 inbox ���队，下一轮 wake 再处理

**对应代码改造**：
- `lead_idle.rs::WakeFn` 签名：`Fn(LeadKey, String) + Send + Sync`
- `inbox.rs` 投递路径在调 `supervisor.enqueue` 时同步携带 team_name
- `chat_turn_driver` wake 入口创建 ctx 时 `active_team_name = Some(wake_team_name)`，**不读 conv.json**

### 6.6 冷启动 / resume 时的 TeamRegistry hydration

`TeamRegistry` 是 process-wide 内存结构，进程重启后为空。当用户重新打开一个有多 team 的 conv，Lead 继续对话时如果不重建 in-memory map，`ctx.active_team_name` 推导链路会全断。

**hydration 触发时机**：`SessionRuntime::open_conversation(conv_id)` 调用时（用户进入会话或 conv 状态被首次访问），同步调一次 `TeamRegistry::hydrate_from_disk(session, conv_dir)`：

```rust
async fn hydrate_from_disk(
    &self,
    session_id: &SessionId,
    conv_dir: &Path,
) -> Result<usize, TeamHydrateError> {
    let teams_root = conv_dir.join("teams");
    if !teams_root.exists() { return Ok(0); }
    let mut count = 0;
    for entry in fs::read_dir(&teams_root)? {
        let entry = entry?;
        let team_dir = entry.path();
        let config = team_dir.join("config.json");
        if !config.exists() { continue; }
        match read_team_snapshot(&config) {
            Ok(snapshot) => {
                // 校验 team_name 与目录名一致；不一致只 log warn 跳过
                if snapshot.team_name != entry.file_name().to_string_lossy() {
                    log::warn!("team_name mismatch in {:?}, skipping", config);
                    continue;
                }
                let team = Team::from_snapshot(session_id.clone(), snapshot);
                self.insert_internal(session_id, team).await;
                count += 1;
            }
            Err(e) => log::warn!("hydrate skip {:?}: {e}", config),
        }
    }
    Ok(count)
}
```

**幂等**：已存在的 team_name 跳过（不覆盖 in-memory state）；损坏的 config.json 只 log warn，不阻塞 conv 打开。

**注**：hydration 重建的是 `TeamRegistry` 内层 `HashMap<TeamName, Arc<Mutex<Team>>>`；`AgentNameRegistry` / `InboxRegistry` / `CancellationRegistry` **不重建**——这些是运行态注册表，重启后所有 Teammate 都已经死了，不需要 register。Teammate 不再自动 resume。重启后用户重新派活才会有新的 Teammate 进入对应 team。

---

## 7. 数据流（端到端示例）

### 7.1 用户在 conv 内创建 team "alpha"

```
1. 用户消息触发 Lead LLM turn
2. Lead 调 TeamCreate(team_name="alpha")
3. team_tools.rs::TeamCreate::execute
   a. ctx.team_registry().create(session, lead, "alpha") → 内层 HashMap 插入 {"alpha": Team}
   b. fs::create_dir_all(TeamPaths::for_team(conv_dir, "alpha").team_root())
   c. TeamRegistry::persist(session, "alpha", conv_dir) → 写 teams/alpha/config.json
   d. 把 active_team_name="alpha" 写进 conv.json
   e. agent_names.register((session, "alpha"), LEAD_NAME, lead_id)
   f. inbox_registry.register((session, "alpha", lead_id), Inbox)
4. 后续同一 Lead turn 内的 tool call ctx 都带 active_team_name="alpha"
```

### 7.2 Lead 在 team alpha 内创建 task

```
1. Lead 调 TaskCreate(subject="x")
2. ctx.active_team_name = Some("alpha")（由 SessionRuntime 推导注入）
3. task_tools.rs::store_for(ctx)
   → TeamPaths::for_team(conv_dir, "alpha").tasks_dir()
   → <conv>/teams/alpha/tasks/
4. FileTaskV2Store::create → <conv>/teams/alpha/tasks/1.json
```

### 7.3 Lead 同时建第二个 team "beta"

```
1. Lead 调 TeamCreate(team_name="beta")
2. 与 7.1 相同流程
3. 完成后：
   - teams/alpha/ 保留（含 config.json / team-chat.jsonl / tasks/ / teammates/）
   - teams/beta/ 新建
   - active_team_name 切到 "beta"
4. team alpha 的 Teammate 们仍在自己 team 内 idle，调 tool 时 ctx.active_team_name="alpha"
```

### 7.4 TeamDelete("alpha")

```
1. Lead 调 TeamDelete(team_name="alpha")
2. team_tools.rs::TeamDelete::execute（严格按下面顺序，不可调换）
   a. cancellation_registry.cancel_team(session, "alpha")
      → 所有 alpha 内 Teammate 的 cancel token 触发
      → 每个 Teammate 的 idle loop 在下一次 tokio::select! 检查到取消，自然退出
      → loop 退出时调 cleanup_teammate（自我清理 agent_names / inbox / cancel entry）
   b. 等待所有 Teammate worker 退出（通过 JoinSet 或带 timeout 的等待）；超时后强制进入 c
   c. ctx.team_registry().delete_team(session, "alpha") → 从内层 HashMap 移除
   d. fs::remove_dir_all(TeamPaths::for_team(conv_dir, "alpha").team_root())
   e. agent_names.unregister_team(session, "alpha") + inbox_registry.unregister_team(session, "alpha")
      （冗余兜底——cleanup_teammate 已经清掉单条，这里是 idempotent 的 sweep）
   f. 若 conv.json::active_team_name == "alpha" 改为 None
```

**为什么必须先 cancel 再 delete**：如果先 `delete_team`，已 active 的 Teammate worker 在下一个 tool call 走 `cleanup_teammate` 时找不到 entry（registry 已空），unregister 静默 no-op；同时 Teammate 还在尝试写 `teams/alpha/teammates/...`（已被 d 步删除的目录），产生 IO 错误。先 cancel 让 worker 自我清理，再 delete 是无脑安全顺序。

**幂等保证**：`cleanup_teammate::unregister` 在三元 key 下找不到 entry 时只返回 `false`，不报错；step e 的 `unregister_team` 同样 idempotent。两次 TeamDelete 同一 team 第二次直接走 noop（§9 错误处理）。

---

## 8. 旧 conv 的处理

**本期不做数据迁移**。决策原因：
- 用户明确不要迁移
- 当前 lotus 处于 v0.x，老 conv 数据量小，可接受破坏性
- 一切迁移逻辑都会增加并发安全、跨设备 fs::rename、半迁移留尸等复杂度

**直接行为**：
- 新装版本对 `<conv>/team.json + team-chat.jsonl + tasks/ + teammates/` 老结构**不主动读取**，team_view 仅扫 `teams/` 子目录
- 旧 conv 上之前创建的 team 在新版本里**视为不存在**——前端列表为空，老的 `team.json` 文件保留在磁盘上但不展示、不影响新 TeamCreate
- 用户在旧 conv 里新调 TeamCreate 会正常工作：直接走新 `teams/{name}/` 路径，跟从未有过 team 的 conv 表现一致
- 文档须在 release notes / 用户公告中说明"升级后旧 team 数据不可见，需要重新组建"

**不需要迁移函数、不需要 LegacyNameNotAscii 错误类型、不需要懒迁移触发点、不需要并发锁保护迁移**——这部分逻辑从 spec 全部移除。

---

## 9. 错误处理

| 场景 | 行为 |
|---|---|
| TeamCreate 时 team_name 不符合 `^[a-zA-Z0-9_-]{1,64}$` | `ToolError::ExecutionFailed("team_name must match ^[a-zA-Z0-9_-]{1,64}$ (got: {raw})")` |
| TeamCreate 时 team_name 是 Windows 保留名 | `ToolError::ExecutionFailed("team_name `{raw}` is a Windows reserved name")` |
| TeamCreate 时 team_name 在 conv 内已存在 | `TeamError::NameAlreadyTaken(name)` |
| TeamDelete 指向不存在的 team | 返回 noop 提示，**不报错**（幂等） |
| TeamSwitch 切到不存在的 team | `ToolError::ExecutionFailed("team `{name}` not found in this conversation")` |
| SendMessage 调用方无 active team | `ToolError::ExecutionFailed("SendMessage requires active team; call TeamCreate first")` |
| task tool 在 active team 下但 `teams/{name}/tasks/` 目录不可创建 | log warn + fallback 到 conv 根 tasks/，确保 task 不丢 |
| TeamSwitch 切换到与当前 active 相同的 team | noop 返回成功（幂等） |
| hydrate_from_disk 读到损坏的 config.json | log warn 跳过该 team，不阻塞 conv 打开 |

---

## 10. 测试策略

### 10.1 单元测试

- `team_paths.rs::TeamPaths`：path 派生函数全枚举（with/without team_name）
- `validate_team_name`：合法名、空、超长、含中文/emoji/空格、Windows 保留名（CON/PRN/com1）、纯 `--`、纯 `..`
- `TeamRegistry`：同 conv 内 unique 校验、跨 conv 不冲突、`list/delete_team/drop_session` 语义对照、`hydrate_from_disk` 幂等性与损坏 config 跳过
- 三元 key 注册表：`AgentNameRegistry` / `InboxRegistry` / `CancellationRegistry` 的 register / unregister / unregister_team / cancel_team
- `task_notification_lead::emit_to_lead` 三元 key 投递

### 10.2 集成测试

新增 `src-tauri/tests/team_multi_team_test.rs`：
- 同 conv 内创建 2 个 team，验证磁盘隔离
- TeamSwitch 后 task tool 落在正确目录
- TeamDelete 一个 team，验证另一个 team 不受影响（在 cancel 顺序保证下 Teammate 工人正确退出）
- SendMessage 跨 team 不可达，team-A 的 Teammate 看不到 team-B 的 peer
- Path C wake：team-alpha 的 Teammate 发消息给 team-lead，验证 continuation turn 的 ctx.active_team_name == "alpha"，不依赖 conv.json 的值
- Teammate permission 边界：team-A 的 Teammate 尝试 read `teams/beta/tasks/1.json` 被 path_auth 拒绝
- 冷启动 hydration：重启 process 后 `TeamRegistry::hydrate_from_disk` 重建出所有 team

### 10.3 review_ 回归

新增 `src-tauri/tests/review_team_paths.rs`：
- grep 整仓 `team.json` / `team-chat.jsonl` 字面量出现位置
- 断言它们仅在 `team_paths.rs` 内（以及测试 fixture）

---

## 11. 前端影响

### 11.1 本期落地：最小可用 UI

后端把 `team_view.rs::build_overview` 升级到多 team 后，前端落一个最小可用 UI：

- **入口**：聊天页右上区域加一个"团队"按钮，气泡角标显示当前 conv 内 team 数量
- **打开后**：抽屉/弹层列出该 conv 的所有 team（按创建时间倒序），每行显示 team_name + teammate 数 + active 标记
- **点击某 team**：右侧或下方面板展示该 team 的 `team-chat.jsonl` 内容（按时间正序、from/to/text 三栏），自动滚到底；新消息事件 push 到来时增量追加
- **active team 切换**：team 行尾有"切换到此"按钮，调用新增 Tauri 命令 `team_switch_active(conv_id, team_name)`，等价于让 Lead 在下一轮调用 `TeamSwitch` 工具
- **本期不做**：手动 TeamCreate/TeamDelete 入口（这两个由 LLM 通过工具决定）、跨 team 全局搜索、teammate 个体 drill-down、token/cost per-team 分桶展示

### 11.2 新增/修改的 Tauri 命令

| 命令 | 签名 | 行为 |
|---|---|---|
| `team_overview(conv_id)` | 已存在，签名不变 | 返回 `TeamOverview { teams: Vec<TeamSession> }`；本期后 vec 真实承载多 team |
| `team_chat_messages(conv_id, team_name)` | **新增** | 读 `teams/{team_name}/team-chat.jsonl` 全文（含分页参数 `since_ts` / `limit`） |
| `team_switch_active(conv_id, team_name)` | **新增** | 把 conv.json 的 `active_team_name` 改为目标值；推 RuntimeEvent `team:active-changed` 触发前端刷新；如果 Lead 正在某个 turn 内，标记为 pending 切换，下一轮 turn 入口生效 |
| `team_create` / `team_delete` | **不暴露** | 仍由 LLM 通过工具触发，前端不直接调 |

### 11.3 新增的前端事件

| 事件 key | 触发时机 | payload |
|---|---|---|
| `team:created` | TeamCreate tool 成功后 | `{ conversationId, teamName }` |
| `team:deleted` | TeamDelete tool 成功后 | `{ conversationId, teamName }` |
| `team:active-changed` | `team_switch_active` 命令或 `TeamSwitch` tool 成功后 | `{ conversationId, oldTeamName, newTeamName }` |
| `team-chat:appended` | `team-chat.jsonl` 写入一行后 | `{ conversationId, teamName, ts, from, to, text, variant }` |

前端 `team-chat:appended` 监听器只接收当前 UI 上选中那个 team 的事件（按 `teamName` 过滤），其它 team 的事件直接丢弃以减小渲染压力。

### 11.4 现有前端兼容契约

- `TeamOverview.teams` 数组当前前端可能只读 `teams[0]`——本期后**保留这种读法的兼容性**：后端按 `created_at` 倒序排，第一个永远是最近创建的 team；旧前端在多 team conv 上只会显示"最近一个"，不会崩。
- `src/services/teamMemorySync` / `src/components/teams/` 现有引用如果出现 `team-chat.jsonl` / `team.json` 字面量，**新加一层 Tauri 命令做中转**——前端不直接读文件，本期把直接读路径的代码全部改成调命令。这是必须的，因为新布局下 `team.json` 不在 conv 根目录而在 `teams/{name}/config.json`。
- 旧 conv 里残留的根级 `team.json` / `team-chat.jsonl`：前端 Tauri 命令直接返回空（按 §8 决策旧数据不展示）。

### 11.5 i18n

`src/i18n/{zh-CN,en-US}.json` 增 4 个 key：

- `team.button.label` / `team.button.tooltip`（按钮）
- `team.drawer.title`（"团队列表 / Teams"）
- `team.drawer.empty`（"当前对话还没有团队"）
- `team.chat.empty`（"该团队还没有内部消息"）

### 11.6 前端越界部分（其它 out-of-scope 见 §13）

- 手动 TeamCreate / TeamDelete UI（team 生命周期由 LLM 工具决定）
- team 间消息搬运 UI（Lead-as-bridge pattern 是 LLM 自然行为，不需要专门 UI）
- teammate transcript 单独浏览面板（仅展示 team-chat）
- per-team token / cost 视图

---

## 12. 推出顺序

本设计文档批准后，writing-plans 阶段会拆成以下 PR（顺序执行）：

| PR | 内容 | 可测 |
|---|---|---|
| PR1 | 新建 `team_paths.rs` + `validate_team_name` + 单元测试 | ✅ |
| PR2 | `TeamRegistry` 内层 HashMap 改造 + `delete_team` / `drop_session` / `hydrate_from_disk` 三 API 分立 + `write_atomic_team` 走 fs_atomic + 单测 | ✅ 旧测试全过 |
| PR3 | `ToolExecutionContext::active_team_name` + `ConversationMeta::active_team_name` + ctx 注入链路（无 Path C wake 部分）+ task_tools / send_message / output_writer / task_output / spawn_subagent 路径接入 `TeamPaths` | ✅ |
| PR4 | 三元 key 扩展：`InboxRegistry` / `AgentNameRegistry` / `CancellationRegistry`；`TeammateWorkerCtx` 加 `team_name`；`cleanup_teammate` 三处 unregister 同步；`task_notification_lead::emit_to_lead` 签名加 team_name；批量方法 `unregister_team` / `cancel_team` | ✅ 三元 key 单元 + 集成测试 |
| PR5 | 取消 conv 一对一限制：`TeamSwitch` 工具；TeamCreate 允许多 team；TeamDelete 严格 cancel→delete 顺序；`build_teammate_permission_ctx` 收窄到 `teams/{own_team}/` | ✅ 多 team 集成测试 |
| PR6 | Path C wake 路径改造：`WakeFn` 签名加 team_name；inbox 投递透传；continuation turn 不读 conv.json | ✅ Path C 集成测试 |
| PR7 | `team_view.rs` 多 team 支持 + review_ 回归 | ✅ |
| PR8 | `DiagnosticEvent.team_name` 字段 + 所有 record_diagnostic 调用透传 | ✅ |
| PR9 | Tauri 命令 `team_chat_messages` / `team_switch_active` + 前端事件 `team:created` / `team:deleted` / `team:active-changed` / `team-chat:appended` | ✅ |
| PR10 | 前端"团队"按钮 + 抽屉列表 + team-chat 面板 + 4 个 i18n key（最小可用 UI） | ✅ 手动 e2e（dev server 跑通） |

每个 PR 都满足 superpowers:verification-before-completion——不依赖"下一个 PR 修"的中间态。

---

## 13. 显式 out-of-scope 声明

以下能力**本期不实现**，由后续迭代专题处理：

- **MCP per-team 生命周期**：`McpServerManager` 是 process-wide 单例，team 不拥有 MCP server，TeamDelete 不影响 MCP 连接
- **Skill per-team 注入**：`SkillRegistry` 是 user-global，`SkillSubstitutionContext` 不带 team_name；不同 team 加载 skill 的隔离能力留待后续
- **Per-team 细粒度权限**：本期权限仍是 session 级共享，`PermissionStore` 不引入 team 维度；同一 conv 内所有 team 共享 path/tool 授权
- **Per-team token / cost 统计**：`TurnCompleted` 事件不携带 team_name 字段维度的 token 累加；前端只展示 conv 级累计；后续若需要在 RuntimeEvent + chatStore + 前端三处同步扩展
- **`agent_invocations.json` 的 team 字段**：`AgentInvocationRecord` 暂不加 `team_name`；TeamDelete 不清理 invocation 记录（pre-existing 行为不变）
- **team-chat.jsonl 容量上限 / compaction**：本期不做 rotation；team-chat 仅作 UI 镜像，不进 LLM context，token 压力可控
- **inter-team 通信 UI**：Lead 通过 TeamSwitch 在两个 team 间切换、自行搬运消息（Lead-as-bridge pattern）；不专设跨 team SendMessage 通道
- **teammate transcript 浏览面板**：前端仅展示 team-chat，不暴露单 teammate 内部 LLM 转录
- **手动 TeamCreate / TeamDelete UI**：team 生命周期由 LLM 工具决定，前端不提供按钮直接调
- **旧 conv 迁移**：见 §8，破坏性接受，老 team 数据在新版本不可见

每条都有充分理由（生命周期解耦 / 复杂度 / 等用户反馈再做）。Spec 通过这一节固化"何为本期范围"，防止 PR 阶段范围漂移。

---

## 14. 待人工确认

- [ ] §6.5 Path C wake 来源 team 优先策略是否可接受（替代方案：读 conv.json::active_team_name，但会让 wake 的 continuation turn 跑错 team）
- [ ] §6.6 hydration 仅重建 TeamRegistry、不 resume Teammate 是否可接受（替代方案：把 Teammate 也按 `teams/{name}/teammates/*.meta.json` 恢复，复杂度高很多）
- [ ] §8 不迁移、老 team 不可见的破坏性是否可接受
- [ ] §13 各项 out-of-scope 是否都同意推后
