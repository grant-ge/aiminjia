# Agenda 基座设计：定时任务重做与日程铺路

- 日期：2026-05-06
- 作者：pzc
- 状态：待评审

## 1. 背景与目标

### 1.1 现状问题

后端调研（功能完整性 4/10）、前端调研（UI 完整性 3/10）已经明确：

- `ScheduleRecord` 缺 `last_run_at` / `run_count` / `conversation_id` 关联，无执行历史表
- 仅有 `create / list / delete` 三个命令，缺 `update / 启停切换 / 立即运行`
- runner 60s tick，错过的任务（休眠/重启）不补跑
- 自实现 cron 仅支持 5 字段子集，无秒/年/特殊符号
- 命令层未薄转发，schedule 未接入 SessionId/RunId 模型
- 前端创建只能点模板（3 个硬编码），无 cron 可视化、无编辑、无启停、无立即运行、无执行历史
- 列表行只有"删除"一个操作，enabled/disabled 视觉无差异

### 1.2 产品诉求

- 把现有"定时任务"作为基座能力，未来在其上叠加"日程"
- 日程是给数字员工（persona）的，每条日程归属某个员工
- 员工本人也能在对话中创建/管理自己的日程

### 1.3 本期目标

1. **后端**：把"定时任务"重做成 `agenda` 基座（`AgendaItem + Trigger + Occurrence`），为日程铺路
2. **前端**：UI 形态保持现状（仍是"定时任务"页面），但补齐功能缺陷
3. **工具**：新增一组工具让 agent 在对话中管理自己的日程
4. **顺便**：修 runner 在 scope 切换时不切 store 路径的 bug

### 1.4 非目标

- 日历视图、日程视图、时间轴视图——二期再做
- 多 persona 同时参会的"多人对话协同"——本期只建数据结构（`participants` 字段长度恒为 1）和编辑入口（"添加协作者"按钮可见但灰掉），触发逻辑只跑 organizer
- 派生对话（fork conversation）、context_handoff 字段——员工自己把上下文写进 prompt
- workspace 绑定/切换——跟用户全局 workspace 走
- iCalendar / RRULE / VEVENT 兼容——日程二期再决定要不要做

---

## 2. 领域模型

### 2.1 核心抽象

`AgendaItem` 是基座唯一一等公民。

```rust
// src-tauri/src/runtime/agenda/item.rs

pub struct AgendaItemId(pub String);  // "agenda-{uuid}"

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgendaItem {
    pub id: AgendaItemId,
    pub title: String,
    pub prompt: String,
    pub trigger: Trigger,
    pub organizer_persona_id: String,    // 组织者，必填，必须 ∈ participants
    pub participants: Vec<String>,       // 全部参与员工的 persona_id；本期长度恒为 1
    pub status: ItemStatus,

    pub last_run_at: Option<DateTime<Utc>>,
    pub run_count: u32,
    pub last_run_status: Option<RunOutcome>,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Trigger {
    Cron {
        expr: String,
        timezone: String,           // IANA, e.g. "Asia/Shanghai"
        next_fire_at: Option<DateTime<Utc>>,
    },
    OneShot {
        fire_at: DateTime<Utc>,
        fired: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Active,     // 正常运行
    Paused,     // 用户暂停
    Completed,  // OneShot 已触发完成
    Orphaned,   // owner persona 已删除，需要用户处理
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Success,
    Failed,
}
```

### 2.2 字段说明（关键决策）

- **`organizer_persona_id` 必填**：日程归属唯一组织者；UI 上"执行身份"字段就是它
- **`participants` 至少含 organizer**：约束 `organizer_persona_id ∈ participants`，本期 `participants.len() == 1` 且 == organizer。多人协同（participants 长度 > 1）二期实现，但数据结构和 UI"添加协作者"入口本期就建好（按钮可见但灰掉，标"即将上线"）
- **owner（即 organizer）创建后不可转移**：编辑面板里 `organizer_persona_id` 字段只读
- **OneShot 完成后**：runner 触发后置 `OneShot.fired = true` 且 `status = Completed`；列表默认过滤掉 Completed
- **`Orphaned` 状态**：organizer 被删除时由 runtime 把所有该 persona 为 organizer 的 item 置为 Orphaned，runner 不触发 Orphaned 项；UI 上以警示色显示并引导用户处理（删除或重指 organizer——这是 organizer "不可转移"约束的唯一例外，仅限"复活"流程）

### 2.3 执行历史模型

`Occurrence` 记录每一次触发的实际发生：

```rust
// src-tauri/src/runtime/agenda/occurrence.rs

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub id: String,                       // "occ-{uuid}"
    pub agenda_item_id: AgendaItemId,
    pub fired_at: DateTime<Utc>,          // 实际触发时刻
    pub planned_fire_at: DateTime<Utc>,   // 原本计划的触发时刻
    pub primary_persona_id: String,       // 实际执行者（= organizer）
    pub conversation_id: String,          // 触发后新建的 conversation
    pub session_id: SessionId,            // 接入运行时 ID 体系
    pub run_id: RunId,
    pub status: OccurrenceStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error_summary: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus {
    Running,
    Succeeded,
    Failed,
}
```

`AgendaItem.last_run_at / run_count / last_run_status` 是从 occurrence 派生的快照字段，写入时同步更新——避免列表渲染要扫一遍 occurrence 文件。

---

## 3. 持久化

### 3.1 目录布局

跟随用户 scope（`paths.base_dir()`）：

```
{user_scope}/                            # ~/.renlijia/users/t_xxx__u_yyy/
├── agenda/
│   ├── items/
│   │   └── {agenda_item_id}.json        # 一条日程定义
│   └── occurrences/
│       └── {agenda_item_id}/
│           └── {yyyy-mm}.jsonl          # 当月执行历史，纯追加
└── schedules/                           # 老路径，本期清空（不迁移）
```

### 3.2 Item 写入

复用现有 `atomic_write_json`（写 tmp + rename），单 `Mutex<()>` 序列化。

### 3.3 Occurrence 写入

按月分片纯追加：每次触发时 `OpenOptions::append().create(true)` 打开 `{yyyy-mm}.jsonl` 写一行。Running → Succeeded/Failed 是同一条记录的更新——用"两段写"：

- 触发瞬间：写一行 `Running` 状态的 occurrence
- 完成时：再追加一行 same `id` 的 `Succeeded/Failed` 记录（带 finished_at + error_summary）

读取时按 id 取最后一行作为最终状态。这避免 JSONL 中途改写的复杂度。

### 3.4 不迁移

线上无真实数据——老 `schedules/*.json` 不读、不迁移。启动时若发现老目录非空，仅打印一条 info log，不做任何动作。后续清理由后续提交里加 `schedules/` 目录的删除（或人工 rm），不在本期 doc 范围。

---

## 4. 模块结构（Rust）

### 4.1 新增 `runtime/agenda/`

```
src-tauri/src/runtime/agenda/
├── mod.rs
├── item.rs            // AgendaItem / Trigger / Participant / ItemStatus
├── occurrence.rs      // Occurrence / OccurrenceStatus
├── store.rs           // AgendaStore: 文件持久化 + 查询
├── trigger_eval.rs    // Cron 解析 + next_fire_at 计算（沿用现 schedule.rs 的实现，扩展 OneShot 分支）
├── runner.rs          // AgendaRunner: 60s tick 扫描，触发分发
└── dispatcher.rs      // AgendaRunDispatcher trait（扩展现有 ScheduleRunDispatcher）
```

### 4.2 替换现有

- 删除 `runtime/schedule.rs`、`runtime/schedule_runner.rs`
- 删除 `commands/schedules.rs`
- 新增 `transport/tauri_commands/agenda.rs`（薄转发，遵循"command 只接受参数 → 转发 runtime"原则）

### 4.3 接入 SessionId/RunId

后端调研指出现 schedule 未接入运行时 ID 体系。本期 `AgendaRunDispatcher` 实现里：

- 触发时显式 `SessionId::new()` + `RunId::new()`
- Occurrence 记录 session_id / run_id，链路可溯源
- `dispatch_chat_request` 调用时传入这套 ID

---

## 5. 触发与执行

### 5.1 Runner

```rust
// runtime/agenda/runner.rs

pub fn spawn_agenda_runner(
    path_resolver: Arc<dyn UserScopedPathResolver>,
    dispatcher: Arc<dyn AgendaRunDispatcher>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            // 关键：每个 tick 重新 resolve scope，修复 scope 切换 bug
            let Some(paths) = path_resolver.resolve_paths() else { continue; };
            let store = AgendaStore::new(paths.base_dir());
            run_due_once(&store, dispatcher.as_ref(), Utc::now()).await.ok();
        }
    });
}
```

每 tick 重新 `resolve_paths()` 即修复了"scope 切换后 runner 仍指向旧 scope"的问题。本期顺便加 review_ 测试锁住此行为。

### 5.2 触发时序

1. Runner tick：`AgendaStore::take_due(now)` 找到所有 `next_fire_at <= now` 且 `status=Active` 的 item
2. 对每个 due item：
   - 推进 `next_fire_at`（Cron）或置 `OneShot.fired=true` + `status=Completed`（OneShot）
   - 写入 occurrence（`Running`）
   - 调 `AgendaRunDispatcher::dispatch(item, occurrence)`
3. Dispatcher 实现（在 `transport/tauri_commands/chat.rs` 或新文件）：
   - 新建 conversation（`source = AgendaTrigger { item_id, occurrence_id }`，记录在 conversation metadata 里）
   - 切到 item.primary persona
   - 发送 `item.prompt` 作为 user message
   - 走完整 agent 主链路
4. agent 跑完：
   - 根据最终结果（成功/异常）追加一行 occurrence（`Succeeded/Failed`）
   - 更新 item 的 `last_run_at / run_count / last_run_status` 快照字段

### 5.3 不补跑（沿用现状）

错过的触发点（休眠/重启）不补跑，跳到下一个计划时间。理由：
- 用户预期"每天 9 点"是"每天 9 点这个时刻"，不是"我开机了就把昨天的也补一遍"
- 补跑语义复杂（多次错过怎么办？合并 prompt？逐个跑？）

未来可选加 `Trigger.miss_strategy` 字段，本期不做。

### 5.4 立即运行

`run_agenda_item_now` Tauri command：直接调 dispatcher，不经过 due 判定。**不暴露给 agent 工具**，仅 UI 端使用。

---

## 6. Tauri 命令（前端可调用）

`transport/tauri_commands/agenda.rs`：

| 命令 | 入参 | 出参 |
|---|---|---|
| `list_agenda_items` | `(filter: Option<ItemFilter>)` | `Vec<AgendaItem>` |
| `get_agenda_item` | `(id: String)` | `AgendaItem` |
| `create_agenda_item` | `CreateAgendaItemRequest` | `AgendaItem` |
| `update_agenda_item` | `(id, UpdateAgendaItemRequest)` | `AgendaItem` |
| `delete_agenda_item` | `(id: String)` | `bool` |
| `run_agenda_item_now` | `(id: String)` | `Occurrence` |
| `list_agenda_occurrences` | `(item_id, limit, before)` | `Vec<Occurrence>` |

`ItemFilter` 含 `status_in / persona_id / search`。前端列表保持现状（不分组、不过滤）——`ItemFilter` 字段都建好但默认全部传 None。

---

## 7. Agent 工具（agent 在对话中可调用）

新增 5 个 RuntimeTool（在 `runtime/tools/builtin/agenda/` 下）。**所有工具的 owner 范围强制限定为当前 persona**——runtime 注入 `current_persona_id`，工具实现里查询/修改时强制按此过滤，agent 改不了别人的日程。

### 7.1 `create_agenda_item`

```json
{
  "title": "string，必填",
  "prompt": "string，必填，到点要执行的内容",
  "trigger": {
    "kind": "cron" | "oneshot",
    "expr": "0 9 * * *",            // cron 时
    "timezone": "Asia/Shanghai",    // cron 时，默认 Asia/Shanghai
    "fire_at": "2026-05-07T09:00:00+08:00"  // oneshot 时
  }
}
```

owner（organizer）由 runtime 强制设为当前 persona：`organizer_persona_id = current_persona`，`participants = [current_persona]`，agent 不能传。

### 7.2 `list_agenda_items`

```json
{
  "status_in": ["active", "paused"],   // 可选
  "limit": 50                           // 可选
}
```

返回当前 persona 的日程列表。

### 7.3 `update_agenda_item`

```json
{
  "id": "agenda-xxx",
  "title": "string，可选",
  "prompt": "string，可选",
  "trigger": { ... },                   // 可选
  "status": "active" | "paused"         // 可选，启停切换
}
```

校验：id 必须属于当前 persona（organizer == current）；不允许改 organizer / participants。

### 7.4 `cancel_agenda_item`

```json
{ "id": "agenda-xxx" }
```

实际行为：删除 item（同时 occurrences 记录保留）。

### 7.5 `list_agenda_occurrences`

```json
{
  "agenda_item_id": "agenda-xxx",
  "limit": 20
}
```

校验：item 必须属于当前 persona。

---

## 8. 前端改造

### 8.1 范围

UI 形态保持现状（"定时任务"页面），但补齐功能缺陷。**列表布局/分组/过滤 这一期都不动**——保留现有 `ScheduleListCard / ScheduleTableHeader / ScheduleTaskRow / ScheduleEmptyState` 的组织结构。

### 8.2 命名变更

- `tauri.ts` 中 `listSchedules / createSchedule / deleteSchedule` 改为 `listAgendaItems / createAgendaItem / deleteAgendaItem` 等 7 个 invoke 封装
- `SchedulesPage` 路由名/UI 词不改（仍叫"定时任务"），内部调新 API
- 组件内部类型从 `ScheduleRecord` 改为 `AgendaItem`

### 8.3 列表行（`ScheduleTaskRow`）补齐

每行 hover 后显示 4 个图标按钮：

- **立即运行**（`run_agenda_item_now`）：调用 + toast 反馈
- **启停切换**（`update_agenda_item` + `status`）：Active ↔ Paused
- **编辑**（打开右侧 Sheet）
- **删除**（保留二次确认）

视觉强分化：

- Active：左侧 2px 蓝色色条 + 下次时间高亮
- Paused：整行灰显 70% opacity + 下次时间留空
- Completed：列表默认过滤掉
- Orphaned：左侧红色色条 + 警示文案"该员工已删除，请处理"
- 上次失败：右上角红色小角标
- 即将触发（< 5 min）：呼吸动画

行内增加：

- owner persona 头像 + 名字（小标签）
- 上次执行结果（✓ 时间 / ✗ 时间）

### 8.4 详情面板

新增一个右侧滑出 Sheet `AgendaItemDetail`，3 个 Tab：

1. **概览**：标题/owner/下次触发倒计时/最近 5 次 occurrence 摘要
2. **执行历史**：完整 occurrence 列表，每行显示触发时间/状态/耗时；点击跳转对应 conversation
3. **设置**：编辑 prompt/trigger/status（受 7.3 校验约束）

### 8.5 创建/编辑 Sheet

新增 `AgendaItemEditor` Sheet，分组：

1. **基础**
   - 标题
   - **执行身份**（必选 persona，下拉。创建后只读，仅 Orphaned 可改）
2. **触发时机**
   - 频率：一次性 / 每天 / 每周 / 每月 / 自定义
   - 时间选择器（HH:mm）
   - 星期/日期多选（频率选每周/每月时显示）
   - 时区（默认 Asia/Shanghai）
   - "自定义"展开 cron 表达式输入框（高级）
3. **执行内容**
   - prompt 多行编辑器（支持 `/` 引用 skill，沿用现有 composer 能力）
4. **协作者**（本期可见，但选项灰掉，标"即将上线"）
   - 添加协作者按钮 → disabled tooltip："多人协同将在后续版本支持"

### 8.6 模板系统

3 个硬编码模板降级为"创建 Sheet 顶部的 chip 起点"——点击 chip 预填表单（不直接创建），用户可改。

### 8.7 Hooks 抽象

将 `SchedulesPage` 内联的 fetch/refresh 抽到 `src/hooks/useAgendaItems.ts`，方便详情 Sheet 复用。

---

## 9. Persona 删除联动

订阅 persona 删除事件（或在 persona 删除命令里直接调用）：

```rust
// 在 PersonaService::delete 里
self.agenda_store.mark_orphaned_by_organizer(&persona_id)?;
```

实现：扫描 items，将 organizer 命中的置为 `Orphaned` 状态。Orphaned item runner 不触发。

UI 上 Orphaned 项显示警示色 + 可改 organizer（重指）—— 这是 organizer "不可转移"约束的唯一例外（仅复活流程）。

---

## 10. 测试

### 10.1 单元测试

- `agenda::store`：CRUD、并发安全、Orphaned 标记
- `agenda::trigger_eval`：cron 解析（沿用现 `expand_field` 测试）、OneShot next_fire_at 计算
- `agenda::runner`：take_due 推进 next_fire_at、Completed 不再触发、Orphaned 不触发

### 10.2 集成测试

- `tests/agenda_commands_test.rs`：list/create/update/delete/run_now 端到端
- `tests/agenda_runner_scope_test.rs`：scope 切换后 runner 切换 store（修 bug 的回归测试）
- `tests/agenda_persona_delete_test.rs`：persona 删除 → items 转 Orphaned
- `tests/review_agenda_session_id.rs`：触发链路必经 SessionId/RunId（架构约束回归）
- `tests/review_agenda_command_thinness.rs`：transport/tauri_commands/agenda.rs 不含业务逻辑（架构约束回归）

### 10.3 前端测试

- `SchedulesPage.test.tsx` 扩展：编辑、启停、立即运行、删除、Orphaned 状态、Completed 过滤
- `AgendaItemEditor.test.tsx` 新增：频率选择器各分支、cron 高级输入

---

## 11. 落地顺序（仅给后续 plan 参考，不在本 doc 决定）

1. 数据结构 + Store + trigger_eval（带单测）
2. Runner + Dispatcher（接入 SessionId/RunId）
3. Tauri commands + 前端 invoke 封装替换
4. 前端列表行/详情 Sheet/编辑 Sheet
5. Agent 工具
6. Persona 删除联动
7. Scope 切换 bug 回归测试

具体步骤、PR 切分由 writing-plans 阶段产出。

---

## 12. 待确认事项

无。所有澄清问题已对齐。

---

## 附录 A：与现状映射

| 现状 | 新模型 |
|---|---|
| `ScheduleRecord` | `AgendaItem`（增加 `organizer_persona_id` + `participants`） |
| `ScheduleStatus::Enabled / Disabled` | `ItemStatus::Active / Paused`（多了 Completed / Orphaned） |
| `cron` 字段 | `Trigger::Cron { expr, timezone, next_fire_at }` |
| `next_run_at` | `Trigger::Cron { ... next_fire_at }` |
| `human_schedule` | 不持久化，前端按 trigger 渲染 |
| 无 | `last_run_at / run_count / last_run_status` 快照 |
| 无 | `organizer_persona_id` + `participants: Vec<String>` |
| 无 | `Occurrence` 执行历史 |
| `ScheduleStore` | `AgendaStore` |
| `ScheduleRunDispatcher` | `AgendaRunDispatcher`（接入 SessionId/RunId） |

## 附录 B：未做但留口子的扩展点

- **多人协同（`participants.len() > 1`）**：本期数据结构存得下（`participants` 是 `Vec<String>`）、UI 编辑入口可见但灰掉，触发逻辑只跑 organizer。多人对话协同二期实现
- **派生对话 / source_conversation_id**：员工创建日程时把上下文写进 prompt 即可，本期不引入字段
- **workspace 绑定**：跟用户全局 workspace 走，未来可加 `AgendaItem.workspace_override`
- **`Trigger::Recurrence { dtstart, rrule }`**：日程二期若需要 RRULE 兼容时新增枚举分支
- **Action trait 抽象**：本期内联 PromptAction 行为；未来增加 `NotificationAction / WebhookAction` 时再抽 `Action` trait
- **错过补跑**：`Trigger.miss_strategy` 字段未来可加
- **`read_conversation` 工具**：让 agent 能查源对话，未来若需要可加
