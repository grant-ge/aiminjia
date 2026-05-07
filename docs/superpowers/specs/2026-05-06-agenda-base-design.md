# Agenda 基座设计：定时任务重做与日程铺路

- 日期：2026-05-06
- 作者：pzc
- 状态：待评审

## 1. 领域建模

### 1.1 实体清单

| 实体 | 用途 |
|---|---|
| `AgendaItem` | 一条日程定义（一次性 或 循环） |
| `Occurrence` | 一次执行的运行记录 |

`Participant` / `RecurrenceRule` / `OverrideRef` 嵌入在 `AgendaItem` 内，不是独立实体。

### 1.2 实体关系

```
Persona (1) ─── organizer ─── (N) AgendaItem (1) ─── (N) Occurrence (1) ─── (1) Conversation
                                  │
                                  └── participants(N)  本期 N=1，二期 N≥1
```

### 1.3 `AgendaItem`

```rust
// src-tauri/src/runtime/agenda/item.rs

pub struct AgendaItemId(pub String);  // "agenda-{uuid}"

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgendaItem {
    pub id: AgendaItemId,

    // —— 内容
    pub title: String,
    pub prompt: String,

    // —— 归属
    pub organizer_persona_id: String,            // 必填，创建后不可改
    pub participants: Vec<Participant>,          // 至少含 organizer；本期长度恒为 1

    // —— 时间
    pub start_at: DateTime<Utc>,                 // 开始时间
    pub timezone: String,                        // IANA, e.g. "Asia/Shanghai"
    pub rule: Option<RecurrenceRule>,            // None=一次性 / Some=循环
    pub skip_dates: Vec<DateTime<Utc>>,          // 循环跳过的具体次（本期开放）

    // —— 运行时缓存
    pub next_fire_at: Option<DateTime<Utc>>,
    pub occurrence_count: u32,

    // —— 状态
    pub status: ItemStatus,

    // —— 二期留口子
    pub override_of: Option<OverrideRef>,        // 本期恒为 None

    // —— 元信息
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Active,
    Paused,
    Completed,
    Orphaned,
}
```

### 1.4 `Participant`

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub persona_id: String,
    pub joined_at: DateTime<Utc>,
}
```

约束：`participants[0].persona_id == organizer_persona_id`，本期 `participants.len() == 1`。

### 1.5 `RecurrenceRule`

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRule {
    pub freq: Freq,                              // Daily/Weekly/Monthly/Yearly
    pub interval: u32,                           // >=1
    pub end_condition: EndCondition,
    pub by_day: Vec<Weekday>,                    // 二期留口子；本期恒空
    pub by_month_day: Vec<i8>,                   // 二期留口子；本期恒空
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Freq { Daily, Weekly, Monthly, Yearly }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EndCondition {
    Never,
    Count { n: u32 },
    Until { at: DateTime<Utc> },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Weekday { Mon, Tue, Wed, Thu, Fri, Sat, Sun }
```

`Count { n }` 按实际成功触发次数计数，对应 `AgendaItem.occurrence_count`。休眠/重启期间错过的计划时刻不补跑，也不消耗 Count 额度；runner 只在真正触发后递增 `occurrence_count`。

### 1.6 `OverrideRef`（二期留口子）

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverrideRef {
    pub series_item_id: AgendaItemId,            // 被覆盖的循环 item
    pub original_at: DateTime<Utc>,              // 原本是哪一次
}
```

二期实现"修改循环里的某一次"时启用：派生一条普通 Item，`override_of = Some(...)`，runner 触发时优先用 override item。

### 1.7 `Occurrence`

```rust
// src-tauri/src/runtime/agenda/occurrence.rs

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub id: String,                              // "occ-{uuid}"
    pub agenda_item_id: AgendaItemId,

    // —— 时间
    pub fired_at: DateTime<Utc>,
    pub planned_fire_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,

    // —— 执行链路
    pub primary_persona_id: String,
    pub conversation_id: String,
    pub session_id: SessionId,
    pub run_id: RunId,

    // —— 结果
    pub status: OccurrenceStatus,
    pub error_summary: Option<String>,

    // —— 触发来源
    pub trigger_source: TriggerSource,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus { Running, Succeeded, Failed }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    Scheduled,
    ManualRunNow,
}
```

### 1.8 状态转换

```
Active ─触发完一次性─→ Completed
Active ─触发完循环最后一次─→ Completed
Active ─用户暂停─→ Paused
Paused ─用户恢复─→ Active
Active ─organizer删除─→ Orphaned
Orphaned ─用户重指organizer─→ Active
```

### 1.9 本期严格约束（store 层校验）

写入时拒绝违反以下规则的 item：

1. `participants.len() == 1`
2. `participants[0].persona_id == organizer_persona_id`
3. `override_of.is_none()`
4. 若 `rule.is_some()`：`rule.by_day.is_empty() && rule.by_month_day.is_empty()`
5. `organizer_persona_id` 在 update 时不可改（除 Orphaned 复活流程外）

校验失败返回错误。runner 也在 due 检查时跳过违反约束的 item（防御性）。

二期开放某项约束时，删除对应校验即可，旧数据自动可用。

### 1.10 跳过单次（本期开放）

`skip_dates: Vec<DateTime<Utc>>` 本期开放使用：

- `trigger_eval` 计算下次触发时跳过命中 `skip_dates` 的时刻
- 提供 `skip_occurrence` / `unskip_occurrence` 工具与命令
- 仅对 `rule.is_some()` 的循环 item 有意义；一次性 item 不允许设 `skip_dates`（store 校验）

### 1.11 字段语义示例

**一次性日程**：
```
start_at = 2026-05-07T09:00:00+08:00
rule = None
→ 5/7 9:00 触发一次后 status=Completed
```

**每天循环**：
```
start_at = 2026-05-07T09:00:00+08:00
rule = Some { freq: Daily, interval: 1, end_condition: Never, by_day: [], by_month_day: [] }
→ 5/7 9:00, 5/8 9:00, 5/9 9:00, ... 无限
```

**每两周循环 10 次**：
```
start_at = 2026-05-07T10:00:00+08:00
rule = Some { freq: Weekly, interval: 2, end_condition: Count { n: 10 }, by_day: [], by_month_day: [] }
→ 5/7, 5/21, 6/4, ... 共 10 次后 Completed
```

---

## 2. 背景与目标

### 2.1 现状问题

后端调研（功能完整性 4/10）、前端调研（UI 完整性 3/10）已经明确：

- `ScheduleRecord` 缺 `last_run_at` / `run_count` / `conversation_id` 关联，无执行历史表
- 仅有 `create / list / delete` 三个命令，缺 `update / 启停切换 / 立即运行`
- runner 60s tick，错过的任务（休眠/重启）不补跑
- 自实现 cron 仅支持 5 字段子集，无秒/年/特殊符号
- 命令层未薄转发，schedule 未接入 SessionId/RunId 模型
- 前端创建只能点模板（3 个硬编码），无 cron 可视化、无编辑、无启停、无立即运行、无执行历史
- 列表行只有"删除"一个操作，enabled/disabled 视觉无差异
- runner 在 scope 切换后不会切换 store 路径

### 2.2 产品诉求

- 把现有"定时任务"作为基座能力，未来在其上叠加"日程"
- 日程是给数字员工（persona）的，每条日程归属某个员工
- 员工本人也能在对话中创建/管理自己的日程

### 2.3 本期目标

1. **后端**：把"定时任务"重做成 `agenda` 基座（`AgendaItem + Occurrence`），为日程铺路
2. **前端**：UI 形态保持现状（仍是"定时任务"页面），但补齐功能缺陷
3. **工具**：新增一组工具让 agent 在对话中管理自己的日程
4. **顺便**：修 runner 在 scope 切换时不切 store 路径的 bug

### 2.4 非目标

- 日历视图、日程视图、时间轴视图——二期再做
- 多 persona 同时参会的"多人对话协同"——本期约束 `participants` 长度恒为 1，字段已建好
- 修改循环日程的某一次（iCalendar RECURRENCE-ID）——字段留口子（`override_of`），本期约束恒为 None
- 派生对话（fork conversation）/ context_handoff——员工自己把上下文写进 prompt
- workspace 绑定/切换——跟用户全局 workspace 走
- 完整 CalDAV 协议兼容——领域字段向 iCalendar 形状靠拢，但不实现 CalDAV 同步

---

## 3. 持久化

### 3.1 目录布局

```
{user_scope}/agenda/
├── items/
│   └── {agenda_item_id}.json
└── occurrences/
    └── {agenda_item_id}/
        └── {yyyy-mm}.jsonl
```

跟随用户 scope（`paths.base_dir()`）。老 `schedules/` 不读不迁移。

### 3.2 Item 写入

复用现有 `atomic_write_json`（写 tmp + rename），单 `Mutex<()>` 序列化。

### 3.3 Occurrence 写入

按月分片纯追加：

- 触发瞬间：写一行 `Running` occurrence
- 完成时：再追加一行 same `id` 的 `Succeeded/Failed` 记录

读取时按 id 取最后一行作为最终状态。

---

## 4. 模块结构（Rust）

### 4.1 新增

```
src-tauri/src/runtime/agenda/
├── mod.rs
├── item.rs              // AgendaItem / Participant / RecurrenceRule / OverrideRef / ItemStatus / Freq / EndCondition / Weekday
├── occurrence.rs        // Occurrence / OccurrenceStatus / TriggerSource
├── store.rs             // AgendaStore: 文件持久化 + 查询 + 约束校验
├── trigger_eval.rs      // 一次性 / 循环规则 next_fire_at 计算 + skip_dates 过滤
├── runner.rs            // AgendaRunner: 60s tick 扫描，每 tick 重 resolve scope
└── dispatcher.rs        // AgendaRunDispatcher trait
```

### 4.2 删除

- `src-tauri/src/runtime/schedule.rs`
- `src-tauri/src/runtime/schedule_runner.rs`
- `src-tauri/src/commands/schedules.rs`

### 4.3 新增 Tauri 命令文件

`src-tauri/src/transport/tauri_commands/agenda.rs`：薄转发，遵循"command 只接受参数 → 转发 runtime"原则。

### 4.4 接入 SessionId/RunId

后端调研指出现 schedule 未接入运行时 ID 体系。本期 `AgendaRunDispatcher` 实现里：

- 触发时显式 `SessionId::new()` + `RunId::new()`
- Occurrence 记录 session_id / run_id，链路可溯源
- 加 `tests/review_agenda_session_id.rs` 锁住此约束

---

## 5. 触发与执行

### 5.1 Runner

```rust
pub fn spawn_agenda_runner(
    path_resolver: Arc<dyn UserScopedPathResolver>,
    dispatcher: Arc<dyn AgendaRunDispatcher>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            // 每个 tick 重新 resolve scope，修复 scope 切换 bug
            let Some(paths) = path_resolver.resolve_paths() else { continue; };
            let store = AgendaStore::new(paths.base_dir());
            run_due_once(&store, dispatcher.as_ref(), Utc::now()).await.ok();
        }
    });
}
```

每 tick 重新 `resolve_paths()` 即修复"scope 切换后 runner 仍指向旧 scope"的 bug。加 `tests/agenda_runner_scope_test.rs` 回归测试。

### 5.2 触发时序

1. Runner tick：`AgendaStore::take_due(now)` 找到所有 `next_fire_at <= now` 且 `status=Active` 且 `override_of.is_none()` 的 item
2. 对每个 due item：
   - 写入 `Running` 状态的 occurrence
   - 推进 item：`occurrence_count += 1`，重算 `next_fire_at`（含 skip_dates 过滤）
   - 若 `next_fire_at` 为 None（一次性已跑 / 循环已达 Count|Until）→ `status = Completed`
   - 调 `AgendaRunDispatcher::dispatch(item, occurrence)`
3. Dispatcher：
   - 新建 conversation
   - 切到 organizer persona
   - 发送 `item.prompt` 作为 user message
   - 走完整 agent 主链路
4. agent 跑完：
   - 追加一行 occurrence（`Succeeded/Failed`）

### 5.3 next_fire_at 计算

```
fn compute_next_fire_at(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match &item.rule {
        None => {
            // 一次性：start_at 未到且未触发
            if item.occurrence_count == 0 && item.start_at > now {
                Some(item.start_at)
            } else {
                None
            }
        }
        Some(rule) => {
            // 循环：从 start_at 出发按 freq+interval 步进
            // 过滤 skip_dates 命中的时刻
            // 应用 end_condition (Never/Count/Until)
            // 返回 > now 的最近一次
        }
    }
}
```

### 5.4 不补跑

错过的触发点（休眠/重启）不补跑，跳到下一个未来触发时刻。

### 5.5 立即运行

`run_agenda_item_now` Tauri command：直接构造 occurrence (`trigger_source = ManualRunNow`)，跳过 due 判定调 dispatcher。**不暴露给 agent 工具**，仅 UI 端使用。

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
| `run_agenda_item_now` | `(id: String)` | `String`（occurrence_id，见下注） |
| `skip_occurrence` | `(id: String, at: DateTime<Utc>)` | `AgendaItem` |
| `unskip_occurrence` | `(id: String, at: DateTime<Utc>)` | `AgendaItem` |
| `list_agenda_occurrences` | `(item_id, limit)`（见下注） | `Vec<Occurrence>` |

`ItemFilter` 含 `status_in / persona_id / search`，本期前端默认全部传 None。

**本期签名收窄说明：**

- `run_agenda_item_now` 出参为 `String`（occurrence_id），前端需要完整 `Occurrence` 时再调 `list_agenda_occurrences` 取最新一行。延后到二期再考虑直接返回 `Occurrence`。
- `list_agenda_occurrences` 暂不实现 `before` 游标分页参数，本期仅按 `limit` 取最近 N 条。二期接入分页时再加 `before: Option<DateTime<Utc>>`。

---

## 7. Agent 工具（agent 在对话中可调用）

新增 6 个 RuntimeTool（在 `runtime/tools/builtin/agenda/`）。**所有工具的 owner 范围强制限定为当前 persona**——runtime 注入 `current_persona_id`，工具实现里查询/修改时强制按此过滤。

| 工具 | 用途 |
|---|---|
| `create_agenda_item` | 创建日程，organizer 强制为当前 persona |
| `list_agenda_items` | 查询当前 persona 的日程 |
| `update_agenda_item` | 修改 title / prompt / rule / status |
| `cancel_agenda_item` | 删除（实际调 store delete） |
| `skip_occurrence` | 跳过循环里的某次 |
| `list_agenda_occurrences` | 查执行历史 |

**不暴露**给 agent：`run_agenda_item_now`（员工要立刻做事直接做即可，不用绕日程）。

工具校验：所有传入的 `id` 必须属于当前 persona（organizer == current），否则返回错误。

### 7.1 `create_agenda_item` schema

```json
{
  "title": "string，必填",
  "prompt": "string，必填",
  "start_at": "ISO8601，必填",
  "timezone": "string，可选，默认 Asia/Shanghai",
  "rule": {
    "freq": "daily | weekly | monthly | yearly",
    "interval": 1,
    "end_condition": { "kind": "never" } | { "kind": "count", "n": 10 } | { "kind": "until", "at": "..." }
  }
}
```

`rule` 可省略 = 一次性日程。runtime 强制注入 `organizer_persona_id = current` 和 `participants = [{persona_id: current, joined_at: now}]`，agent 不能传。

---

## 8. 前端改造

### 8.1 范围

UI 形态保持现状（"定时任务"页面），补功能缺陷。**列表布局/分组/过滤本期不动**。

### 8.2 命名变更

- `tauri.ts`：`listSchedules / createSchedule / deleteSchedule` → `listAgendaItems / createAgendaItem / updateAgendaItem / deleteAgendaItem / runAgendaItemNow / skipOccurrence / unskipOccurrence / listAgendaOccurrences`
- `SchedulesPage` 路由名/UI 词不改（仍叫"定时任务"）
- 组件内部类型从 `ScheduleRecord` 改为 `AgendaItem`

### 8.3 列表行（`ScheduleTaskRow`）补齐

每行 hover 后显示 4 个图标按钮：

- **立即运行**（`run_agenda_item_now`）
- **启停切换**（`update_agenda_item` + `status: Active|Paused`）
- **编辑**（打开右侧 Sheet）
- **删除**（保留二次确认）

视觉强分化：

- Active：左侧 2px 蓝色色条 + 下次时间高亮
- Paused：整行灰显 70% opacity + 下次时间留空
- Completed：列表默认过滤掉（"已完成"过滤可见）
- Orphaned：左侧红色色条 + 警示文案"该员工已删除，请处理"
- 即将触发（< 5 min）：呼吸动画

行内增加：

- organizer persona 头像 + 名字（小标签）
- 自然语言频率描述（"每天 9:00"、"5 月 7 日 9:00"），由 `rule + start_at + timezone` 派生

### 8.4 详情面板

新增右侧滑出 Sheet `AgendaItemDetail`，3 个 Tab：

1. **概览**：标题 / organizer / 下次触发倒计时 / 未来 7 天预览（含 skip 标记）
2. **执行历史**：occurrence 列表，每行触发时间 / 状态 / 耗时 / 跳转对应 conversation；跳过单次按钮（仅未来未触发的可跳过）
3. **设置**：编辑 prompt / start_at / rule / status

### 8.5 创建/编辑 Sheet

`AgendaItemEditor` Sheet，分组：

1. **基础**
   - 标题
   - **执行身份**（必选 persona，下拉。创建后只读，仅 Orphaned 可改）
2. **触发时机**
   - 频率：一次性 / 每天 / 每周 / 每月 / 每年
   - 时间选择器（HH:mm 或完整日期时间）
   - interval 数字输入（频率 ≠ 一次性时显示，"每 N 天/周/月/年"）
   - 结束条件：永不 / N 次后 / 到日期（频率 ≠ 一次性时显示）
   - 时区（默认 Asia/Shanghai）
3. **执行内容**
   - prompt 多行编辑器（支持 `/` 引用 skill）

### 8.6 模板系统

3 个硬编码模板降级为"创建 Sheet 顶部的 chip 起点"——点击 chip 预填表单（不直接创建），用户可改后再保存。

### 8.7 Hooks 抽象

`SchedulesPage` 内联的 fetch/refresh 抽到 `src/hooks/useAgendaItems.ts`，方便详情 Sheet 复用。

---

## 9. Persona 删除联动

订阅 persona 删除事件（或在 persona 删除命令里直接调用）：

```rust
self.agenda_store.mark_orphaned_by_organizer(&persona_id)?;
```

实现：扫描 items，将 `organizer_persona_id == deleted_persona_id` 的置为 `Orphaned`。Orphaned item runner 不触发。

UI 上 Orphaned 项显示警示色 + 可改 organizer（重指）—— 这是 organizer "不可转移"约束的唯一例外（仅复活流程）。

---

## 10. 测试

### 10.1 单元测试

- `agenda::store`：CRUD、并发安全、Orphaned 标记、所有约束（5 条）的拒绝路径
- `agenda::trigger_eval`：
  - 一次性 next_fire_at 计算
  - 循环 Daily/Weekly/Monthly/Yearly + 各 interval
  - EndCondition::Never/Count/Until 各分支
  - skip_dates 命中跳过
- `agenda::runner`：take_due 推进、Completed 不再触发、Orphaned 不触发、Paused 不触发

### 10.2 集成测试

- `tests/agenda_commands_test.rs`：list/create/update/delete/run_now/skip/unskip 端到端
- `tests/agenda_runner_scope_test.rs`：scope 切换后 runner 切换 store 路径（修 bug 的回归测试）
- `tests/agenda_persona_delete_test.rs`：persona 删除 → items 转 Orphaned
- `tests/review_agenda_session_id.rs`：触发链路必经 SessionId/RunId（架构约束回归）
- `tests/review_agenda_command_thinness.rs`：transport/tauri_commands/agenda.rs 不含业务逻辑（架构约束回归）
- `tests/review_agenda_phase1_constraints.rs`：本期 5 条严格约束未被破坏（约束回归）

### 10.3 前端测试

- `SchedulesPage.test.tsx` 扩展：编辑、启停、立即运行、删除、Orphaned 状态、Completed 过滤
- `AgendaItemEditor.test.tsx` 新增：频率选择各分支、EndCondition 各分支
- `AgendaItemDetail.test.tsx` 新增：执行历史 Tab、跳过单次按钮

---

## 11. 落地顺序（仅给后续 plan 参考）

1. 数据结构 + Store + 约束校验（带单测）
2. trigger_eval（一次性 + 循环 + skip_dates）
3. Runner + Dispatcher（接入 SessionId/RunId）
4. Tauri commands + 前端 invoke 封装替换
5. 前端列表行/详情 Sheet/编辑 Sheet
6. Agent 工具
7. Persona 删除联动
8. Scope 切换 bug 回归测试

具体 PR 切分由 writing-plans 阶段产出。

---

## 12. 待确认事项

无。所有澄清已对齐。

---

## 附录 A：与现状映射

| 现状 | 新模型 |
|---|---|
| `ScheduleRecord` | `AgendaItem` |
| `ScheduleStatus::Enabled / Disabled` | `ItemStatus::Active / Paused`（多了 Completed / Orphaned） |
| `cron` 字段 | `start_at + Option<RecurrenceRule>` |
| `next_run_at` | `next_fire_at` |
| `human_schedule` | 不持久化，前端按 `rule + start_at` 渲染 |
| 无 | `organizer_persona_id` + `participants: Vec<Participant>` |
| 无 | `Occurrence` 执行历史 |
| 无 | `skip_dates`（本期开放） |
| 无 | `override_of`（二期留口子） |
| `ScheduleStore` | `AgendaStore` |
| `ScheduleRunDispatcher` | `AgendaRunDispatcher`（接入 SessionId/RunId） |

## 附录 B：未做但留口子的扩展点

| 扩展点 | 字段 | 二期用途 |
|---|---|---|
| 多 persona 协同 | `participants` 长度 > 1 | 多人对话协同 |
| 修改循环里的某一次 | `override_of: Option<OverrideRef>` | iCalendar RECURRENCE-ID 等价 |
| 高级循环模式 | `RecurrenceRule.by_day` / `by_month_day` | 每周一三五、每月 N 号 |

二期开放任一扩展点：删除 `store::validate_phase1_constraints` 中对应的约束 + 在 trigger_eval / dispatcher / UI 实现对应行为。旧数据自动可用。

## 附录 C：与 iCalendar 字段对照（备查）

| 我们的字段 | iCalendar | 状态 |
|---|---|---|
| `start_at` | `DTSTART` | ✅ 本期 |
| `timezone` | `TZID` | ✅ 本期 |
| `rule.freq/interval` | `RRULE FREQ/INTERVAL` | ✅ 本期 |
| `rule.end_condition` | `RRULE COUNT/UNTIL` | ✅ 本期 |
| `rule.by_day` | `RRULE BYDAY` | ⏸ 字段建好，本期约束为空 |
| `rule.by_month_day` | `RRULE BYMONTHDAY` | ⏸ 字段建好，本期约束为空 |
| `skip_dates` | `EXDATE` | ✅ 本期开放 |
| `override_of` | `RECURRENCE-ID` | ⏸ 字段建好，本期约束为 None |
| `organizer_persona_id` | `ORGANIZER` | ✅ 本期 |
| `participants` | `ATTENDEE` | ⏸ 字段建好，本期长度为 1 |
| 无 | `DTEND / DURATION` | ❌ 不要（数字员工瞬时执行） |
| 无 | `VALARM` | ❌ 不要（员工不需要被提醒） |
| 无 | `LOCATION / CONFERENCE` | ❌ 不要（不去物理地点） |
| 无 | `STATUS:CANCELLED` | ❌ 不要（删除/暂停已覆盖） |

未来要接 CalDAV 同步时加协议适配层，不需要改基座。
