# rules.md — Agenda 基座意图规格（PR-1 + PR-2）

Agenda 基座是把"定时任务"重做为日程模型：用户创建一个 agenda item（一次性或循环），到点系统自动新建对话发 prompt 给指定 persona 跑完，并把每次执行记成一条 occurrence。

本规格覆盖 PR-1（领域 + Store + trigger_eval）和 PR-2（Runner + Dispatcher + Tauri 命令）落盘的产品承诺。PR-3（前端 Sheet）和 PR-4（Agent 工具 + persona 删除联动）不在本期。

权威 spec：`docs/superpowers/specs/2026-05-06-agenda-base-design.md`。本期签名收窄说明在 spec §6。

---

# 一、领域写入（Store 层）

## 意图 1：合法 agenda item 落盘后能被读回，字段不变

**场景**
用户创建一条最简单的一次性日程，下次启动 / 切 user scope 后还能完整看到。

**前提**
- `AgendaStore::new(dir)`，dir 是 TempDir 路径
- 构造一个 `AgendaItem`：
  - `id = AgendaItemId::new()`
  - `title = "T"`、`prompt = "P"`
  - `organizer_employee_id = "p1"`
  - `participants = vec![Participant { employee_id: "p1", joined_at: now }]`
  - `start_at = now`、`timezone = "Asia/Shanghai"`
  - `rule = None`、`skip_dates = vec![]`
  - `next_fire_at = None`、`occurrence_count = 0`
  - `status = Active`、`override_of = None`
  - `created_at = now`、`updated_at = now`

**操作**
- `store.create(item.clone())`

**断言**
- `create` 返回的 `AgendaItem` 与传入的 `item` 字段全等（`PartialEq`）
- `dir/agenda/items/{item.id}.json` 文件存在
- 反序列化文件内容得到的 `AgendaItem` 与传入的 `item` 字段全等

---

## 意图 2：participants 长度不为 1 时拒���写入

**场景**
本期不开放多 persona 协作，store 必须挡住违反约束的写入。

**前提**
- 合法 base item（`organizer_employee_id = "p1"`），但 `participants` 追加一个 `Participant { employee_id: "p2", joined_at: now }`，使长度为 2

**操作**
- `store.create(item)`

**断言**
- 返回 `Err`
- 错误信息包含字符串 `"participants"`
- `dir/agenda/items/` 下无任何文件

---

## 意图 3：organizer 不在 participants[0] 时拒绝写入

**前提**
- 合法 base item，`organizer_employee_id = "p1"`，但 `participants[0].employee_id = "other"`（与 organizer 不一致），长度仍为 1

**操作**
- `store.create(item)`

**断言**
- 返回 `Err`
- 错误信息包含字符串 `"organizer"`

---

## 意图 4：override_of 非空时拒绝写入

**场景**
override_of 是二期"循环单次例外"留的口子，本期必须为 None。

**前提**
- 合法 base item，但 `override_of = Some(OverrideRef { series_item_id: AgendaItemId("agenda-x"), original_at: now })`

**操作**
- `store.create(item)`

**断言**
- 返回 `Err`
- 错误信息包含字符串 `"override_of"`

---

## 意图 5：循环 rule.by_day 非空时拒绝写入

**前提**
- 合法 base item，但 `rule = Some(RecurrenceRule { freq: Weekly, interval: 1, end_condition: Never, by_day: vec![Weekday::Mon], by_month_day: vec![] })`

**操作**
- `store.create(item)`

**断言**
- 返回 `Err`
- 错误信息包含字符串 `"by_day"`

---

## 意图 6：循环 rule.by_month_day 非空时拒绝写入

**前提**
- 合法 base item，但 `rule = Some(RecurrenceRule { freq: Monthly, interval: 1, end_condition: Never, by_day: vec![], by_month_day: vec![7] })`

**操作**
- `store.create(item)`

**断言**
- 返回 `Err`
- 错误信息包含字符串 `"by_month_day"`

---

## 意图 7：一次性 item 设了 skip_dates 时拒绝写入

**场景**
skip_dates 仅对循环有意义，一次性日程不能跳过自己。

**前提**
- 合法 base item，`rule = None`，但 `skip_dates = vec![now]`

**操作**
- `store.create(item)`

**断言**
- 返回 `Err`
- 错误信息包含字符串 `"skip_dates"`

---

## 意图 8：update 时 organizer 不可改（除非 status 是 Orphaned）

**场景**
活跃 / 暂停 / 已完成的日程，organizer_employee_id 是不变量；只有 organizer 被删除导致日程进入 Orphaned 后，用户重新指派 organizer 才允许改。

**前提**
- 合法 base item（`organizer_employee_id = "p1"`、`status = Active`）已 `create`
- 拷贝 saved，把 `organizer_employee_id` 改为 `"p2"`，`participants` 同步改为 `[Participant { employee_id: "p2", joined_at: now }]`

**操作**
- `store.update(modified)`

**断言**
- 返回 `Err`
- 错误信息包含字符串 `"organizer"`
- 磁盘上文件仍是原 organizer `"p1"`

---

## 意图 9：update 在 status 是 Orphaned 时允许改 organizer

**前提**
- 合法 base item（`organizer_employee_id = "p1"`）已 `create`
- 把 saved 的 `status` 改为 `Orphaned` 并 `update` 一次（持久化 Orphaned）
- 再拷贝一份，`organizer_employee_id = "p2"`、`participants[0].employee_id = "p2"`、`status = Active`

**操作**
- `store.update(revived)`

**断言**
- 返回 `Ok(updated)`
- `updated.organizer_employee_id == "p2"`
- `updated.status == Active`

---

## 意图 10：item id 含路径穿越字符时拒绝读 / 写 / 删 / 追加 occurrence

**场景**
item id 直接拼进文件路径，必须挡住 `..`、`/`、`\` 等字符，避免越权写到 agenda 目录之外。

**前提**
- 在 `store.root` 下事先放一个 `outside.json`（用于检验 traversal 触达后会不会被改）
- `unsafe_id = AgendaItemId("../outside")`

**操作 + 断言**（每个动作一条独立断言）
- `store.get(&unsafe_id)`：返回 `Err`，错误信息包含 `"invalid agenda item id"`，`outside.json` 仍存在
- `store.create(item with id = unsafe_id)`：返回 `Err`，错误信息包含 `"invalid agenda item id"`，`store.root/outside.json` 不被改写
- `store.delete(&unsafe_id)`：返回 `Err`，错误信息包含 `"invalid agenda item id"`，`outside.json` 仍存在
- `store.append_occurrence(&Occurrence { agenda_item_id: unsafe_id, … })`：返回 `Err`，错误信息包含 `"invalid agenda item id"`，`store.root/outside/` 目录不被创建
- `store.list_occurrences(&unsafe_id, 10)`：返回 `Err`，错误信息包含 `"invalid agenda item id"`

---

## 意图 11：occurrence 写入按 yyyy-mm 分片，同 id 多次追加只读最后一条

**场景**
一次执行先写 `Running`、跑完再写 `Succeeded`，前端只该看到最终态。

**前提**
- 合法 base item 已 `create`
- 构造 `Occurrence`：
  - `id = "occ-fixed-1"`（手写定值，不用 `Occurrence::new_id()`）
  - `agenda_item_id = item.id`
  - `fired_at = 2026-05-07T01:02:03Z`
  - `planned_fire_at = fired_at`、`started_at = fired_at`、`finished_at = None`
  - `primary_employee_id = "p1"`、`conversation_id = "conv-1"`
  - `session_id = SessionId::new("conv-1")`、`run_id = RunId::new("run-1")`
  - `status = Running`、`error_summary = None`、`trigger_source = Scheduled`

**操作**
- `store.append_occurrence(&running)`
- 拷贝 running，`status = Succeeded`、`finished_at = Some(now)`，`store.append_occurrence(&completed)`

**断言**
- `dir/agenda/occurrences/{item.id}/2026-05.jsonl` 文件存在
- `store.list_occurrences(&item.id, 10)` 返回长度 1 的 Vec
- 该 Occurrence 的 `id == "occ-fixed-1"`、`status == Succeeded`、`finished_at.is_some()`

---

## 意图 12：mark_orphaned_by_organizer 只翻 Active/Paused，跳过 Completed/Orphaned

**场景**
用户删除一个 persona 时，把所有以这个 persona 为 organizer 的活跃日程转 Orphaned；已完成 / 已 Orphaned 的不动。

**前提**
- 合法 base item A（`organizer = "alice"`、`status = Active`）已 `create`
- 合法 base item B（`organizer = "bob"`、`status = Active`）已 `create`
- 合法 base item C（`organizer = "alice"`、`status = Completed`）已 `create`

**操作**
- `store.mark_orphaned_by_organizer("alice")`

**断言**
- 返回 `Ok(1)`（只有 A 被翻）
- `store.get(&A.id).unwrap().status == Orphaned`
- `store.get(&B.id).unwrap().status == Active`
- `store.get(&C.id).unwrap().status == Completed`

---

# 二、触发计算（trigger_eval）

## 意图 13：一次性 item 在 start_at 未到时，next_fire_at = start_at

**前提**
- `now = 2026-05-07T08:00:00Z`
- `item.start_at = 2026-05-07T09:00:00Z`、`rule = None`、`occurrence_count = 0`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-07T09:00:00Z)`

---

## 意图 14：一次性 item 在 start_at 等于 now 时，next_fire_at = start_at

**场景**
spec §5.3 写 `>`，但本期实现裁剪为 `>=`，等值仍允许触发以避免擦肩而过。

**前提**
- `now = item.start_at = 2026-05-07T09:00:00Z`，`rule = None`、`occurrence_count = 0`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-07T09:00:00Z)`

---

## 意图 15：一次性 item 在 start_at 已过时，next_fire_at = None

**前提**
- `now = 2026-05-07T10:00:00Z`、`item.start_at = 2026-05-07T09:00:00Z`、`rule = None`、`occurrence_count = 0`

**断言**
- `compute_next_fire_at(&item, now) == None`

---

## 意图 16：一次性 item 已触发过（occurrence_count = 1）时，next_fire_at = None

**前提**
- `now = 2026-05-07T08:00:00Z`、`start_at = 2026-05-07T09:00:00Z`、`rule = None`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == None`

---

## 意图 17：Daily 循环返回未来第一个时刻

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-08T12:00:00Z`
- `rule = Daily, interval=1, end_condition=Never, by_day=[], by_month_day=[]`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-09T09:00:00Z)`

---

## 意图 18：Daily interval=2 跳过中间日

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-08T00:00:00Z`
- `rule = Daily, interval=2, end_condition=Never, by_day=[], by_month_day=[]`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-09T09:00:00Z)`

---

## 意图 19：Weekly 循环步进 7 天

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-08T00:00:00Z`
- `rule = Weekly, interval=1, end_condition=Never, by_day=[], by_month_day=[]`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-14T09:00:00Z)`

---

## 意图 20：Monthly 循环步进 1 个月

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-08T00:00:00Z`
- `rule = Monthly, interval=1, end_condition=Never, by_day=[], by_month_day=[]`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-06-07T09:00:00Z)`

---

## 意图 21：Yearly 循环步进 1 年

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-08T00:00:00Z`
- `rule = Yearly, interval=1, end_condition=Never, by_day=[], by_month_day=[]`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == Some(2027-05-07T09:00:00Z)`

---

## 意图 22：Yearly 循环遇到 2/29 时跳到下一个闰年

**场景**
2024-02-29 → 下一个 yearly 触发应该是 2028-02-29（2025/2026/2027 不是闰年，跳过）。

**前提**
- `start = 2024-02-29T09:00:00Z`、`now = 2024-03-01T00:00:00Z`
- `rule = Yearly, interval=1, end_condition=Never, by_day=[], by_month_day=[]`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == Some(2028-02-29T09:00:00Z)`

---

## 意图 23：长间隔 Daily catch-up 不会卡死，返回紧邻 now 的下一个时刻

**场景**
用户设了 1990 年开始的循环但今天才打开。系统不能 36 年逐天 advance。

**前提**
- `start = 1990-01-01T09:00:00Z`、`now = 2026-05-07T12:00:00Z`
- `rule = Daily, interval=1, end_condition=Never, by_day=[], by_month_day=[]`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-08T09:00:00Z)`
- 计算应在毫秒级返回（不是逐天循环）

---

## 意图 24：EndCondition::Count 在 occurrence_count 达 N 后返回 None

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-09T00:00:00Z`
- `rule = Daily, interval=1, end_condition=Count { n: 3 }`、`occurrence_count = 3`

**断言**
- `compute_next_fire_at(&item, now) == None`

---

## 意图 25：EndCondition::Count 在 occurrence_count < N 时仍返回未来时刻

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-08T00:00:00Z`
- `rule = Daily, interval=1, end_condition=Count { n: 3 }`、`occurrence_count = 1`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-08T09:00:00Z)`

---

## 意图 26：EndCondition::Count 不消耗错过的时间槽

**场景**
用户离线 4 天再打开（错过 5/8、5/9、5/10），不应该把这 3 个错过的算进 Count。

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-10T12:00:00Z`
- `rule = Daily, interval=1, end_condition=Count { n: 3 }`、`occurrence_count = 1`（只有 5/7 真触发过）

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-11T09:00:00Z)`（不是 None；count 按 actual fires 计）

---

## 意图 27：EndCondition::Until 在 now 超过 until 时���回 None

**前提**
- `start = 2026-05-07T09:00:00Z`、`until = 2026-05-09T00:00:00Z`、`now = 2026-05-09T12:00:00Z`
- `rule = Daily, interval=1, end_condition=Until { at: until }`、`occurrence_count = 2`

**断言**
- `compute_next_fire_at(&item, now) == None`

---

## 意图 28：skip_dates 命中时跳到下一个时刻

**前提**
- `start = 2026-05-07T09:00:00Z`、`now = 2026-05-07T12:00:00Z`
- `rule = Daily, interval=1, end_condition=Never, by_day=[], by_month_day=[]`、`occurrence_count = 1`
- `skip_dates = vec![2026-05-08T09:00:00Z]`

**断言**
- `compute_next_fire_at(&item, now) == Some(2026-05-09T09:00:00Z)`（5/8 跳过）

---

# 三、触发推进（Store::take_due / advance_after_fire / set_skip）

## 意图 29：take_due 只返回 Active 且 next_fire_at <= now 且 override_of = None 且通过本期 5 约束的 item

**前提**
- TempDir store 上 `create`：
  - itemA：`status=Active`、`next_fire_at = 2026-05-07T08:00:00Z`
  - （在意图 30 单独覆盖 Paused/Completed/Orphaned 拒绝路径）
- `now = 2026-05-07T09:00:00Z`

**操作**
- `store.take_due(now)`

**断言**
- 返回长度 1 的 Vec，元素 id 与 itemA.id 相等

---

## 意图 30：take_due 跳过 Paused / Completed / Orphaned 的 item

**前提**
- TempDir store 上分别 `create` 三条 item，next_fire_at 同为 `2026-05-07T08:00:00Z`，status 分别为 `Paused` / `Completed` / `Orphaned`
- `now = 2026-05-07T09:00:00Z`

**操作**
- `store.take_due(now)`

**断言**
- 返回长度 0 的 Vec

---

## 意图 31：advance_after_fire 推进 occurrence_count，并按规则重算 next_fire_at

**前提**
- `start = 2026-05-07T09:00:00Z`，base item：`start_at = start`、`next_fire_at = Some(start)`、`occurrence_count = 0`、`status = Active`
- `rule = Daily, interval=1, end_condition=Never, by_day=[], by_month_day=[]`
- `create` 落盘
- `now = 2026-05-07T09:00:01Z`

**操作**
- `store.advance_after_fire(&item.id, now)`

**断言**
- 返回 `Ok(updated)`
- `updated.occurrence_count == 1`
- `updated.next_fire_at == Some(2026-05-08T09:00:00Z)`
- `updated.status == Active`

---

## 意图 32：advance_after_fire 在一次性 item 上把 status 翻为 Completed

**前提**
- `start = 2026-05-07T09:00:00Z`，base item：`rule = None`、`start_at = start`、`next_fire_at = Some(start)`、`occurrence_count = 0`
- `now = 2026-05-07T09:00:01Z`

**操作**
- `store.advance_after_fire(&item.id, now)`

**断言**
- `updated.occurrence_count == 1`
- `updated.next_fire_at == None`
- `updated.status == Completed`

---

## 意图 33：advance_after_fire 在非 Active 状态拒绝并不改任何字段

**场景**
runner 拿到 due 后 advance 之前如果 status 已经被外部改成 Paused，必须挡住，不能错把 Paused 当 Active 推进。

**前提**
- base item：`status = Paused`、`next_fire_at = Some(start)`、`occurrence_count = 0`
- `now = start + 1s`

**操作**
- `store.advance_after_fire(&item.id, now)`

**断言**
- 返回 `Err`，错误信息包含字符串 `"not active"`
- `store.get(&item.id).unwrap()`：`occurrence_count == 0`、`next_fire_at == Some(start)`、`status == Paused`

---

## 意图 34：set_skip 只在 rule.is_some 时允许

**前提**
- 合法一次性 item（`rule = None`）已 `create`
- `at = Utc::now()`

**操作**
- `store.set_skip(&item.id, at)`

**断言**
- 返回 `Err`，错误信息包含字符串 `"rule"`

---

## 意图 35：set_skip 把 at 加入 skip_dates 并重算 next_fire_at

**前提**
- 合法循环 item（`rule = Daily, interval=1, end_condition=Never, by_day=[], by_month_day=[]`、`start_at = now`）已 `create`
- `target = 2026-05-08T09:00:00Z`

**操作**
- `store.set_skip(&item.id, target)`

**断言**
- `updated.skip_dates.contains(&target) == true`
- `unset_skip(&item.id, target)` 后 `skip_dates.contains(&target) == false`

---

# 四、Runner（每 tick 重新 resolve scope + 派发）

## 意图 36：runner 每个 tick 都 re-resolve scope，不缓存 store

**场景**
用户切了 user scope（多用户切换），下一 tick 应该用新 scope 的 agenda 目录，不能继续用旧 scope。

**前提**
- 阅读 `src-tauri/src/runtime/agenda/runner.rs` 源文件

**断言**（源码结构断言，不跑代码）
- 文件包含字符串 `"path_resolver.resolve_paths()"`
- 调用 `resolve_paths()` 的行号 > `loop {` 行号
- 调用 `AgendaStore::new(paths.base_dir())` 的行号 > `loop {` 行号
- `loop {` 之前的所有行都不包含 `"AgendaStore::new("`

---

## 意图 37：run_due_once 把 due 的 item 派发给 dispatcher，且 trigger_source = Scheduled

**前提**
- `now = 2026-05-07T09:00:00Z`、`due_at = 2026-05-07T08:00:00Z`
- TempDir AgendaStore 上 `create` 一条合法 base item（`status = Active`、`next_fire_at = Some(due_at)`、`organizer = "p1"`）
- `RecordingDispatcher`：每次 `dispatch` 把 `(item.id.as_str().to_string(), trigger_source)` 推入 `Vec`

**操作**
- `run_due_once(&store, &dispatcher, now).await`

**断言**
- 返回 `Ok(())`
- `dispatcher.calls.len() == 1`
- 第一条记录的 item id 等于 created item 的 id
- 第一条记录的 trigger_source 序列化值为 `"scheduled"`

---

## 意图 38：run_due_once 在没有 due item 时不调 dispatcher

**前提**
- 空 TempDir AgendaStore
- `RecordingDispatcher` 同上

**操作**
- `run_due_once(&store, &dispatcher, Utc::now()).await`

**断言**
- 返回 `Ok(())`
- `dispatcher.calls.len() == 0`

---

# 五、Tauri 命令层（薄转发 + 输入校验）

## 意图 39：transport/tauri_commands/agenda.rs 每个 #[tauri::command] 函数体不超过 30 行

**场景**
CLAUDE.md 要求 transport 层只做参数接收 → 转发 runtime。本意图通过源码扫描锁住此约束。

**前提**
- 阅读 `src-tauri/src/transport/tauri_commands/agenda.rs` 源文件

**断言**（源码结构断言，不跑代码）
- 文件中每个 `#[tauri::command]` 标记的 `pub async fn` 函数体（从函数签名行后到下一个起始 `}` 之间，不算签名行和闭合行）行数 < 30

---

## 意图 40：create_agenda_item 接收的 title trim 后为空时返回错误

**前提**
- `CreateAgendaItemRequest { title: "   ", prompt: "Prompt", organizer_employee_id: "p1", start_at: 2026-05-07T09:00:00Z, timezone: None, rule: None }`
- `now = 2026-05-07T08:00:00Z`

**操作**
- `build_agenda_item_from_create_request(request, now)`

**断言**
- 返回 `Err`
- 错误信息字面值等于字符串 `"title is required"`

---

## 意图 41：create_agenda_item 接收的 prompt trim 后为空时返回错误

**前提**
- `CreateAgendaItemRequest { title: "Title", prompt: "   ", organizer_employee_id: "p1", start_at: 2026-05-07T09:00:00Z, timezone: None, rule: None }`
- `now = 2026-05-07T08:00:00Z`

**操作**
- `build_agenda_item_from_create_request(request, now)`

**断言**
- 错误信息字面值等于字符串 `"prompt is required"`

---

## 意图 42：create_agenda_item 接收的 organizer_employee_id trim 后为空时返回错误

**前提**
- `CreateAgendaItemRequest { title: "Title", prompt: "Prompt", organizer_employee_id: "   ", start_at: 2026-05-07T09:00:00Z, timezone: None, rule: None }`
- `now = 2026-05-07T08:00:00Z`

**操作**
- `build_agenda_item_from_create_request(request, now)`

**断言**
- 错误信息字面值等于字符串 `"organizer_employee_id is required"`

---

## 意图 43：create_agenda_item 的 timezone 不是有效 IANA 时区时返回错误

**前提**
- `CreateAgendaItemRequest { title: "Title", prompt: "Prompt", organizer_employee_id: "p1", start_at: 2026-05-07T09:00:00Z, timezone: Some("Not/AZone"), rule: None }`
- `now = 2026-05-07T08:00:00Z`

**操作**
- `build_agenda_item_from_create_request(request, now)`

**断言**
- 错误信息字面值等于字符串 `"timezone must be a valid IANA timezone"`

---

## 意图 44：create_agenda_item 在 timezone 为空白或缺失时默认使用 "Asia/Shanghai"

**前提**
- `CreateAgendaItemRequest { title: "  Standup  ", prompt: "  Discuss blockers  ", organizer_employee_id: " persona-1 ", start_at: 2026-05-07T09:00:00Z, timezone: Some("   "), rule: None }`
- `now = 2026-05-07T08:00:00Z`

**操作**
- `build_agenda_item_from_create_request(request, now)`

**断言**
- 返回 `Ok(item)`
- `item.title == "Standup"`（trim）
- `item.prompt == "Discuss blockers"`（trim）
- `item.organizer_employee_id == "persona-1"`（trim）
- `item.participants[0].employee_id == "persona-1"`
- `item.timezone == "Asia/Shanghai"`

---

## 意图 45：update_agenda_item 字段为 None 时不动对应字段，trim 后为空字符串时拒绝

**前提**
- 已存在 base item（`title = "Old"`、`prompt = "Old prompt"`、`organizer = "p1"`、`status = Active`、`rule = None`）
- `now = 2026-05-07T08:00:00Z`

**操作 + 断言**（每个动作一条独立断言）
- `apply_update(item, UpdateAgendaItemRequest { title: Some("  New title  "), prompt: Some("  New prompt  "), start_at: Some(2026-05-07T10:00:00Z), timezone: Some("  UTC  "), rule: Some(None), status: Some(Paused) }, now)`：返回 `Ok(updated)`，`updated.title == "New title"`、`updated.prompt == "New prompt"`、`updated.timezone == "UTC"`、`updated.status == Paused`、`updated.organizer_employee_id == "p1"`（不变）、`updated.participants[0].employee_id == "p1"`（不变）、`updated.updated_at == now`、`updated.next_fire_at == Some(updated.start_at)`
- `apply_update(item, UpdateAgendaItemRequest { title: Some("   "), ..Default::default() }, now)`：错误信息字面值 `"title is required"`
- `apply_update(item, UpdateAgendaItemRequest { prompt: Some("   "), ..Default::default() }, now)`：错误信息字面值 `"prompt is required"`
- `apply_update(item, UpdateAgendaItemRequest { timezone: Some("   "), ..Default::default() }, now)`：错误信息字面值 `"timezone is required"`
- `apply_update(item, UpdateAgendaItemRequest { timezone: Some("Not/AZone"), ..Default::default() }, now)`：错误信息字面值 `"timezone must be a valid IANA timezone"`

---

## 意图 46：update_agenda_item 入参 rule:null 与 rule 缺失语义不同

**场景**
前端"清空循环规则"传 `rule: null`，"不动 rule"传 `rule` 字段缺失。这两种语义在 IPC 层必须区分。

**前提**
- 两个 JSON：
  - `{"rule": null}`
  - `{"title": "T"}`（不含 rule key）

**操作**
- `serde_json::from_value::<UpdateAgendaItemRequest>(json)`

**断言**
- `{"rule": null}` 反序列化得到的 `request.rule` 字面匹配 `Some(None)`（外层 Some 表示"用户传了"，内层 None 表示"清空"）
- `{"title": "T"}` 反序列化得到的 `request.rule` 字面匹配 `None`（不传等于不动）

---

# 六、签名收窄（spec §6 vs 实现）

## 意图 47：run_agenda_item_now 出参为 occurrence_id 字符串，前端要拿完整 Occurrence 需另调 list_agenda_occurrences

**场景**
spec §6 原写出参 `Occurrence`，本期裁剪为 `String`。前端类型与后端命令出参必须保持一致。

**前提**
- 阅读 `src/lib/tauri.ts` 中的 `runAgendaItemNow` 类型签名
- 阅读 `src-tauri/src/transport/tauri_commands/agenda.rs` 中 `run_agenda_item_now` 的返回类型

**断言**（源码结构断言）
- TS 端 `runAgendaItemNow` 的返回类型字面包含 `Promise<string>`
- Rust 端 `run_agenda_item_now` 的函数签名包含 `-> Result<String, String>`

---

## 意图 48：list_agenda_occurrences 入参为 (item_id, limit)，不接 before 游标

**前提**
- 阅读 `src/lib/tauri.ts` 中的 `listAgendaOccurrences` 函数签名
- 阅读 `src-tauri/src/transport/tauri_commands/agenda.rs` 中 `list_agenda_occurrences` 的参数列表

**断言**
- TS 端签名字面匹配 `listAgendaOccurrences(itemId: string, limit?: number)`，参数列表中没有 `before`
- Rust 端 `#[tauri::command] pub async fn list_agenda_occurrences` 的参数只有 `item_id: String, limit: Option<usize>, resolver: State<...>`，没有 `before`

---

# 七、本期不覆盖（PR-3 / PR-4 接入后再补）

以下产品承诺已在 spec 中存在但本期实现不到位 / 测试未覆盖，PR-4 完成后回填：

- ⏳ 触发链路 SessionId / RunId 的端到端事件锁（spec §10.2 `review_agenda_session_id.rs`）
- ⏳ Persona 删除后 `mark_orphaned_by_organizer` 被自动调用（spec §9，PR-4 任务 54-55）
- ⏳ Runner 切换 scope 的端到端集成测试（spec §10.2 `agenda_runner_scope_test.rs`，PR-4 任务 56）
- ⏳ Agent 工具层 6 个 RuntimeTool 的 owner = current_persona 强制（spec §7，PR-4 任务 45-53）
- ⏳ 前端 Editor / Detail Sheet 的 UI 行为（spec §8，PR-3 任务 33-44）
- ⏳ MessageNormalizer 从 `/skill-id ...` 文本派生 `skillCommand` 的回归测试（PR-2 收尾删了对应后端单测，前端 normalize 行为目前无测试锁，详见 plan F2 B 组 follow-up TODO）
