# Per-Team 子目录化磁盘布局（方案 A）

**Status**: Draft
**Date**: 2026-05-14
**Owner**: pzc
**Scope**: 后端 `runtime/agent/team*`、`runtime/tools/builtin/{team_tools, send_message, task_tools, spawn_subagent}`、`runtime/agent/output_writer`、`runtime/team_view`、相关迁移脚本。

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
    ├── {sanitize(team_name_A)}/
    │   ├── config.json                    # 原 team.json
    │   ├── team-chat.jsonl                # 该 team 内部消息
    │   ├── tasks/
    │   │   ├── 1.json
    │   │   └── .highwatermark
    │   └── teammates/
    │       ├── {agent_id}.jsonl
    │       └── {agent_id}.meta.json
    └── {sanitize(team_name_B)}/
        └── ...
```

### 命名规则

- **路径用** `sanitize_path_component(team_name)`：`[^a-zA-Z0-9_-]` → `-`，转小写
- **显示用**原始 `team_name` 保留在 `config.json::team_name` 字段
- **唯一性约束**：同一 conv 内 `sanitize(team_name)` 不重复（在 TeamCreate 时校验）

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

    pub fn team_root(&self) -> Option<PathBuf>;       // teams/{sanitize(name)}
    pub fn config_json(&self) -> PathBuf;             // teams/{name}/config.json OR (legacy fallback) <conv>/team.json
    pub fn team_chat_jsonl(&self) -> PathBuf;
    pub fn tasks_dir(&self) -> PathBuf;
    pub fn teammates_dir(&self) -> PathBuf;
    pub fn teammate_transcript(&self, agent_id: &str) -> PathBuf;
    pub fn teammate_meta(&self, agent_id: &str) -> PathBuf;
}

pub fn sanitize_team_name(raw: &str) -> String;       // 复用 CCB 规则
```

**所有调用方都必须通过 `TeamPaths` 取路径**——禁止再写裸 `conv_dir.join("team.json")` 等。Review 时 grep `team.json` / `team-chat.jsonl` / `teammates/` 字面量应仅出现在 `team_paths.rs` 内。

---

## 4. 运行态数据结构

### `runtime/agent/team.rs::TeamRegistry`

```rust
pub struct TeamRegistry {
    // 外层 SessionId，内层 sanitized team_name → Team
    teams: Mutex<HashMap<SessionId, HashMap<String, Arc<Mutex<Team>>>>>,
}

impl TeamRegistry {
    pub async fn create(
        &self,
        session_id: SessionId,
        lead: Member,
        team_name: String,                            // 原始名
    ) -> Result<Arc<Mutex<Team>>, TeamError>;

    pub async fn get(
        &self,
        session_id: &SessionId,
        team_name: &str,                              // 原始或 sanitized 都接受
    ) -> Option<Arc<Mutex<Team>>>;

    pub async fn list(&self, session_id: &SessionId) -> Vec<(String, Arc<Mutex<Team>>)>;

    pub async fn delete(
        &self,
        session_id: &SessionId,
        team_name: &str,
    ) -> Option<Arc<Mutex<Team>>>;

    pub async fn persist(
        &self,
        session_id: &SessionId,
        team_name: &str,
        conv_dir: &Path,
    ) -> Result<(), TeamPersistError>;
}
```

唯一性：`create()` 内查 `inner_map.contains_key(&sanitize_team_name(&team_name))`，已存在返回 `TeamError::NameAlreadyTaken`。

### `Team` 本身

无需改动结构——`team_name` 字段已有。但需要补一个方法：

```rust
impl Team {
    pub fn sanitized_name(&self) -> String { sanitize_team_name(&self.team_name) }
}
```

### `ToolExecutionContext`

`runtime/tools/context.rs` 新增字段：

```rust
pub struct ToolExecutionContext {
    // ...
    pub conv_dir: Option<PathBuf>,        // 已有
    pub active_team_name: Option<String>, // ← 新增，原始名（非 sanitized）
}
```

**注入点**：`SessionRuntime` 构造 ctx 时，从某处读出"这个 tool call 属于哪个 team"，注入 `active_team_name`。具体来源：
- 主对话 Lead 调 tool：ctx.agent_id 反查 `TeamRegistry.list(session)` 找到 lead 所在的 team
- Teammate 调 tool：ctx.agent_id 反查到唯一 team
- 都查不到：保持 `None`（Lead 单飞场景）

实现细节见 §7 改造点 7-3。

---

## 5. 主要改造点（按文件清单）

| # | 文件 | 改造内容 |
|---|---|---|
| 5-1 | `runtime/agent/team.rs` | `TeamRegistry` 内层加 team_name 维度；`persist`/`delete_persisted` 改用 `TeamPaths`；`Team::sanitized_name`；唯一性校验 |
| 5-2 | `runtime/agent/team_paths.rs` | **新建**——`TeamPaths` + `sanitize_team_name` |
| 5-3 | `runtime/agent/team_context.rs` | `render(...)` 接收 team_name，把 prompt 里的 `团队配置` 路径替换为 `teams/{name}/config.json`；`tasks/` 路径替换为 `teams/{name}/tasks/` |
| 5-4 | `runtime/agent/output_writer.rs` | `transcript_path_for_kind` / `meta_path_for_kind` 增加 `team_name: Option<&str>` 参数；当 kind=Teammate 且 team_name=Some 时走 `teams/{name}/teammates/`，否则走旧路径；`AgentTranscriptMeta::team_id` 字段语义改为 sanitized team_name（**破坏性**——见 §8） |
| 5-5 | `runtime/tools/context.rs` | 加 `active_team_name: Option<String>` 字段 + `with_active_team` builder |
| 5-6 | `runtime/tools/builtin/team_tools.rs::TeamCreate` | 唯一性校验；建 `teams/{name}/` 目录；写 `config.json` 而非 `team.json`；ctx 上回灌 `active_team_name` 供后续 tool 看到 |
| 5-7 | `runtime/tools/builtin/team_tools.rs::TeamDelete` | 改为按 team_name 删除；磁盘上 `rm -rf teams/{sanitize(name)}/`；从内层 HashMap 移除该 entry，不删整个 session entry |
| 5-8 | `runtime/tools/builtin/send_message.rs::append_team_chat_entry` | 走 `TeamPaths::team_chat_jsonl()`；Lead/Teammate 必有 team_name 才能 SendMessage，无 team_name 返回 `ExecutionFailed("SendMessage requires active team")` |
| 5-9 | `runtime/tools/builtin/task_tools.rs::store_for` | `ctx.active_team_name` 走 `teams/{name}/tasks/`；无则 `conv_dir/tasks/`（Lead 单飞兼容） |
| 5-10 | `runtime/tools/builtin/spawn_subagent.rs` | 派 teammate 时把 `team_name` 写进 ctx；teammate 的 transcript/meta 走 `teams/{name}/teammates/` |
| 5-11 | `runtime/agent/worker_runtime.rs` | Teammate boot prompt 拼装时用新的 `teams/{name}/...` 路径；worktree allow-list 增加 `teams/{name}/` 子目录 |
| 5-12 | `runtime/team_view.rs` | 改为扫 `teams/` listdir 后 per-team 构建 `TeamSession`；`append_events_from_team_chat_jsonl` 接收 team_name 参数；`load_teammates` 从 `teams/{name}/teammates/` 读 |
| 5-13 | `transport/tauri_commands/...` | 若有 list/get team 的命令，签名加 team_name 参数（如有） |

**禁止新增** `LegacyToolAdapter`、`compat_*` 字段或"兼容模式开关"——一次性完成（除迁移脚本本身）。

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

Teammate spawn 时被锁定到一个 team——`AgentTranscriptMeta.team_id = sanitize(team_name)`。Teammate 永不切换 team。Teammate 调 tool 时 ctx 的 `active_team_name` 由其归属 team 推导，**不是** Lead 的 active team——这样即使 Lead 已经切到别的 team，原来 team 里 idle 的 Teammate 收到消息后仍能正确读写自己 team 的 tasks/。

### 6.3 SendMessage 路由

- 解析 `to: "team-lead"`：解析到 sender 所在 team 的 Lead（Lead 不一定 active 在该 team，但身份仍然是该 team 的 Lead）
- `to: "*"`：sender 所在 team 内广播
- `to: "<peer_name>"`：仅在 sender 所在 team 内 resolve；跨 team 不可达

`AgentNameRegistry` 当前是 `HashMap<SessionId, HashMap<Name, AgentId>>`，需要扩成 `HashMap<SessionId, HashMap<TeamName, HashMap<Name, AgentId>>>`。或者更简单：注册时 name 自动加上 `@team_name` 后缀变全限定。本期选**前者**（嵌套 HashMap），保留 user-facing `to` 字符串干净。

### 6.4 LeadIdle / Inbox

每个 team 的 Lead inbox / lead_idle supervisor 独立。当前 `LeadIdleSupervisor` 用 `(SessionId, AgentId)` 做 key，已经天然按 agent_id 隔离，**无需改动**——因为同一 Lead 在不同 team 内是同一个 agent_id（Lead 就是主对话 LLM 自己），但 inbox 是 per-team 的：

`InboxRegistry` 当前 `HashMap<(SessionId, AgentId), Inbox>`。Lead 的 inbox 在两个 team 内是否要分开？

**结论**：分开。`InboxRegistry` key 改为 `(SessionId, TeamName, AgentId)`。理由：Lead 在 team-alpha 收到的消息和在 team-beta 收到的消息属于不同对话上下文，混进同一 inbox 会让 Lead 看到跨 team 串流的消息。

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
2. team_tools.rs::TeamDelete::execute
   a. ctx.team_registry().delete(session, "alpha") → 从内层 HashMap 移除
   b. 取消所有 alpha 内 Teammate 的 cancel token（worker 自然退出）
   c. fs::remove_dir_all(TeamPaths::for_team(conv_dir, "alpha").team_root())
   d. agent_names 清掉 (session, "alpha") 下所有 entry
   e. inbox_registry 清掉 (session, "alpha", *) 所有 entry
   f. 若 conv.json::active_team_name == "alpha" 改为 None
```

---

## 8. 迁移策略

### 8.1 现有数据形态

```
旧：<conv>/team.json + team-chat.jsonl + tasks/* + teammates/*
新：<conv>/teams/{sanitize(team_name)}/{config.json, team-chat.jsonl, tasks/, teammates/}
```

### 8.2 迁移触发时机

**懒迁移**（不在启动时一次性扫全部 conv）：

- `TeamRegistry::create` 之前：如果 `<conv>/team.json` 存在 → 读出旧 team_name → 调 `migrate_legacy_to_per_team()` 把根目录里的 team 数据下沉到 `teams/{name}/`
- `team_view::build_overview` 之前：如果 conv 没有 `teams/` 但有 `team.json` → 同上

### 8.3 迁移函数（伪代码）

```rust
fn migrate_legacy_to_per_team(conv_dir: &Path) -> io::Result<()> {
    let legacy_team_json = conv_dir.join("team.json");
    if !legacy_team_json.exists() { return Ok(()); }

    let snapshot: TeamSnapshot = read_json(&legacy_team_json)?;
    let safe = sanitize_team_name(&snapshot.team_name);
    let team_root = conv_dir.join("teams").join(&safe);
    fs::create_dir_all(&team_root)?;

    // 1. config
    fs::rename(&legacy_team_json, team_root.join("config.json"))?;
    // 2. team-chat
    let chat = conv_dir.join("team-chat.jsonl");
    if chat.exists() {
        fs::rename(&chat, team_root.join("team-chat.jsonl"))?;
    }
    // 3. teammates/
    let teammates = conv_dir.join("teammates");
    if teammates.exists() {
        fs::rename(&teammates, team_root.join("teammates"))?;
    }
    // 4. tasks/ —— ⚠️ 难点见 §8.4
    let tasks = conv_dir.join("tasks");
    if tasks.exists() {
        fs::rename(&tasks, team_root.join("tasks"))?;
    }
    Ok(())
}
```

### 8.4 迁移难点：tasks/ 归属

旧布局下 `tasks/` 是 Lead 和 Teammate 共享池。新布局下：
- "Lead 单飞 task" 属于 conv 根 `tasks/`
- "team 内 task" 属于 `teams/{name}/tasks/`

历史 conv 里这两者已经混在一起，**无法机器判别**。处理方式：

**全部下沉到 team 子目录**——因为历史 conv 只可能有 0 或 1 个 team，迁移后那个 team 拥有所有 task，等同于"该 conv 历史上没有过 Lead 单飞 task"的合理假设（实际多数情况确实如此）。如果用户后续在该 conv 起 Lead 单飞 task，新 task 会进根 `tasks/`，老 task 在 `teams/{name}/tasks/` 里——可接受。

### 8.5 `AgentTranscriptMeta.team_id` 字段语义变更

**旧**：`team_id = session_id`（output_writer.rs:201 注释 "The conversation id — used as the team scope id"）
**新**：`team_id = sanitize(team_name)`

迁移函数同时改写每个 `teammates/*.meta.json` 的 `team_id` 字段——读出来、改值、写回。
新写入的 meta 直接用新语义。

**兼容窗口**：本期完成迁移后即不再支持旧语义；任何残留旧 meta 的逻辑必须由迁移补齐而非运行时兼容。

### 8.6 迁移失败处理

迁移失败（IO 错误）：
- log error
- 该 conv 视为只读：禁止任何 TeamCreate/spawn，提示用户手动检查
- 不阻塞别的 conv

---

## 9. 错误处理

| 场景 | 行为 |
|---|---|
| TeamCreate 时 sanitize(name) 在 conv 内已存在 | `TeamError::NameAlreadyTaken(name)` |
| TeamCreate 时 team_name 全是非法字符（sanitize 后为空） | `ToolError::ExecutionFailed("team_name must contain alphanumeric / _ / -")` |
| TeamDelete 指向不存在的 team | 返回 noop 提示，**不报错**（幂等） |
| TeamSwitch 切到不存在的 team | `ToolError::ExecutionFailed("team `{name}` not found in this conversation")` |
| SendMessage 调用方无 active team | `ToolError::ExecutionFailed("SendMessage requires active team; call TeamCreate first")` |
| task tool 在 active team 下但 `teams/{name}/tasks/` 目录不可创建 | log warn + fallback 到 conv 根 tasks/，确保 task 不丢 |
| 迁移函数 IO 错误 | 见 §8.6 |

---

## 10. 测试策略

### 10.1 单元测试

- `team_paths.rs`：path 派生函数全枚举（with/without team_name）
- `sanitize_team_name`：中文/空格/特殊字符/全非法字符/超长名
- `TeamRegistry`：同 conv 内 unique 校验、跨 conv 不冲突、`list/delete` 正确性
- 迁移函数：从模拟旧布局到新布局的完整迁移（含 task_id 字段重写）

### 10.2 集成测试

新增 `src-tauri/tests/team_multi_team_test.rs`：
- 同 conv 内创建 2 个 team，验证磁盘隔离
- TeamSwitch 后 task tool 落在正确目录
- TeamDelete 一个 team，验证另一个 team 不受影响
- SendMessage 跨 team 不可达

新增 `src-tauri/tests/team_migration_test.rs`：
- 准备一个旧布局 conv fixture
- 触发懒迁移
- 验证磁盘最终状态 + `team_id` 字段语义已升级

### 10.3 review_ 回归

新增 `src-tauri/tests/review_team_paths.rs`：
- grep 整仓 `team.json` / `team-chat.jsonl` 字面量出现位置
- 断言它们仅在 `team_paths.rs` 内（以及测试 fixture）

---

## 11. 前端影响

前端通过 `team_view.rs::build_overview` 拿到 `TeamOverview { conversation_id, teams: Vec<TeamSession> }`：

- **当前** `teams` 数组实际上长度 ≤ 1（按 TeamCreate/TeamDelete 窗口拆）
- **本期后** `teams` 数组真实承载多 team

需要做的前端变更：
1. team 列表 UI：从"显示 0/1 个 team"变成"显示 N 个 team"
2. 切换 active team 的 UI 入口（调 TeamSwitch tool 或直接走新增的 Tauri 命令）
3. SendMessage / task list / teammate list 视图按当前选中的 team 过滤

前端改造**不在本期范围内**——本期只保证后端契约稳定，前端可在后续迭代逐步接入。

---

## 12. 推出顺序

本设计文档批准后，writing-plans 阶段会拆成以下 PR（顺序执行）：

| PR | 内容 | 可测 |
|---|---|---|
| PR1 | 新建 `team_paths.rs` + `sanitize_team_name` + 单元测试 | ✅ |
| PR2 | `TeamRegistry` 内层 HashMap 改造 + 现有 team_tools 适配（仍按 conv 一对一限制） | ✅ 旧测试全过 |
| PR3 | `ToolExecutionContext::active_team_name` + 注入链路 + task_tools/send_message 接入 TeamPaths | ✅ |
| PR4 | 迁移函数 + 懒迁移触发 + 字段语义升级 | ✅ |
| PR5 | 取消 conv 一对一限制 + `TeamSwitch` 工具 + `InboxRegistry` / `AgentNameRegistry` key 扩展 | ✅ 多 team 集成测试 |
| PR6 | `team_view.rs` 多 team 支持 + review_ 回归 | ✅ |

每个 PR 都满足 superpowers:verification-before-completion——不依赖"下一个 PR 修"的中间态。

---

## 13. 待人工确认

- [ ] §6.4 InboxRegistry key 扩展为 `(SessionId, TeamName, AgentId)` 的破坏性是否可接受
- [ ] §8.4 历史 task 全部下沉到 team 子目录的迁移策略是否可接受
- [ ] §8.5 `AgentTranscriptMeta.team_id` 字段语义变更的兼容窗口策略
- [ ] §6.1 `TeamSwitch` 新工具命名是否合适（替代名：`TeamEnter` / `TeamFocus`）
- [ ] §11 前端改造是否本期同步进行
