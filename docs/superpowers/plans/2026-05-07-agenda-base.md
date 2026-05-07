# Agenda 基座实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把现有"定时任务"重做成 `agenda` 基座（`AgendaItem + Occurrence`），为日程铺路；前端 UI 形态不变但补齐功能；新增 6 个 agent 工具让数字员工自管日程。

**Architecture:** 后端新增 `runtime/agenda/`（item / occurrence / store / trigger_eval / runner / dispatcher），用 `start_at + Option<RecurrenceRule>` 替代 cron 表达式（向 iCalendar 形状靠拢）；持久化跟随用户 scope 在 `~/.renlijia/users/{scope}/agenda/`。Persona 切换走 per-turn `persona_id_override`（避免全局 `set_active_persona` 的并发竞态）。Agent 工具按 `current_persona_id` 强制过滤，5 条本期约束在 store / runner / review test 三道防线锁住。

**Tech Stack:** Rust（tokio / async-trait / chrono / chrono-tz / serde / tempfile）、Tauri 2.x、React + TypeScript（Zustand / vitest）、Vite。

**关键设计决策（基于调研）：**

1. **Persona 切换**：扩展 `ChatTurnRequest` 加 `persona_id_override: Option<String>`，`build_system_prompt` 优先用此字段查 persona；不用全局 `set_active_persona`（多 dispatcher 并发会写同一份 index.json）。
2. **SessionId/RunId 注入**：`SessionId = conversation_id`（已有 `From<String>` 转换）；`RunId` 由 dispatcher 显式 `RunId::new(uuid)` 创建后传入新增的 `send_message_with_run_id` 变体（避免 ChatTurnRequest 内部默认生成导致 dispatcher 拿不到值）。
3. **Scope 切换 bug**��现有 `schedule_runner` 已每 tick 重 resolve（spec 描述的 bug 实际不存在），新 `agenda_runner` 保持此模式 + `review_agenda_runner_scope.rs` 锁住。
4. **Conversation 反向指向 agenda**：不加字段；Occurrence 已有 `conversation_id`，单向引用够用。
5. **老 `schedules/` 不迁移**：启动时若发现非空打 info log，不读不写。
6. **`AgendaItem.timezone` 真正生效**：引入 `chrono-tz` crate，`trigger_eval` 用 IANA 字符串（"Asia/Shanghai"）按本地墙钟时间计算 next_fire_at；现有 `ScheduleRecord.timezone` 是假字段（只存不用）。

**PR 切分（4 段，每段独立 ship 可工作）：**

- **PR-1：领域 + Store + trigger_eval**（任务 1-15）— 纯后端、纯单测，不涉及任何运行时改动
- **PR-2：Runner + Dispatcher + Tauri 命令 + 前端 invoke 替换**（任务 16-32）— 后端可触发 + 前端可调用（UI 不动）
- **PR-3：前端 Sheet + hooks**（任务 33-44）— 编辑/详情 Sheet + 列表行补齐
- **PR-4：Agent 工具 + Persona 删除联动 + review tests + 删旧代码**（任务 45-58）— 收尾

---

## 文件结构

### 新增

```
src-tauri/src/runtime/agenda/
├── mod.rs                       # pub use 子模块
├── item.rs                      # AgendaItem / Participant / RecurrenceRule / OverrideRef / 枚举
├── occurrence.rs                # Occurrence / OccurrenceStatus / TriggerSource
├── store.rs                     # AgendaStore: CRUD + 5 条约束 + Mutex 锁
├── trigger_eval.rs              # compute_next_fire_at(item, now) 纯函数
├── runner.rs                    # spawn_agenda_runner + run_due_once
└── dispatcher.rs                # AgendaRunDispatcher trait

src-tauri/src/runtime/tools/builtin/agenda/
├── mod.rs                       # pub use
├── deps.rs                      # AgendaToolDeps（base_dir + current_persona_id）
├── create.rs                    # CreateAgendaItemRuntimeTool
├── list.rs                      # ListAgendaItemsRuntimeTool
├── update.rs                    # UpdateAgendaItemRuntimeTool
├── cancel.rs                    # CancelAgendaItemRuntimeTool
├── skip.rs                      # SkipOccurrenceRuntimeTool
└── list_occurrences.rs          # ListAgendaOccurrencesRuntimeTool

src-tauri/src/transport/tauri_commands/agenda.rs   # 9 个 Tauri 命令薄转发

src-tauri/tests/
├── agenda_commands_test.rs                  # CRUD + run_now + skip 端到端
├── agenda_runner_scope_test.rs              # scope 切换后 runner 切 store
├── agenda_persona_delete_test.rs            # persona 删除 → Orphaned
├── review_agenda_session_id.rs              # 触发链路必经 SessionId/RunId
├── review_agenda_command_thinness.rs        # transport 层薄转发
├── review_agenda_phase1_constraints.rs      # 本期 5 条约束
└── review_agenda_runner_scope.rs            # 每 tick 重 resolve 模式

src/features/agenda/
├── AgendaItemEditor.tsx                     # 创建/编辑 Sheet
├── AgendaItemDetail.tsx                     # 详情面板（3 Tab）
├── AgendaItemEditor.test.tsx
└── AgendaItemDetail.test.tsx

src/hooks/useAgendaItems.ts                  # fetch/refresh + 单 item 查询
```

### 修改

```
src-tauri/Cargo.toml                         # + chrono-tz 0.9
src-tauri/src/runtime/mod.rs                 # + pub mod agenda
src-tauri/src/runtime/tools/builtin/mod.rs   # + pub mod agenda
src-tauri/src/runtime/tools/capability.rs    # + current_persona_id 字段
src-tauri/src/runtime/tools/catalog.rs       # + 6 个 agenda 工具 CatalogEntry + DAILY_ALLOWED_TOOLS
src-tauri/src/plugin/registry.rs             # + try_build_request_scoped_tool 里按 name 注入 AgendaToolDeps
src-tauri/src/runtime/chat/chat_turn_driver.rs   # ChatTurnRequest + persona_id_override
src-tauri/src/transport/tauri_commands/chat.rs   # build_system_prompt 优先用 persona_id_override
src-tauri/src/storage/user_scoped_paths.rs   # + agenda_dir() 方法
src-tauri/src/storage/aijia_home.rs          # ensure_user_dirs 加 agenda 子目录
src-tauri/src/lib.rs                         # spawn_schedule_runner → spawn_agenda_runner，invoke handler 替换
src-tauri/src/commands/persona.rs            # delete_persona 后调 mark_orphaned_by_organizer
src-tauri/src/commands/mod.rs                # 删 mod schedules
src-tauri/src/transport/tauri_commands/mod.rs    # + pub mod agenda

src/lib/tauri.ts                             # 替换 schedule 封装为 agenda（9 个 invoke + 类型）
src/features/schedules/SchedulesPage.tsx     # 接 useAgendaItems hook + 4 个行内按钮 + AgendaItemEditor/Detail
src/features/schedules/SchedulesPage.test.tsx
src/components/schedules/ScheduleTaskRow.tsx # 4 个 hover 按钮、状态色条、organizer 头像、自然语言频率
src/components/schedules/ScheduleTemplateCard.tsx   # 改为预填表单不直接创建
```

### 删除（PR-4 收尾）

```
src-tauri/src/runtime/schedule.rs
src-tauri/src/runtime/schedule_runner.rs
src-tauri/src/commands/schedules.rs
src-tauri/tests/schedule_commands_test.rs
```

---

## Spec 自检（写计划时一并修）

- spec §2 子标题 `### 1.2 / 1.3 / 1.4` 漏改编号，正确为 `2.2 / 2.3 / 2.4`（章节交换遗留）。**任务 1 一并修**。
- spec §10.1 写"约束（10 条）"实际是 §1.9 的 5 条。**任务 1 一并修**。

---

# PR-1：领域 + Store + trigger_eval

> 这一段产出纯后端类型层 + 文件持久化 + 触发时间计算，全部用 `tempfile::TempDir` 单测覆盖，不接 runtime / Tauri / 前端。
> 完成后这一段代码独立可工作（虽然没人调用），CI 能跑通。

## 任务 1：修 spec 笔误

**Files:**
- Modify: `docs/superpowers/specs/2026-05-06-agenda-base-design.md`

- [ ] **Step 1：修 §2 子标题编号**

把以下三处替换：

```
### 1.2 产品诉求   →   ### 2.2 产品诉求
### 1.3 本期目标   →   ### 2.3 本期目标
### 1.4 非目标     →   ### 2.4 非目标
```

- [ ] **Step 2：修 §10.1 文案**

```
- `agenda::store`：CRUD、并发安全、Orphaned 标记、所有约束（10 条）的拒绝路径
```
改为：
```
- `agenda::store`：CRUD、并发安全、Orphaned 标记、所有约束（5 条）的拒绝路径
```

- [ ] **Step 3：Commit**

```bash
git add docs/superpowers/specs/2026-05-06-agenda-base-design.md
git commit -m "docs(agenda): fix section numbering and constraint count"
```

---

## 任务 2：引入 chrono-tz 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1：在 chrono 同行下方加 chrono-tz**

定位到 `chrono = { version = "0.4", features = ["serde"] }` 这一行，下方追加：

```toml
chrono-tz = "0.9"
```

- [ ] **Step 2：验证编译**

```bash
cd src-tauri && cargo check
```
预期：依赖下载 + 编译通过，无 error。

- [ ] **Step 3：Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "build(agenda): add chrono-tz 0.9 for IANA timezone support"
```

---

## 任务 3：新增 `UserScopedPaths::agenda_dir()` + 创建目录

**Files:**
- Modify: `src-tauri/src/storage/user_scoped_paths.rs`
- Modify: `src-tauri/src/storage/aijia_home.rs`
- Test: `src-tauri/src/storage/user_scoped_paths.rs`（同文件 #[cfg(test)]）
- Test: `src-tauri/src/storage/aijia_home.rs`（同文件 #[cfg(test)]）

- [ ] **Step 1：写失败的测试**

在 `user_scoped_paths.rs` 末尾的 `#[cfg(test)] mod tests` 里加（如果没有 mod 就新建）：

```rust
#[test]
fn agenda_dir_under_base() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let paths = UserScopedPaths::new(dir.path(), "t_1__u_2");
    assert_eq!(paths.agenda_dir(), dir.path().join("users/t_1__u_2/agenda"));
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib storage::user_scoped_paths::tests::agenda_dir_under_base -- --nocapture
```
预期：FAIL `no method named agenda_dir`。

- [ ] **Step 3：实现 `agenda_dir()`**

在 `UserScopedPaths` 的 `impl` 块里，紧挨 `schedules_dir` 方法添加：

```rust
pub fn agenda_dir(&self) -> std::path::PathBuf {
    self.base.join("agenda")
}
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib storage::user_scoped_paths::tests::agenda_dir_under_base
```
预期：PASS。

- [ ] **Step 5：在 `ensure_user_dirs` 里创建子目录**

打开 `src-tauri/src/storage/aijia_home.rs`，找到 `ensure_user_dirs` 函数（grep `ensure_user_dirs`），在创建 `schedules` 目录的同段代码下方追加：

```rust
let agenda_dir = self.user_dir(scope).join("agenda");
std::fs::create_dir_all(agenda_dir.join("items"))?;
std::fs::create_dir_all(agenda_dir.join("occurrences"))?;
```

- [ ] **Step 6：补 `ensure_user_dirs` 回归断言（质量审查修正）**

在 `test_ensure_user_dirs_creates_user_subdirs` 中，紧挨 `schedules` 目录断言后追加：

```rust
assert!(user_dir.join("agenda").join("items").exists());
assert!(user_dir.join("agenda").join("occurrences").exists());
```

执行：

```bash
cd src-tauri && cargo test --lib storage::aijia_home::tests::test_ensure_user_dirs_creates_user_subdirs
```

- [ ] **Step 7：Commit**

```bash
git add src-tauri/src/storage/user_scoped_paths.rs src-tauri/src/storage/aijia_home.rs
git commit -m "feat(agenda): add agenda_dir() and ensure subdirs on user scope init"
```

---

## 任务 4：定义 `AgendaItemId` + 枚举类型

**Files:**
- Create: `src-tauri/src/runtime/agenda/mod.rs`
- Create: `src-tauri/src/runtime/agenda/item.rs`
- Modify: `src-tauri/src/runtime/mod.rs`

- [ ] **Step 1：在 `runtime/mod.rs` 注册新 mod**

打开 `src-tauri/src/runtime/mod.rs`，按字母序在 `pub mod schedule;` 上方加：

```rust
pub mod agenda;
```

- [ ] **Step 2：创建 `agenda/mod.rs` 雏形**

```rust
pub mod item;
pub mod occurrence;
pub mod store;
pub mod trigger_eval;
pub mod runner;
pub mod dispatcher;

pub use item::{
    AgendaItem, AgendaItemId, EndCondition, Freq, ItemStatus,
    OverrideRef, Participant, RecurrenceRule, Weekday,
};
pub use occurrence::{Occurrence, OccurrenceStatus, TriggerSource};
pub use store::AgendaStore;
pub use trigger_eval::compute_next_fire_at;
pub use runner::{run_due_once, spawn_agenda_runner};
pub use dispatcher::AgendaRunDispatcher;
```

- [ ] **Step 3：创建 `agenda/item.rs` 占位 stubs**

为了让 `mod.rs` 的 `pub use` 不爆，先创建 stub 类型（后续任务填充）：

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct AgendaItemId(pub String);

impl AgendaItemId {
    pub fn new() -> Self {
        Self(format!("agenda-{}", uuid::Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Active,
    Paused,
    Completed,
    Orphaned,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Freq { Daily, Weekly, Monthly, Yearly }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EndCondition {
    Never,
    Count { n: u32 },
    Until { at: DateTime<Utc> },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Weekday { Mon, Tue, Wed, Thu, Fri, Sat, Sun }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub persona_id: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRule {
    pub freq: Freq,
    pub interval: u32,
    pub end_condition: EndCondition,
    #[serde(default)]
    pub by_day: Vec<Weekday>,
    #[serde(default)]
    pub by_month_day: Vec<i8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverrideRef {
    pub series_item_id: AgendaItemId,
    pub original_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaItem {
    pub id: AgendaItemId,
    pub title: String,
    pub prompt: String,
    pub organizer_persona_id: String,
    pub participants: Vec<Participant>,
    pub start_at: DateTime<Utc>,
    pub timezone: String,
    pub rule: Option<RecurrenceRule>,
    #[serde(default)]
    pub skip_dates: Vec<DateTime<Utc>>,
    pub next_fire_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub occurrence_count: u32,
    pub status: ItemStatus,
    pub override_of: Option<OverrideRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

- [ ] **Step 4：创建 `occurrence.rs` stub**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::item::AgendaItemId;
use crate::runtime::ids::{RunId, SessionId};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus { Running, Succeeded, Failed }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    Scheduled,
    ManualRunNow,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub id: String,
    pub agenda_item_id: AgendaItemId,
    pub fired_at: DateTime<Utc>,
    pub planned_fire_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub primary_persona_id: String,
    pub conversation_id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub status: OccurrenceStatus,
    pub error_summary: Option<String>,
    pub trigger_source: TriggerSource,
}

impl Occurrence {
    pub fn new_id() -> String {
        format!("occ-{}", uuid::Uuid::new_v4())
    }
}
```

- [ ] **Step 5：创建其余 4 个 stub 文件（让 mod.rs 编译通过）**

`store.rs`:

```rust
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AgendaStore {
    pub(crate) root: PathBuf,
    pub(crate) lock: Mutex<()>,
}
```

`trigger_eval.rs`:

```rust
use chrono::{DateTime, Utc};
use super::item::AgendaItem;

pub fn compute_next_fire_at(_item: &AgendaItem, _now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    None
}
```

`runner.rs`:

```rust
use std::sync::Arc;
use chrono::{DateTime, Utc};
use super::dispatcher::AgendaRunDispatcher;
use super::store::AgendaStore;
use crate::storage::UserScopedPathResolver;

pub fn spawn_agenda_runner(
    _path_resolver: Arc<dyn UserScopedPathResolver>,
    _dispatcher: Arc<dyn AgendaRunDispatcher>,
) {}

pub async fn run_due_once(
    _store: &AgendaStore,
    _dispatcher: &dyn AgendaRunDispatcher,
    _now: DateTime<Utc>,
) -> anyhow::Result<()> {
    Ok(())
}
```

`dispatcher.rs`:

```rust
use anyhow::Result;
use async_trait::async_trait;

use super::item::AgendaItem;
use super::occurrence::Occurrence;

#[async_trait]
pub trait AgendaRunDispatcher: Send + Sync {
    async fn dispatch(&self, item: AgendaItem, occurrence: Occurrence) -> Result<()>;
}
```

- [ ] **Step 6：cargo check**

```bash
cd src-tauri && cargo check
```
预期：PASS。

- [ ] **Step 7：Commit**

```bash
git add src-tauri/src/runtime/mod.rs src-tauri/src/runtime/agenda/
git commit -m "feat(agenda): scaffold runtime/agenda module with type stubs"
```

---

## 任务 5：`AgendaStore::new` + 文件路径策略

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs`
- Test: `src-tauri/src/runtime/agenda/store.rs`（同文件 #[cfg(test)]）

- [ ] **Step 1：写失败的测试**

替换 `store.rs` 全部内容为：

```rust
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::item::AgendaItemId;

pub struct AgendaStore {
    pub(crate) root: PathBuf,
    pub(crate) lock: Mutex<()>,
}

impl AgendaStore {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            root: base_dir.as_ref().join("agenda"),
            lock: Mutex::new(()),
        }
    }

    pub(crate) fn items_dir(&self) -> PathBuf {
        self.root.join("items")
    }

    pub(crate) fn occurrences_dir(&self) -> PathBuf {
        self.root.join("occurrences")
    }

    pub(crate) fn item_path(&self, id: &AgendaItemId) -> PathBuf {
        self.items_dir().join(format!("{}.json", id.as_str()))
    }

    pub(crate) fn occurrence_dir_for(&self, id: &AgendaItemId) -> PathBuf {
        self.occurrences_dir().join(id.as_str())
    }

    pub(crate) fn occurrence_shard_path(
        &self,
        id: &AgendaItemId,
        when: chrono::DateTime<chrono::Utc>,
    ) -> PathBuf {
        let yyyy_mm = when.format("%Y-%m").to_string();
        self.occurrence_dir_for(id).join(format!("{yyyy_mm}.jsonl"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn store_paths_under_agenda_subdir() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        assert_eq!(store.root, dir.path().join("agenda"));
        assert_eq!(store.items_dir(), dir.path().join("agenda/items"));
        assert_eq!(
            store.occurrences_dir(),
            dir.path().join("agenda/occurrences")
        );
    }

    #[test]
    fn item_path_uses_id_as_filename() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let id = AgendaItemId("agenda-abc".into());
        assert_eq!(
            store.item_path(&id),
            dir.path().join("agenda/items/agenda-abc.json")
        );
    }

    #[test]
    fn occurrence_shard_uses_yyyy_mm() {
        use chrono::TimeZone;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let id = AgendaItemId("agenda-x".into());
        let when = chrono::Utc.with_ymd_and_hms(2026, 5, 7, 1, 2, 3).unwrap();
        assert_eq!(
            store.occurrence_shard_path(&id, when),
            dir.path().join("agenda/occurrences/agenda-x/2026-05.jsonl")
        );
    }
}
```

- [ ] **Step 2：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests
```
预期：3 个测试 PASS。

- [ ] **Step 3：Commit**

```bash
git add src-tauri/src/runtime/agenda/store.rs
git commit -m "feat(agenda): AgendaStore path layout (items/{id}.json, occurrences/{id}/{yyyy-mm}.jsonl)"
```

---

## 任务 6：Store::create + 5 条约束校验

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs`

- [ ] **Step 1：写失败的测试（5 条约束 + 正常路径）**

在 `store.rs` 的 `mod tests` 末尾追加：

```rust
fn make_valid_item(persona: &str) -> super::super::item::AgendaItem {
    use chrono::Utc;
    use super::super::item::*;
    let now = Utc::now();
    AgendaItem {
        id: AgendaItemId::new(),
        title: "T".into(),
        prompt: "P".into(),
        organizer_persona_id: persona.into(),
        participants: vec![Participant { persona_id: persona.into(), joined_at: now }],
        start_at: now,
        timezone: "Asia/Shanghai".into(),
        rule: None,
        skip_dates: vec![],
        next_fire_at: None,
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn create_persists_item() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = make_valid_item("p1");
    let saved = store.create(item.clone()).unwrap();
    assert_eq!(saved, item);
    assert_eq!(saved.id, item.id);
    assert!(store.item_path(&item.id).exists());
    let persisted: super::super::item::AgendaItem =
        serde_json::from_str(&std::fs::read_to_string(store.item_path(&item.id)).unwrap())
            .unwrap();
    assert_eq!(persisted, item);
}

#[test]
fn rejects_participants_len_not_one() {
    use chrono::Utc;
    use super::super::item::Participant;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.participants.push(Participant { persona_id: "p2".into(), joined_at: Utc::now() });
    let err = store.create(item).unwrap_err();
    assert!(err.to_string().contains("participants"));
}

#[test]
fn rejects_organizer_not_in_participants() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.participants[0].persona_id = "other".into();
    let err = store.create(item).unwrap_err();
    assert!(err.to_string().contains("organizer"));
}

#[test]
fn rejects_override_of_set() {
    use chrono::Utc;
    use super::super::item::{AgendaItemId, OverrideRef};
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.override_of = Some(OverrideRef {
        series_item_id: AgendaItemId("agenda-x".into()),
        original_at: Utc::now(),
    });
    let err = store.create(item).unwrap_err();
    assert!(err.to_string().contains("override_of"));
}

#[test]
fn rejects_rule_with_by_day() {
    use super::super::item::*;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.rule = Some(RecurrenceRule {
        freq: Freq::Weekly,
        interval: 1,
        end_condition: EndCondition::Never,
        by_day: vec![Weekday::Mon],
        by_month_day: vec![],
    });
    let err = store.create(item).unwrap_err();
    assert!(err.to_string().contains("by_day"));
}

#[test]
fn rejects_rule_with_by_month_day() {
    use super::super::item::*;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.rule = Some(RecurrenceRule {
        freq: Freq::Monthly,
        interval: 1,
        end_condition: EndCondition::Never,
        by_day: vec![],
        by_month_day: vec![7],
    });
    let err = store.create(item).unwrap_err();
    assert!(err.to_string().contains("by_month_day"));
}

#[test]
fn rejects_skip_dates_on_one_shot() {
    use chrono::Utc;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.skip_dates.push(Utc::now());
    let err = store.create(item).unwrap_err();
    assert!(err.to_string().contains("skip_dates"));
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests::create_persists_item
```
预期：FAIL `no method named create`。

- [ ] **Step 3：实现 `create` + `validate_phase1_constraints`**

在 `store.rs` 的 `impl AgendaStore` 块里追加：

```rust
use super::item::AgendaItem;
use crate::storage::file_store::io::atomic_write_json;

impl AgendaStore {
    pub fn create(&self, item: AgendaItem) -> anyhow::Result<AgendaItem> {
        let _guard = self.lock.lock().unwrap();
        validate_phase1_constraints(&item)?;
        std::fs::create_dir_all(self.items_dir())?;
        atomic_write_json(&self.item_path(&item.id), &item)?;
        Ok(item)
    }
}

pub(crate) fn validate_phase1_constraints(item: &AgendaItem) -> anyhow::Result<()> {
    if item.participants.len() != 1 {
        anyhow::bail!("phase1 constraint: participants.len() must be 1");
    }
    if item.participants[0].persona_id != item.organizer_persona_id {
        anyhow::bail!("phase1 constraint: organizer must equal participants[0]");
    }
    if item.override_of.is_some() {
        anyhow::bail!("phase1 constraint: override_of must be None");
    }
    if let Some(rule) = &item.rule {
        if !rule.by_day.is_empty() || !rule.by_month_day.is_empty() {
            anyhow::bail!("phase1 constraint: rule.by_day / by_month_day must be empty");
        }
    } else if !item.skip_dates.is_empty() {
        anyhow::bail!("phase1 constraint: skip_dates only valid when rule is Some");
    }
    Ok(())
}
```

- [ ] **Step 4：跑全部 store 测试**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests
```
预期：全部 PASS。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/store.rs
git commit -m "feat(agenda): AgendaStore::create with 5 phase-1 constraint validations"
```

---

## 任务 7：Store::get / list / delete

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs`

- [ ] **Step 1：写失败的测试**

在 `mod tests` 末尾追加：

```rust
#[test]
fn get_returns_saved_item() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let saved = store.create(make_valid_item("p1")).unwrap();
    let fetched = store.get(&saved.id).unwrap();
    assert_eq!(fetched.id, saved.id);
}

#[test]
fn get_missing_returns_err() {
    use super::super::item::AgendaItemId;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let result = store.get(&AgendaItemId("missing".into()));
    assert!(result.is_err());
}

#[test]
fn list_returns_all() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    store.create(make_valid_item("p1")).unwrap();
    store.create(make_valid_item("p2")).unwrap();
    let all = store.list().unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn delete_removes_file_returns_true() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let saved = store.create(make_valid_item("p1")).unwrap();
    assert!(store.delete(&saved.id).unwrap());
    assert!(!store.item_path(&saved.id).exists());
}

#[test]
fn delete_missing_returns_false() {
    use super::super::item::AgendaItemId;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let result = store.delete(&AgendaItemId("missing".into())).unwrap();
    assert!(!result);
}

#[test]
fn get_rejects_path_traversal_id() {
    use super::super::item::AgendaItemId;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let outside = store.root.join("outside.json");
    std::fs::create_dir_all(&store.root).unwrap();
    std::fs::write(&outside, "{}").unwrap();

    let err = store.get(&AgendaItemId("../outside".into())).unwrap_err();
    assert!(err.to_string().contains("invalid agenda item id"));
    assert!(outside.exists());
}

#[test]
fn create_rejects_path_traversal_id_without_writing_outside_file() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.id = super::super::item::AgendaItemId("../outside".into());

    let err = store.create(item).unwrap_err();
    assert!(err.to_string().contains("invalid agenda item id"));
    assert!(!store.root.join("outside.json").exists());
}

#[test]
fn delete_rejects_path_traversal_id_without_removing_outside_file() {
    use super::super::item::AgendaItemId;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let outside = store.root.join("outside.json");
    std::fs::create_dir_all(&store.root).unwrap();
    std::fs::write(&outside, "{}").unwrap();

    let err = store.delete(&AgendaItemId("../outside".into())).unwrap_err();
    assert!(err.to_string().contains("invalid agenda item id"));
    assert!(outside.exists());
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests::get_returns_saved_item
```
预期：FAIL `no method named get`。

- [ ] **Step 3：实现 get / list / delete**

在 `impl AgendaStore` 块里追加：

```rust
pub fn get(&self, id: &AgendaItemId) -> anyhow::Result<AgendaItem> {
    let _guard = self.lock.lock().unwrap();
    validate_item_id_for_path(id)?;
    let path = self.item_path(id);
    if !path.exists() {
        anyhow::bail!("agenda item not found: {}", id.as_str());
    }
    let bytes = std::fs::read(&path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn list(&self) -> anyhow::Result<Vec<AgendaItem>> {
    let _guard = self.lock.lock().unwrap();
    if !self.items_dir().exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(self.items_dir())? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        out.push(serde_json::from_slice(&bytes)?);
    }
    Ok(out)
}

pub fn delete(&self, id: &AgendaItemId) -> anyhow::Result<bool> {
    let _guard = self.lock.lock().unwrap();
    validate_item_id_for_path(id)?;
    let path = self.item_path(id);
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path)?;
    Ok(true)
}

fn validate_item_id_for_path(id: &AgendaItemId) -> anyhow::Result<()> {
    let raw = id.as_str();
    if raw.is_empty() || raw == "." || raw == ".." || raw.contains('/') || raw.contains('\\') {
        anyhow::bail!("invalid agenda item id: {}", raw);
    }
    Ok(())
}
```

同时在已有 `create` 方法持锁后、`validate_phase1_constraints(&item)?;` 前追加：

```rust
validate_item_id_for_path(&item.id)?;
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests
```
预期：全部 PASS（含已有 6 个 + 新增 5 个 = 11 个）。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/store.rs
git commit -m "feat(agenda): AgendaStore get / list / delete"
```

---

## 任务 8：Store::update + organizer 不可改约束

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs`

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn update_persists_changes() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut saved = store.create(make_valid_item("p1")).unwrap();
    saved.title = "new title".into();
    let updated = store.update(saved.clone()).unwrap();
    assert_eq!(updated.title, "new title");
    assert_eq!(store.get(&saved.id).unwrap().title, "new title");
}

#[test]
fn update_rejects_organizer_change_when_not_orphaned() {
    use super::super::item::Participant;
    use chrono::Utc;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let saved = store.create(make_valid_item("p1")).unwrap();
    let mut modified = saved.clone();
    modified.organizer_persona_id = "p2".into();
    modified.participants = vec![Participant { persona_id: "p2".into(), joined_at: Utc::now() }];
    let err = store.update(modified).unwrap_err();
    assert!(err.to_string().contains("organizer"));
}

#[test]
fn update_allows_organizer_change_when_orphaned() {
    use super::super::item::{ItemStatus, Participant};
    use chrono::Utc;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut saved = store.create(make_valid_item("p1")).unwrap();
    saved.status = ItemStatus::Orphaned;
    store.update(saved.clone()).unwrap();

    let mut revived = saved.clone();
    revived.organizer_persona_id = "p2".into();
    revived.participants = vec![Participant { persona_id: "p2".into(), joined_at: Utc::now() }];
    revived.status = ItemStatus::Active;
    let updated = store.update(revived).unwrap();
    assert_eq!(updated.organizer_persona_id, "p2");
    assert_eq!(updated.status, ItemStatus::Active);
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests::update_persists_changes
```
预期：FAIL `no method named update`。

- [ ] **Step 3：实现 update**

```rust
pub fn update(&self, item: AgendaItem) -> anyhow::Result<AgendaItem> {
    let _guard = self.lock.lock().unwrap();
    validate_phase1_constraints(&item)?;
    let path = self.item_path(&item.id);
    if !path.exists() {
        anyhow::bail!("agenda item not found: {}", item.id.as_str());
    }
    let prev: AgendaItem = serde_json::from_slice(&std::fs::read(&path)?)?;
    if prev.organizer_persona_id != item.organizer_persona_id
        && prev.status != super::item::ItemStatus::Orphaned
    {
        anyhow::bail!(
            "phase1 constraint: organizer can only change when status was Orphaned"
        );
    }
    atomic_write_json(&path, &item)?;
    Ok(item)
}
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests
```
预期：全部 PASS。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/store.rs
git commit -m "feat(agenda): AgendaStore::update with organizer-immutable-unless-orphaned rule"
```

---

## 任务 9：Store::mark_orphaned_by_organizer

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs`

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn mark_orphaned_flips_status_for_matching_organizer() {
    use super::super::item::ItemStatus;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let i1 = store.create(make_valid_item("alice")).unwrap();
    let i2 = store.create(make_valid_item("bob")).unwrap();
    let count = store.mark_orphaned_by_organizer("alice").unwrap();
    assert_eq!(count, 1);
    assert_eq!(store.get(&i1.id).unwrap().status, ItemStatus::Orphaned);
    assert_eq!(store.get(&i2.id).unwrap().status, ItemStatus::Active);
}

#[test]
fn mark_orphaned_skips_already_completed() {
    use super::super::item::ItemStatus;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("alice");
    item.status = ItemStatus::Completed;
    store.create(item.clone()).unwrap();
    let count = store.mark_orphaned_by_organizer("alice").unwrap();
    assert_eq!(count, 0);
    assert_eq!(store.get(&item.id).unwrap().status, ItemStatus::Completed);
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests::mark_orphaned_flips_status_for_matching_organizer
```
预期：FAIL。

- [ ] **Step 3：实现**

```rust
pub fn mark_orphaned_by_organizer(&self, persona_id: &str) -> anyhow::Result<usize> {
    use super::item::ItemStatus;
    let _guard = self.lock.lock().unwrap();
    let mut count = 0;
    if !self.items_dir().exists() {
        return Ok(0);
    }
    for entry in std::fs::read_dir(self.items_dir())? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        let mut item: AgendaItem = serde_json::from_slice(&bytes)?;
        if item.organizer_persona_id != persona_id {
            continue;
        }
        if matches!(item.status, ItemStatus::Active | ItemStatus::Paused) {
            item.status = ItemStatus::Orphaned;
            item.updated_at = chrono::Utc::now();
            atomic_write_json(&path, &item)?;
            count += 1;
        }
    }
    Ok(count)
}
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests
```
预期：全部 PASS。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/store.rs
git commit -m "feat(agenda): AgendaStore::mark_orphaned_by_organizer"
```

---

## 任务 10：Occurrence 两段写入

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs`

- [ ] **Step 1：写失败的测试**

```rust
fn make_running_occurrence(item_id: &super::super::item::AgendaItemId) -> super::super::occurrence::Occurrence {
    use chrono::Utc;
    use super::super::occurrence::*;
    use crate::runtime::ids::{RunId, SessionId};
    let now = Utc::now();
    Occurrence {
        id: Occurrence::new_id(),
        agenda_item_id: item_id.clone(),
        fired_at: now,
        planned_fire_at: now,
        started_at: now,
        finished_at: None,
        primary_persona_id: "p1".into(),
        conversation_id: "conv-x".into(),
        session_id: SessionId::new("conv-x"),
        run_id: RunId::new("run-y"),
        status: OccurrenceStatus::Running,
        error_summary: None,
        trigger_source: TriggerSource::Scheduled,
    }
}

#[test]
fn append_occurrence_creates_jsonl_shard() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make_valid_item("p1")).unwrap();
    let occ = make_running_occurrence(&item.id);
    store.append_occurrence(&occ).unwrap();
    assert!(store.occurrence_shard_path(&item.id, occ.fired_at).exists());
}

#[test]
fn read_occurrences_returns_last_state_per_id() {
    use super::super::occurrence::OccurrenceStatus;
    use chrono::Utc;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make_valid_item("p1")).unwrap();

    let mut running = make_running_occurrence(&item.id);
    store.append_occurrence(&running).unwrap();

    let mut completed = running.clone();
    completed.status = OccurrenceStatus::Succeeded;
    completed.finished_at = Some(Utc::now());
    store.append_occurrence(&completed).unwrap();

    let occs = store.list_occurrences(&item.id, 10).unwrap();
    assert_eq!(occs.len(), 1);
    assert_eq!(occs[0].status, OccurrenceStatus::Succeeded);
    assert!(occs[0].finished_at.is_some());
}

#[test]
fn append_occurrence_rejects_path_traversal_item_id_without_writing_outside_dir() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let unsafe_id = super::super::item::AgendaItemId("../outside".into());
    let occ = make_running_occurrence(&unsafe_id);

    let err = store.append_occurrence(&occ).unwrap_err();
    assert!(err.to_string().contains("invalid agenda item id"));
    assert!(!store.root.join("outside").exists());
}

#[test]
fn list_occurrences_rejects_path_traversal_item_id() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let unsafe_id = super::super::item::AgendaItemId("../outside".into());

    let err = store.list_occurrences(&unsafe_id, 10).unwrap_err();
    assert!(err.to_string().contains("invalid agenda item id"));
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests::append_occurrence_creates_jsonl_shard
```
预期：FAIL。

- [ ] **Step 3：实现 append + list_occurrences**

```rust
use super::occurrence::Occurrence;
use std::io::Write;

impl AgendaStore {
    pub fn append_occurrence(&self, occ: &Occurrence) -> anyhow::Result<()> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(&occ.agenda_item_id)?;
        let path = self.occurrence_shard_path(&occ.agenda_item_id, occ.fired_at);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)?;
        let line = serde_json::to_string(occ)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    pub fn list_occurrences(
        &self,
        item_id: &super::item::AgendaItemId,
        limit: usize,
    ) -> anyhow::Result<Vec<Occurrence>> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(item_id)?;
        let dir = self.occurrence_dir_for(item_id);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut latest: std::collections::HashMap<String, Occurrence> = Default::default();
        let mut shards: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .collect();
        shards.sort();
        for shard in shards {
            let bytes = std::fs::read(&shard)?;
            for line in bytes.split(|b| *b == b'\n') {
                if line.is_empty() {
                    continue;
                }
                let occ: Occurrence = serde_json::from_slice(line)?;
                latest.insert(occ.id.clone(), occ);
            }
        }
        let mut out: Vec<Occurrence> = latest.into_values().collect();
        out.sort_by(|a, b| b.fired_at.cmp(&a.fired_at));
        out.truncate(limit);
        Ok(out)
    }
}
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests
```
预期：全部 PASS。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/store.rs
git commit -m "feat(agenda): AgendaStore append_occurrence / list_occurrences (two-phase write)"
```

---

## 任务 11：trigger_eval 一次性日程

**Files:**
- Modify: `src-tauri/src/runtime/agenda/trigger_eval.rs`

- [ ] **Step 1：写失败的测试**

替换 `trigger_eval.rs` 全部内容为：

```rust
use chrono::{DateTime, Utc};
use super::item::AgendaItem;

pub fn compute_next_fire_at(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if item.rule.is_none() {
        return one_shot_next(item, now);
    }
    None // 循环分支后续任务实现
}

fn one_shot_next(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if item.occurrence_count == 0 && item.start_at >= now {
        Some(item.start_at)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::item::*;
    use chrono::TimeZone;

    fn make_one_shot(start_at: DateTime<Utc>, occurrence_count: u32) -> AgendaItem {
        AgendaItem {
            id: AgendaItemId::new(),
            title: "T".into(),
            prompt: "P".into(),
            organizer_persona_id: "p1".into(),
            participants: vec![Participant { persona_id: "p1".into(), joined_at: Utc::now() }],
            start_at,
            timezone: "UTC".into(),
            rule: None,
            skip_dates: vec![],
            next_fire_at: None,
            occurrence_count,
            status: ItemStatus::Active,
            override_of: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn one_shot_future_returns_start_at() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let start_at = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let item = make_one_shot(start_at, 0);
        assert_eq!(compute_next_fire_at(&item, now), Some(start_at));
    }

    #[test]
    fn one_shot_equal_now_returns_start_at() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let item = make_one_shot(now, 0);
        assert_eq!(compute_next_fire_at(&item, now), Some(now));
    }

    #[test]
    fn one_shot_past_returns_none() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 10, 0, 0).unwrap();
        let start_at = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let item = make_one_shot(start_at, 0);
        assert_eq!(compute_next_fire_at(&item, now), None);
    }

    #[test]
    fn one_shot_already_fired_returns_none() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let start_at = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let item = make_one_shot(start_at, 1);
        assert_eq!(compute_next_fire_at(&item, now), None);
    }
}
```

- [ ] **Step 2：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::trigger_eval::tests
```
预期：3 个测试 PASS。

- [ ] **Step 3：Commit**

```bash
git add src-tauri/src/runtime/agenda/trigger_eval.rs
git commit -m "feat(agenda): trigger_eval one-shot next_fire_at"
```

---

## 任务 12：trigger_eval 循环 Daily/Weekly/Monthly/Yearly + interval

**Files:**
- Modify: `src-tauri/src/runtime/agenda/trigger_eval.rs`

- [ ] **Step 1：写失败的测试**

在 `mod tests` 末尾追加：

```rust
fn make_recurring(
    start_at: DateTime<Utc>,
    rule: RecurrenceRule,
    occurrence_count: u32,
) -> AgendaItem {
    AgendaItem {
        id: AgendaItemId::new(),
        title: "T".into(),
        prompt: "P".into(),
        organizer_persona_id: "p1".into(),
        participants: vec![Participant { persona_id: "p1".into(), joined_at: Utc::now() }],
        start_at,
        timezone: "UTC".into(),
        rule: Some(rule),
        skip_dates: vec![],
        next_fire_at: None,
        occurrence_count,
        status: ItemStatus::Active,
        override_of: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn daily_returns_first_future_occurrence() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 8, 12, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    }, 1);
    let expected = Utc.with_ymd_and_hms(2026, 5, 9, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}

#[test]
fn daily_interval_2_skips_every_other_day() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Daily, interval: 2, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    }, 1);
    let expected = Utc.with_ymd_and_hms(2026, 5, 9, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}

#[test]
fn weekly_steps_seven_days() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Weekly, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    }, 1);
    let expected = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}

#[test]
fn monthly_steps_one_month() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Monthly, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    }, 1);
    let expected = Utc.with_ymd_and_hms(2026, 6, 7, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}

#[test]
fn yearly_steps_one_year() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Yearly, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    }, 1);
    let expected = Utc.with_ymd_and_hms(2027, 5, 7, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::trigger_eval::tests::daily_returns_first_future_occurrence
```
预期：FAIL（返回 None）。

- [ ] **Step 3：实现循环步进**

替换 `compute_next_fire_at` 函数，并新增辅助函数：

```rust
use chrono::{DateTime, Datelike, Months, TimeZone, Utc};
use super::item::{AgendaItem, EndCondition, Freq, RecurrenceRule};

pub fn compute_next_fire_at(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match &item.rule {
        None => one_shot_next(item, now),
        Some(rule) => recurring_next(item, rule, now),
    }
}

fn one_shot_next(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if item.occurrence_count == 0 && item.start_at >= now {
        Some(item.start_at)
    } else {
        None
    }
}

fn recurring_next(
    item: &AgendaItem,
    rule: &RecurrenceRule,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let interval = rule.interval.max(1) as i64;
    let mut cursor = item.start_at;
    let mut steps_taken: u32 = 0;

    while cursor <= now || item.skip_dates.contains(&cursor) {
        cursor = match rule.freq {
            Freq::Daily => cursor + chrono::Duration::days(interval),
            Freq::Weekly => cursor + chrono::Duration::weeks(interval),
            Freq::Monthly => add_months(cursor, interval as u32)?,
            Freq::Yearly => add_years(cursor, interval as u32)?,
        };
        steps_taken += 1;
        if steps_taken > 10_000 {
            return None;
        }
    }

    let total_occurrences = item.occurrence_count + 1;
    match &rule.end_condition {
        EndCondition::Never => Some(cursor),
        EndCondition::Count { n } => {
            if total_occurrences > *n {
                None
            } else {
                Some(cursor)
            }
        }
        EndCondition::Until { at } => {
            if cursor > *at {
                None
            } else {
                Some(cursor)
            }
        }
    }
}

fn add_months(dt: DateTime<Utc>, months: u32) -> Option<DateTime<Utc>> {
    dt.checked_add_months(Months::new(months))
}

fn add_years(dt: DateTime<Utc>, years: u32) -> Option<DateTime<Utc>> {
    dt.with_year(dt.year() + years as i32)
}
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::trigger_eval::tests
```
预期：9 个测试全部 PASS。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/trigger_eval.rs
git commit -m "feat(agenda): trigger_eval recurring step (Daily/Weekly/Monthly/Yearly + interval)"
```

- [ ] **Step 6：写 code quality review 回归失败测试**

在 `mod tests` 末尾追加：

```rust
#[test]
fn yearly_leap_day_skips_invalid_years() {
    let start = Utc.with_ymd_and_hms(2024, 2, 29, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Yearly, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    }, 1);
    let expected = Utc.with_ymd_and_hms(2028, 2, 29, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}

#[test]
fn daily_long_catch_up_returns_next_future_occurrence() {
    let start = Utc.with_ymd_and_hms(1990, 1, 1, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    }, 1);
    let expected = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}
```

- [ ] **Step 7：跑新增回归测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::trigger_eval::tests
```
预期：FAIL，新增的 `yearly_leap_day_skips_invalid_years` 和 `daily_long_catch_up_returns_next_future_occurrence` 失败。

- [ ] **Step 8：修正闰日 yearly 与长跨度 fixed-interval catch-up**

替换 `recurring_next`、`add_years`，并新增辅助函数：

```rust
fn recurring_next(
    item: &AgendaItem,
    rule: &RecurrenceRule,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let interval = rule.interval.max(1) as i64;
    let mut cursor = item.start_at;

    if cursor <= now {
        cursor = advance_after_now(cursor, &rule.freq, interval, now)?;
    }

    let mut skip_steps: u32 = 0;
    while item.skip_dates.contains(&cursor) {
        cursor = advance_once(cursor, &rule.freq, interval)?;
        skip_steps += 1;
        if skip_steps > 10_000 {
            return None;
        }
    }

    let total_occurrences = item.occurrence_count + 1;
    match &rule.end_condition {
        EndCondition::Never => Some(cursor),
        EndCondition::Count { n } => {
            if total_occurrences > *n {
                None
            } else {
                Some(cursor)
            }
        }
        EndCondition::Until { at } => {
            if cursor > *at {
                None
            } else {
                Some(cursor)
            }
        }
    }
}

fn advance_after_now(
    cursor: DateTime<Utc>,
    freq: &Freq,
    interval: i64,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match freq {
        Freq::Daily => advance_by_fixed_days_after_now(cursor, interval, now),
        Freq::Weekly => advance_by_fixed_days_after_now(cursor, interval * 7, now),
        Freq::Monthly | Freq::Yearly => {
            let mut cursor = cursor;
            let mut steps_taken: u32 = 0;
            while cursor <= now {
                cursor = advance_once(cursor, freq, interval)?;
                steps_taken += 1;
                if steps_taken > 10_000 {
                    return None;
                }
            }
            Some(cursor)
        }
    }
}

fn advance_by_fixed_days_after_now(
    cursor: DateTime<Utc>,
    step_days: i64,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let elapsed_days = (now - cursor).num_days();
    let steps = elapsed_days / step_days + 1;
    let days_to_add = step_days.checked_mul(steps)?;
    cursor.checked_add_signed(chrono::Duration::days(days_to_add))
}

fn advance_once(dt: DateTime<Utc>, freq: &Freq, interval: i64) -> Option<DateTime<Utc>> {
    match freq {
        Freq::Daily => dt.checked_add_signed(chrono::Duration::days(interval)),
        Freq::Weekly => dt.checked_add_signed(chrono::Duration::weeks(interval)),
        Freq::Monthly => add_months(dt, interval as u32),
        Freq::Yearly => add_years(dt, interval as u32),
    }
}

fn add_years(dt: DateTime<Utc>, years: u32) -> Option<DateTime<Utc>> {
    let years = i32::try_from(years.max(1)).ok()?;
    let mut target_year = dt.year().checked_add(years)?;
    let mut attempts: u32 = 0;

    loop {
        if let Some(next) = dt.with_year(target_year) {
            return Some(next);
        }

        attempts += 1;
        if attempts > 10_000 {
            return None;
        }
        target_year = target_year.checked_add(years)?;
    }
}
```

- [ ] **Step 9：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::trigger_eval::tests
```
预期：11 个测试全部 PASS。

- [ ] **Step 10：Commit**

```bash
git add src-tauri/src/runtime/agenda/trigger_eval.rs
git commit -m "fix(agenda): handle recurring leap-day and long catch-up"
```

---

## 任务 13：trigger_eval EndCondition Count/Until + skip_dates

**Files:**
- Modify: `src-tauri/src/runtime/agenda/trigger_eval.rs`

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn count_returns_none_after_n_occurrences() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 9, 0, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Count { n: 3 },
        by_day: vec![], by_month_day: vec![],
    }, 3);
    assert_eq!(compute_next_fire_at(&item, now), None);
}

#[test]
fn count_returns_some_when_under_n() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Count { n: 3 },
        by_day: vec![], by_month_day: vec![],
    }, 1);
    let expected = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}

#[test]
fn until_returns_none_after_until_at() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let until = Utc.with_ymd_and_hms(2026, 5, 9, 0, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 9, 12, 0, 0).unwrap();
    let item = make_recurring(start, RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Until { at: until },
        by_day: vec![], by_month_day: vec![],
    }, 2);
    assert_eq!(compute_next_fire_at(&item, now), None);
}

#[test]
fn skip_dates_skips_to_next() {
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();
    let skip = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
    let mut item = make_recurring(start, RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    }, 1);
    item.skip_dates.push(skip);
    let expected = Utc.with_ymd_and_hms(2026, 5, 9, 9, 0, 0).unwrap();
    assert_eq!(compute_next_fire_at(&item, now), Some(expected));
}
```

- [ ] **Step 2：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::trigger_eval::tests
```
预期：15 个测试全部 PASS（因为 EndCondition / skip_dates 的逻辑在任务 12 已经写进去了）。

- [ ] **Step 3：Commit**

```bash
git add src-tauri/src/runtime/agenda/trigger_eval.rs
git commit -m "test(agenda): trigger_eval end_condition (Count/Until) + skip_dates coverage"
```

---

## 任务 14：Store::take_due 推进 + 状态转换

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs`

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn take_due_returns_active_items_with_past_next_fire_at() {
    use chrono::{TimeZone, Utc};
    use super::super::item::ItemStatus;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.next_fire_at = Some(Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap());
    item.status = ItemStatus::Active;
    store.create(item.clone()).unwrap();

    let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let due = store.take_due(now).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, item.id);
}

#[test]
fn take_due_skips_paused_completed_orphaned() {
    use chrono::{TimeZone, Utc};
    use super::super::item::ItemStatus;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let past = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
    for status in [ItemStatus::Paused, ItemStatus::Completed, ItemStatus::Orphaned] {
        let mut item = make_valid_item("p1");
        item.next_fire_at = Some(past);
        item.status = status;
        store.create(item).unwrap();
    }
    let due = store.take_due(now).unwrap();
    assert_eq!(due.len(), 0);
}

#[test]
fn advance_after_fire_increments_count_and_recomputes() {
    use chrono::{TimeZone, Utc};
    use super::super::item::*;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let mut item = make_valid_item("p1");
    item.start_at = start;
    item.next_fire_at = Some(start);
    item.rule = Some(RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    });
    store.create(item.clone()).unwrap();

    let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 1).unwrap();
    let updated = store.advance_after_fire(&item.id, now).unwrap();
    assert_eq!(updated.occurrence_count, 1);
    assert_eq!(
        updated.next_fire_at,
        Some(Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap())
    );
    assert_eq!(updated.status, ItemStatus::Active);
}

#[test]
fn advance_after_fire_one_shot_marks_completed() {
    use chrono::{TimeZone, Utc};
    use super::super::item::ItemStatus;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let mut item = make_valid_item("p1");
    item.start_at = start;
    item.next_fire_at = Some(start);
    store.create(item.clone()).unwrap();

    let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 1).unwrap();
    let updated = store.advance_after_fire(&item.id, now).unwrap();
    assert_eq!(updated.occurrence_count, 1);
    assert_eq!(updated.next_fire_at, None);
    assert_eq!(updated.status, ItemStatus::Completed);
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests::take_due_returns_active_items_with_past_next_fire_at
```
预期：FAIL `no method named take_due`。

- [ ] **Step 3：实现 take_due + advance_after_fire**

```rust
use super::trigger_eval::compute_next_fire_at;

impl AgendaStore {
    pub fn take_due(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<Vec<AgendaItem>> {
        use super::item::ItemStatus;
        let _guard = self.lock.lock().unwrap();
        let mut out = Vec::new();
        if !self.items_dir().exists() {
            return Ok(vec![]);
        }
        for entry in std::fs::read_dir(self.items_dir())? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let item: AgendaItem = serde_json::from_slice(&bytes)?;
            if !matches!(item.status, ItemStatus::Active) {
                continue;
            }
            if item.override_of.is_some() {
                continue;
            }
            if validate_phase1_constraints(&item).is_err() {
                continue;
            }
            if let Some(next) = item.next_fire_at {
                if next <= now {
                    out.push(item);
                }
            }
        }
        Ok(out)
    }

    pub fn advance_after_fire(
        &self,
        id: &super::item::AgendaItemId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<AgendaItem> {
        use super::item::ItemStatus;
        let _guard = self.lock.lock().unwrap();
        let path = self.item_path(id);
        if !path.exists() {
            anyhow::bail!("agenda item not found: {}", id.as_str());
        }
        let mut item: AgendaItem = serde_json::from_slice(&std::fs::read(&path)?)?;
        item.occurrence_count += 1;
        item.next_fire_at = compute_next_fire_at(&item, now);
        if item.next_fire_at.is_none() {
            item.status = ItemStatus::Completed;
        }
        item.updated_at = chrono::Utc::now();
        atomic_write_json(&path, &item)?;
        Ok(item)
    }
}
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::store::tests
```
预期：全部 PASS。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/store.rs
git commit -m "feat(agenda): AgendaStore take_due + advance_after_fire (Active filter, completion transition)"
```

---

## 任务 15：Store::set_skip / unset_skip

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs`

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn set_skip_adds_to_skip_dates() {
    use chrono::{TimeZone, Utc};
    use super::super::item::*;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.rule = Some(RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    });
    store.create(item.clone()).unwrap();
    let when = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
    let updated = store.set_skip(&item.id, when).unwrap();
    assert!(updated.skip_dates.contains(&when));
}

#[test]
fn unset_skip_removes_from_skip_dates() {
    use chrono::{TimeZone, Utc};
    use super::super::item::*;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_valid_item("p1");
    item.rule = Some(RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    });
    let when = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
    item.skip_dates.push(when);
    store.create(item.clone()).unwrap();
    let updated = store.unset_skip(&item.id, when).unwrap();
    assert!(!updated.skip_dates.contains(&when));
}

#[test]
fn set_skip_rejects_one_shot() {
    use chrono::Utc;
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make_valid_item("p1")).unwrap();
    let err = store.set_skip(&item.id, Utc::now()).unwrap_err();
    assert!(err.to_string().contains("rule"));
}
```

- [ ] **Step 2：跑测试看失败**

- [ ] **Step 3：实现**

```rust
impl AgendaStore {
    pub fn set_skip(
        &self,
        id: &super::item::AgendaItemId,
        at: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<AgendaItem> {
        let _guard = self.lock.lock().unwrap();
        let path = self.item_path(id);
        if !path.exists() {
            anyhow::bail!("agenda item not found: {}", id.as_str());
        }
        let mut item: AgendaItem = serde_json::from_slice(&std::fs::read(&path)?)?;
        if item.rule.is_none() {
            anyhow::bail!("skip_dates only valid when rule is Some");
        }
        if !item.skip_dates.contains(&at) {
            item.skip_dates.push(at);
        }
        item.next_fire_at = compute_next_fire_at(&item, chrono::Utc::now());
        item.updated_at = chrono::Utc::now();
        atomic_write_json(&path, &item)?;
        Ok(item)
    }

    pub fn unset_skip(
        &self,
        id: &super::item::AgendaItemId,
        at: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<AgendaItem> {
        let _guard = self.lock.lock().unwrap();
        let path = self.item_path(id);
        if !path.exists() {
            anyhow::bail!("agenda item not found: {}", id.as_str());
        }
        let mut item: AgendaItem = serde_json::from_slice(&std::fs::read(&path)?)?;
        item.skip_dates.retain(|d| d != &at);
        item.next_fire_at = compute_next_fire_at(&item, chrono::Utc::now());
        item.updated_at = chrono::Utc::now();
        atomic_write_json(&path, &item)?;
        Ok(item)
    }
}
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda
```
预期：全部 PASS（store + trigger_eval 共 ~25 个）。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/store.rs
git commit -m "feat(agenda): AgendaStore set_skip / unset_skip (recompute next_fire_at)"
```

---

**PR-1 收尾检查：**

```bash
cd src-tauri && cargo test --lib runtime::agenda
cd src-tauri && cargo clippy --lib -- -D warnings 2>&1 | grep -i agenda
```

Tag this commit as `agenda-pr1-done` for review checkpoint.

---

# PR-2：Runner + Dispatcher + Tauri 命令 + 前端 invoke 替换

> 这一段把 PR-1 的纯逻辑接到运行时和命令层。完成后用户在前端能通过新命令操作 agenda（虽然 UI 还是旧界面），到点了 dispatcher 真正能跑 agent。
> 关键：**ChatTurnRequest 加 persona_id_override + 改 build_system_prompt**，这是后续 dispatcher 切 persona 的基础。

## 任务 16：ChatTurnRequest 加 persona_id_override

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

- [ ] **Step 1：grep 找到 ChatTurnRequest 定义位置**

```bash
cd src-tauri && grep -n "pub struct ChatTurnRequest" src/runtime/chat/chat_turn_driver.rs
```

- [ ] **Step 2：写失败的测试**

在 `chat_turn_driver.rs` 末尾的 `#[cfg(test)] mod tests`（如果没有就新建）追加：

```rust
#[test]
fn chat_turn_request_default_has_no_persona_override() {
    let req = ChatTurnRequest::new("conv-1".to_string(), "hello".to_string(), vec![]);
    assert!(req.persona_id_override.is_none());
}

#[test]
fn chat_turn_request_with_persona_override() {
    let req = ChatTurnRequest::new("conv-1".to_string(), "hello".to_string(), vec![])
        .with_persona_id_override("persona-x".into());
    assert_eq!(req.persona_id_override.as_deref(), Some("persona-x"));
}
```

- [ ] **Step 3：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::chat::chat_turn_driver::tests::chat_turn_request_default_has_no_persona_override
```
预期：FAIL `no field persona_id_override`。

- [ ] **Step 4：在 ChatTurnRequest 加字段 + 链式 setter**

定位��� `pub struct ChatTurnRequest { ... }`，在末尾加字段：

```rust
    pub persona_id_override: Option<String>,
```

在 `impl ChatTurnRequest::new` 内的初始化里加：

```rust
            persona_id_override: None,
```

紧挨 `new` 加 setter（参考 `agent_name` 已有的 setter 风格）：

```rust
pub fn with_persona_id_override(mut self, persona_id: String) -> Self {
    self.persona_id_override = Some(persona_id);
    self
}
```

- [ ] **Step 5：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::chat::chat_turn_driver::tests
```
预期：PASS。

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(chat): ChatTurnRequest.persona_id_override field"
```

---

## 任务 17：build_system_prompt 优先用 persona_id_override

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1：grep build_system_prompt**

```bash
cd src-tauri && grep -n "fn build_system_prompt" src/transport/tauri_commands/chat.rs
```
应有一处，约在 chat.rs:1181 附近。

- [ ] **Step 2：找到读 active_persona 的位置**

```bash
cd src-tauri && grep -n "get_active_persona" src/transport/tauri_commands/chat.rs
```

应该在 `build_system_prompt` 内：`let persona = self.services.db.get_active_persona().ok();`

- [ ] **Step 3：改成优先用 override**

把这一行替换为：

```rust
let persona = match request.persona_id_override.as_deref() {
    Some(id) => self.services.db.get_persona_by_id(id).ok().or_else(|| {
        self.services.db.get_active_persona().ok()
    }),
    None => self.services.db.get_active_persona().ok(),
};
```

> 注意：`get_persona_by_id` 是已有方法。若 grep `fn get_persona_by_id` 不存在，则改用 `PersonaStore::get_persona(id)`：
> ```rust
> Some(id) => self.services.db.get_persona(id).ok().or_else(|| self.services.db.get_active_persona().ok()),
> ```

- [ ] **Step 4：找到 build_system_prompt 的调用处确保 request 可见**

```bash
cd src-tauri && grep -n "build_system_prompt" src/transport/tauri_commands/chat.rs
```
确保所有调用点都把 `request: &ChatTurnRequest` 传到了内部。如果原签名不接受 request，需要扩展签名加 `request: &ChatTurnRequest` 参数，并把所有 caller 同步更新。

- [ ] **Step 5：cargo check**

```bash
cd src-tauri && cargo check
```
预期：PASS。

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(chat): build_system_prompt prefers ChatTurnRequest.persona_id_override"
```

---

## 任务 18：send_message_with_run_id 变体（让 dispatcher 拿 run_id）

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1：grep send_message 签名**

```bash
cd src-tauri && grep -n "pub async fn send_message" src/transport/tauri_commands/chat.rs
```

- [ ] **Step 2：在 send_message 旁边加新变体**

紧挨 `send_message` 添加：

```rust
pub async fn send_message_with_overrides(
    &self,
    conversation_id: String,
    content: String,
    attachments: Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef>,
    permission_mode: Option<crate::runtime::tools::permission::PermissionMode>,
    agent_name: Option<String>,
    client_message_id: Option<String>,
    persona_id_override: Option<String>,
    run_id: Option<crate::runtime::ids::RunId>,
) -> Result<crate::runtime::ids::RunId, String> {
    let mut request = crate::runtime::chat::chat_turn_driver::ChatTurnRequest::new(
        conversation_id.clone(),
        content,
        attachments,
    );
    if let Some(id) = run_id {
        request.run_id = id;
    }
    request.agent_name = agent_name;
    request.persona_id_override = persona_id_override;
    request.permission_mode = permission_mode.unwrap_or_default();
    request.client_message_id = client_message_id;

    let captured_run_id = request.run_id.clone();

    self.run_chat_request_internal(request).await?;

    Ok(captured_run_id)
}
```

> 注：`run_chat_request_internal` 是 `send_message` 现有内部函数。若名字不一样，grep `fn.*run_chat_request` 找到对应封装。如果只能调 `self.run_chat_request(request)` 公开函数，改成那个即可。
> `request.permission_mode = ...` 行只在 ChatTurnRequest 有该字段时保留，否则删除（grep `permission_mode` 在 ChatTurnRequest 里检查）。

- [ ] **Step 3：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(chat): send_message_with_overrides variant returning RunId"
```

---

## 任务 19：AgendaRunDispatcher trait + 实现骨架

**Files:**
- Modify: `src-tauri/src/runtime/agenda/dispatcher.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1：扩展 trait 签名**

替换 `runtime/agenda/dispatcher.rs` 内容为：

```rust
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::item::AgendaItem;
use super::occurrence::TriggerSource;

#[async_trait]
pub trait AgendaRunDispatcher: Send + Sync {
    /// 异步触发一次执行：创建 conversation、切 persona、发 prompt、等 agent 跑完、回写 occurrence。
    /// 返回触发瞬间的 occurrence_id 让 caller 关联（如 run_now 命令需要返回 occurrence）。
    async fn dispatch(
        &self,
        item: AgendaItem,
        planned_fire_at: DateTime<Utc>,
        trigger_source: TriggerSource,
        now: DateTime<Utc>,
    ) -> Result<String>;
}
```

- [ ] **Step 2：在 chat.rs 加 impl 骨架**

定位到现有 `impl ScheduleRunDispatcher for TauriChatCommandAdapter`，在其下方添加：

```rust
#[async_trait::async_trait]
impl crate::runtime::agenda::AgendaRunDispatcher for TauriChatCommandAdapter {
    async fn dispatch(
        &self,
        item: crate::runtime::agenda::AgendaItem,
        planned_fire_at: chrono::DateTime<chrono::Utc>,
        trigger_source: crate::runtime::agenda::TriggerSource,
        now: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<String> {
        use crate::runtime::agenda::{Occurrence, OccurrenceStatus};
        use crate::runtime::ids::{RunId, SessionId};

        let store = self.agenda_store_for_current_user()?;

        // 1. 创建 conversation
        let conversation_id = conversation_service::create_conversation(
            self.services.db.clone() as Arc<dyn ConversationStore>,
        )
        .await
        .map_err(anyhow::Error::msg)?;

        // 2. 预生成 RunId 并写 Running occurrence
        let run_id = RunId::new(uuid::Uuid::new_v4().to_string());
        let session_id = SessionId::new(conversation_id.clone());
        let occ = Occurrence {
            id: Occurrence::new_id(),
            agenda_item_id: item.id.clone(),
            fired_at: now,
            planned_fire_at,
            started_at: now,
            finished_at: None,
            primary_persona_id: item.organizer_persona_id.clone(),
            conversation_id: conversation_id.clone(),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            status: OccurrenceStatus::Running,
            error_summary: None,
            trigger_source,
        };
        store.append_occurrence(&occ)?;
        let occurrence_id = occ.id.clone();

        // 3. 推进 item next_fire_at + occurrence_count
        if matches!(trigger_source, crate::runtime::agenda::TriggerSource::Scheduled) {
            store.advance_after_fire(&item.id, now)?;
        }

        // 4. 构造 prompt 并触发 agent
        let prompt = format!(
            "[日程触发] {}\n计划触发时间：{}\n\n{}",
            item.title, planned_fire_at, item.prompt
        );

        let result = self
            .send_message_with_overrides(
                conversation_id.clone(),
                prompt,
                Vec::new(),
                None,
                None,
                None,
                Some(item.organizer_persona_id.clone()),
                Some(run_id.clone()),
            )
            .await;

        // 5. 追加最终 occurrence
        let mut final_occ = occ.clone();
        final_occ.finished_at = Some(chrono::Utc::now());
        match result {
            Ok(_) => {
                final_occ.status = OccurrenceStatus::Succeeded;
            }
            Err(e) => {
                final_occ.status = OccurrenceStatus::Failed;
                final_occ.error_summary = Some(e);
            }
        }
        store.append_occurrence(&final_occ)?;

        Ok(occurrence_id)
    }
}
```

> 注：`agenda_store_for_current_user` 是辅助方法，下一步实现。

- [ ] **Step 3：在 TauriChatCommandAdapter 加 agenda_store_for_current_user 辅助方法**

在 `impl TauriChatCommandAdapter` 块里追加（grep `impl TauriChatCommandAdapter` 找位置）：

```rust
fn agenda_store_for_current_user(
    &self,
) -> anyhow::Result<crate::runtime::agenda::AgendaStore> {
    let resolver = self.services.current_user_storage.clone()
        as std::sync::Arc<dyn crate::storage::UserScopedPathResolver>;
    let paths = resolver.require_paths()?;
    Ok(crate::runtime::agenda::AgendaStore::new(paths.base_dir()))
}
```

> 注：`current_user_storage` 字段名要看 `TauriChatCommandAdapter` 实际持有什么。grep `current_user_storage` 或 `Arc<dyn UserScopedPathResolver>` 找到字段名。

- [ ] **Step 4：cargo check**

```bash
cd src-tauri && cargo check
```
预期：PASS。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/agenda/dispatcher.rs src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(agenda): AgendaRunDispatcher impl on TauriChatCommandAdapter"
```

---

## 任务 20：AgendaRunner 实现 + 单测

**Files:**
- Modify: `src-tauri/src/runtime/agenda/runner.rs`

- [ ] **Step 1：替换 runner.rs 完整内容**

```rust
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::time;

use super::dispatcher::AgendaRunDispatcher;
use super::occurrence::TriggerSource;
use super::store::AgendaStore;
use crate::storage::UserScopedPathResolver;

pub fn spawn_agenda_runner(
    path_resolver: Arc<dyn UserScopedPathResolver>,
    dispatcher: Arc<dyn AgendaRunDispatcher>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let Some(paths) = path_resolver.resolve_paths() else { continue; };
            let store = AgendaStore::new(paths.base_dir());
            if let Err(e) = run_due_once(&store, dispatcher.as_ref(), Utc::now()).await {
                tracing::warn!(error = %e, "agenda runner tick failed");
            }
        }
    });
}

pub async fn run_due_once(
    store: &AgendaStore,
    dispatcher: &dyn AgendaRunDispatcher,
    now: DateTime<Utc>,
) -> Result<()> {
    let due = store.take_due(now)?;
    for item in due {
        let planned = item.next_fire_at.unwrap_or(now);
        if let Err(e) = dispatcher
            .dispatch(item.clone(), planned, TriggerSource::Scheduled, now)
            .await
        {
            tracing::warn!(item_id = %item.id.as_str(), error = %e, "agenda dispatch failed");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::TempDir;

    struct RecordingDispatcher {
        calls: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl AgendaRunDispatcher for RecordingDispatcher {
        async fn dispatch(
            &self,
            item: super::super::item::AgendaItem,
            _planned: DateTime<Utc>,
            _src: TriggerSource,
            _now: DateTime<Utc>,
        ) -> anyhow::Result<String> {
            self.calls.lock().unwrap().push(item.id.as_str().to_string());
            Ok("occ-test".into())
        }
    }

    fn make_active_due_item(persona: &str, when: DateTime<Utc>) -> super::super::item::AgendaItem {
        use super::super::item::*;
        AgendaItem {
            id: AgendaItemId::new(),
            title: "T".into(),
            prompt: "P".into(),
            organizer_persona_id: persona.into(),
            participants: vec![Participant { persona_id: persona.into(), joined_at: when }],
            start_at: when,
            timezone: "UTC".into(),
            rule: None,
            skip_dates: vec![],
            next_fire_at: Some(when),
            occurrence_count: 0,
            status: ItemStatus::Active,
            override_of: None,
            created_at: when,
            updated_at: when,
        }
    }

    #[tokio::test]
    async fn run_due_once_dispatches_active_items() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let due_at = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        store.create(make_active_due_item("p1", due_at)).unwrap();

        let dispatcher = RecordingDispatcher { calls: Default::default() };
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        run_due_once(&store, &dispatcher, now).await.unwrap();
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn run_due_once_skips_when_no_due() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let dispatcher = RecordingDispatcher { calls: Default::default() };
        let now = Utc::now();
        run_due_once(&store, &dispatcher, now).await.unwrap();
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 0);
    }
}
```

- [ ] **Step 2：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::runner::tests
```
预期：2 个测试 PASS。

- [ ] **Step 3：Commit**

```bash
git add src-tauri/src/runtime/agenda/runner.rs
git commit -m "feat(agenda): spawn_agenda_runner + run_due_once with per-tick scope re-resolve"
```

---

## 任务 21：lib.rs 切到 agenda runner

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1：grep schedule_runner 启动位置**

```bash
cd src-tauri && grep -n "spawn_schedule_runner" src/lib.rs
```
应有一处（约 line 578-583）。

- [ ] **Step 2：替换 spawn 调用**

把：

```rust
runtime::schedule_runner::spawn_schedule_runner(
    current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>,
    app.state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
        .inner()
        .clone(),
);
```

替换为：

```rust
runtime::agenda::spawn_agenda_runner(
    current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>,
    app.state::<Arc<transport::tauri_commands::chat::TauriChatCommandAdapter>>()
        .inner()
        .clone() as Arc<dyn runtime::agenda::AgendaRunDispatcher>,
);
```

> 旧 schedule_runner 暂时不删（PR-4 收尾删），现在两个 runner 并存不冲突（store 路径不同）。

- [ ] **Step 3：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 4：启动测试（手动）**

```bash
pnpm tauri:dev
```
预期：启动无 panic，控制台无 error 关于 agenda。Ctrl+C 停。

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(agenda): wire spawn_agenda_runner alongside schedule_runner in lib.rs"
```

---

## 任务 22：Tauri 命令 list / get

**Files:**
- Create: `src-tauri/src/transport/tauri_commands/agenda.rs`
- Modify: `src-tauri/src/transport/tauri_commands/mod.rs`

- [ ] **Step 1：注册 mod**

打开 `src-tauri/src/transport/tauri_commands/mod.rs`，按字母序加：

```rust
pub mod agenda;
```

- [ ] **Step 2：创建 agenda.rs 雏形**

```rust
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaStore, ItemStatus, Occurrence,
};
use crate::storage::UserScopedPathResolver;

fn store_for(
    resolver: &Arc<dyn UserScopedPathResolver>,
) -> Result<AgendaStore, String> {
    let paths = resolver.require_paths().map_err(|e| e.to_string())?;
    Ok(AgendaStore::new(paths.base_dir()))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilter {
    pub status_in: Option<Vec<ItemStatus>>,
    pub persona_id: Option<String>,
    pub search: Option<String>,
}

#[tauri::command]
pub async fn list_agenda_items(
    filter: Option<ItemFilter>,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<Vec<AgendaItem>, String> {
    let store = store_for(&resolver)?;
    let mut items = store.list().map_err(|e| e.to_string())?;
    if let Some(filter) = filter {
        if let Some(statuses) = filter.status_in {
            items.retain(|i| statuses.contains(&i.status));
        }
        if let Some(persona) = filter.persona_id {
            items.retain(|i| i.organizer_persona_id == persona);
        }
        if let Some(search) = filter.search.filter(|s| !s.is_empty()) {
            let lower = search.to_lowercase();
            items.retain(|i| {
                i.title.to_lowercase().contains(&lower)
                    || i.prompt.to_lowercase().contains(&lower)
            });
        }
    }
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(items)
}

#[tauri::command]
pub async fn get_agenda_item(
    id: String,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    store.get(&AgendaItemId(id)).map_err(|e| e.to_string())
}
```

- [ ] **Step 3：lib.rs 注册 invoke handler 占位**

```bash
cd src-tauri && grep -n "Schedule commands" src/lib.rs
```

把那段 schedule commands 注册改为：

```rust
// Agenda commands
transport::tauri_commands::agenda::list_agenda_items,
transport::tauri_commands::agenda::get_agenda_item,
// 后续任务追加
```

> 旧 schedule 命令暂时保留同时注册（不冲突，名字不同）。PR-4 删。

- [ ] **Step 4：管理 resolver state**

确认 `app.manage(Arc<dyn UserScopedPathResolver>)` 已存在。grep `app.manage.*UserScopedPath` 在 lib.rs。如果只 manage 了 `Arc<CurrentUserStorage>`，则补加：

```rust
app.manage(current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>);
```

- [ ] **Step 5：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/agenda.rs src-tauri/src/transport/tauri_commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(agenda): tauri commands list_agenda_items / get_agenda_item"
```

---

## 任务 23：Tauri 命令 create

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/agenda.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1：在 agenda.rs 末尾追加**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgendaItemRequest {
    pub title: String,
    pub prompt: String,
    pub organizer_persona_id: String,
    pub start_at: DateTime<Utc>,
    pub timezone: Option<String>,
    pub rule: Option<crate::runtime::agenda::RecurrenceRule>,
}

#[tauri::command]
pub async fn create_agenda_item(
    request: CreateAgendaItemRequest,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    use crate::runtime::agenda::{Participant, ItemStatus};
    let store = store_for(&resolver)?;
    let now = Utc::now();
    let mut item = AgendaItem {
        id: AgendaItemId::new(),
        title: request.title,
        prompt: request.prompt,
        organizer_persona_id: request.organizer_persona_id.clone(),
        participants: vec![Participant {
            persona_id: request.organizer_persona_id,
            joined_at: now,
        }],
        start_at: request.start_at,
        timezone: request.timezone.unwrap_or_else(|| "Asia/Shanghai".into()),
        rule: request.rule,
        skip_dates: vec![],
        next_fire_at: None,
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        created_at: now,
        updated_at: now,
    };
    item.next_fire_at =
        crate::runtime::agenda::compute_next_fire_at(&item, now);
    store.create(item).map_err(|e| e.to_string())
}
```

- [ ] **Step 2：lib.rs 加 invoke 注册**

在 `agenda::list_agenda_items` 那段下方加：

```rust
transport::tauri_commands::agenda::create_agenda_item,
```

- [ ] **Step 3：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/agenda.rs src-tauri/src/lib.rs
git commit -m "feat(agenda): tauri command create_agenda_item"
```

---

## 任务 24：Tauri 命令 update / delete

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/agenda.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1：在 agenda.rs 末尾追加**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgendaItemRequest {
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub start_at: Option<DateTime<Utc>>,
    pub timezone: Option<String>,
    pub rule: Option<Option<crate::runtime::agenda::RecurrenceRule>>,
    pub status: Option<ItemStatus>,
    pub organizer_persona_id: Option<String>,
}

#[tauri::command]
pub async fn update_agenda_item(
    id: String,
    request: UpdateAgendaItemRequest,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    let item_id = AgendaItemId(id);
    let mut item = store.get(&item_id).map_err(|e| e.to_string())?;
    if let Some(t) = request.title { item.title = t; }
    if let Some(p) = request.prompt { item.prompt = p; }
    if let Some(s) = request.start_at { item.start_at = s; }
    if let Some(tz) = request.timezone { item.timezone = tz; }
    if let Some(r) = request.rule { item.rule = r; }
    if let Some(st) = request.status { item.status = st; }
    if let Some(o) = request.organizer_persona_id {
        use crate::runtime::agenda::Participant;
        item.organizer_persona_id = o.clone();
        item.participants = vec![Participant {
            persona_id: o,
            joined_at: Utc::now(),
        }];
    }
    item.updated_at = Utc::now();
    item.next_fire_at =
        crate::runtime::agenda::compute_next_fire_at(&item, Utc::now());
    store.update(item).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_agenda_item(
    id: String,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<bool, String> {
    let store = store_for(&resolver)?;
    store.delete(&AgendaItemId(id)).map_err(|e| e.to_string())
}
```

- [ ] **Step 2：注册 invoke**

```rust
transport::tauri_commands::agenda::update_agenda_item,
transport::tauri_commands::agenda::delete_agenda_item,
```

- [ ] **Step 3：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/agenda.rs src-tauri/src/lib.rs
git commit -m "feat(agenda): tauri commands update_agenda_item / delete_agenda_item"
```

---

## 任务 25：Tauri 命令 run_now / list_occurrences

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/agenda.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1：在 agenda.rs 末尾追加**

```rust
#[tauri::command]
pub async fn run_agenda_item_now(
    id: String,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
    dispatcher: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
) -> Result<String, String> {
    use crate::runtime::agenda::{AgendaRunDispatcher, TriggerSource};
    let store = store_for(&resolver)?;
    let item = store.get(&AgendaItemId(id)).map_err(|e| e.to_string())?;
    let now = Utc::now();
    dispatcher
        .dispatch(item.clone(), now, TriggerSource::ManualRunNow, now)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_agenda_occurrences(
    item_id: String,
    limit: Option<usize>,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<Vec<Occurrence>, String> {
    let store = store_for(&resolver)?;
    store
        .list_occurrences(&AgendaItemId(item_id), limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 2：注册 invoke**

```rust
transport::tauri_commands::agenda::run_agenda_item_now,
transport::tauri_commands::agenda::list_agenda_occurrences,
```

- [ ] **Step 3：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/agenda.rs src-tauri/src/lib.rs
git commit -m "feat(agenda): tauri commands run_agenda_item_now / list_agenda_occurrences"
```

---

## 任务 26：Tauri 命令 skip_occurrence / unskip_occurrence

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/agenda.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1：在 agenda.rs 末尾追加**

```rust
#[tauri::command]
pub async fn skip_occurrence(
    id: String,
    at: DateTime<Utc>,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    store.set_skip(&AgendaItemId(id), at).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unskip_occurrence(
    id: String,
    at: DateTime<Utc>,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    store.unset_skip(&AgendaItemId(id), at).map_err(|e| e.to_string())
}
```

- [ ] **Step 2：注册 invoke**

```rust
transport::tauri_commands::agenda::skip_occurrence,
transport::tauri_commands::agenda::unskip_occurrence,
```

- [ ] **Step 3：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/agenda.rs src-tauri/src/lib.rs
git commit -m "feat(agenda): tauri commands skip_occurrence / unskip_occurrence"
```

---

## 任务 27：集成测试 agenda_commands_test.rs

**Files:**
- Create: `src-tauri/tests/agenda_commands_test.rs`

- [ ] **Step 1：创建端到端测试**

```rust
use chrono::{Duration, TimeZone, Utc};
use tempfile::TempDir;

use app_lib::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaStore, EndCondition, Freq, ItemStatus, Occurrence,
    OccurrenceStatus, Participant, RecurrenceRule, TriggerSource,
};
use app_lib::runtime::ids::{RunId, SessionId};

fn make_item(persona: &str, start_at: chrono::DateTime<chrono::Utc>) -> AgendaItem {
    let now = Utc::now();
    AgendaItem {
        id: AgendaItemId::new(),
        title: "测试日程".into(),
        prompt: "做点事".into(),
        organizer_persona_id: persona.into(),
        participants: vec![Participant { persona_id: persona.into(), joined_at: now }],
        start_at,
        timezone: "Asia/Shanghai".into(),
        rule: None,
        skip_dates: vec![],
        next_fire_at: Some(start_at),
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn create_then_list_includes_item() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let saved = store.create(make_item("p1", Utc::now() + Duration::hours(1))).unwrap();
    let listed = store.list().unwrap();
    assert!(listed.iter().any(|i| i.id == saved.id));
}

#[test]
fn delete_then_list_excludes_item() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let saved = store.create(make_item("p1", Utc::now() + Duration::hours(1))).unwrap();
    assert!(store.delete(&saved.id).unwrap());
    assert!(store.list().unwrap().iter().all(|i| i.id != saved.id));
}

#[test]
fn skip_then_unskip_round_trip() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_item("p1", Utc::now() + Duration::hours(1));
    item.rule = Some(RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    });
    store.create(item.clone()).unwrap();
    let target = Utc::now() + Duration::days(2);
    let after_skip = store.set_skip(&item.id, target).unwrap();
    assert!(after_skip.skip_dates.contains(&target));
    let after_unskip = store.unset_skip(&item.id, target).unwrap();
    assert!(!after_unskip.skip_dates.contains(&target));
}

#[test]
fn append_occurrence_then_list_returns_running() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make_item("p1", Utc::now())).unwrap();
    let occ = Occurrence {
        id: Occurrence::new_id(),
        agenda_item_id: item.id.clone(),
        fired_at: Utc::now(),
        planned_fire_at: Utc::now(),
        started_at: Utc::now(),
        finished_at: None,
        primary_persona_id: "p1".into(),
        conversation_id: "conv-1".into(),
        session_id: SessionId::new("conv-1"),
        run_id: RunId::new("run-1"),
        status: OccurrenceStatus::Running,
        error_summary: None,
        trigger_source: TriggerSource::Scheduled,
    };
    store.append_occurrence(&occ).unwrap();
    let listed = store.list_occurrences(&item.id, 10).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, OccurrenceStatus::Running);
}

#[test]
fn append_occurrence_succeeded_overrides_running() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make_item("p1", Utc::now())).unwrap();
    let id = Occurrence::new_id();
    let mut occ = Occurrence {
        id: id.clone(),
        agenda_item_id: item.id.clone(),
        fired_at: Utc::now(),
        planned_fire_at: Utc::now(),
        started_at: Utc::now(),
        finished_at: None,
        primary_persona_id: "p1".into(),
        conversation_id: "conv-1".into(),
        session_id: SessionId::new("conv-1"),
        run_id: RunId::new("run-1"),
        status: OccurrenceStatus::Running,
        error_summary: None,
        trigger_source: TriggerSource::Scheduled,
    };
    store.append_occurrence(&occ).unwrap();
    occ.status = OccurrenceStatus::Succeeded;
    occ.finished_at = Some(Utc::now());
    store.append_occurrence(&occ).unwrap();
    let listed = store.list_occurrences(&item.id, 10).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, OccurrenceStatus::Succeeded);
}
```

- [ ] **Step 2：跑测试**

```bash
cd src-tauri && cargo test --test agenda_commands_test -- --nocapture
```
预期：5 个测试 PASS。

- [ ] **Step 3：Commit**

```bash
git add src-tauri/tests/agenda_commands_test.rs
git commit -m "test(agenda): integration test for store CRUD + skip + occurrence write"
```

---

## 任务 28：前端 tauri.ts 类型 + invoke 封装替换

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1：grep 找到 schedule 相关代码**

```bash
grep -n "schedule\|Schedule" src/lib/tauri.ts
```

- [ ] **Step 2：替换整段为 agenda 类型 + 9 个 invoke**

定位到 `export type ScheduleStatus` 起始行，整段（含 ScheduleRecord / CreateScheduleRequest / 3 个函数）替换为：

```typescript
export type ItemStatus = 'active' | 'paused' | 'completed' | 'orphaned'
export type OccurrenceStatus = 'running' | 'succeeded' | 'failed'
export type Freq = 'daily' | 'weekly' | 'monthly' | 'yearly'

export interface Participant {
  personaId: string
  joinedAt: string
}

export interface RecurrenceRule {
  freq: Freq
  interval: number
  endCondition:
    | { kind: 'never' }
    | { kind: 'count'; n: number }
    | { kind: 'until'; at: string }
  byDay?: string[]
  byMonthDay?: number[]
}

export interface OverrideRef {
  seriesItemId: string
  originalAt: string
}

export interface AgendaItem {
  id: string
  title: string
  prompt: string
  organizerPersonaId: string
  participants: Participant[]
  startAt: string
  timezone: string
  rule: RecurrenceRule | null
  skipDates: string[]
  nextFireAt: string | null
  occurrenceCount: number
  status: ItemStatus
  overrideOf: OverrideRef | null
  createdAt: string
  updatedAt: string
}

export interface Occurrence {
  id: string
  agendaItemId: string
  firedAt: string
  plannedFireAt: string
  startedAt: string
  finishedAt: string | null
  primaryPersonaId: string
  conversationId: string
  sessionId: string
  runId: string
  status: OccurrenceStatus
  errorSummary: string | null
  triggerSource: 'scheduled' | 'manual_run_now'
}

export interface ItemFilter {
  statusIn?: ItemStatus[]
  personaId?: string
  search?: string
}

export interface CreateAgendaItemRequest {
  title: string
  prompt: string
  organizerPersonaId: string
  startAt: string
  timezone?: string
  rule?: RecurrenceRule | null
}

export interface UpdateAgendaItemRequest {
  title?: string
  prompt?: string
  startAt?: string
  timezone?: string
  rule?: RecurrenceRule | null
  status?: ItemStatus
  organizerPersonaId?: string
}

export function listAgendaItems(filter?: ItemFilter): Promise<AgendaItem[]> {
  return invoke<AgendaItem[]>('list_agenda_items', { filter })
}
export function getAgendaItem(id: string): Promise<AgendaItem> {
  return invoke<AgendaItem>('get_agenda_item', { id })
}
export function createAgendaItem(request: CreateAgendaItemRequest): Promise<AgendaItem> {
  return invoke<AgendaItem>('create_agenda_item', { request })
}
export function updateAgendaItem(
  id: string,
  request: UpdateAgendaItemRequest,
): Promise<AgendaItem> {
  return invoke<AgendaItem>('update_agenda_item', { id, request })
}
export function deleteAgendaItem(id: string): Promise<boolean> {
  return invoke<boolean>('delete_agenda_item', { id })
}
export function runAgendaItemNow(id: string): Promise<string> {
  return invoke<string>('run_agenda_item_now', { id })
}
export function listAgendaOccurrences(itemId: string, limit?: number): Promise<Occurrence[]> {
  return invoke<Occurrence[]>('list_agenda_occurrences', { itemId, limit })
}
export function skipOccurrence(id: string, at: string): Promise<AgendaItem> {
  return invoke<AgendaItem>('skip_occurrence', { id, at })
}
export function unskipOccurrence(id: string, at: string): Promise<AgendaItem> {
  return invoke<AgendaItem>('unskip_occurrence', { id, at })
}
```

> **保留旧 schedule 类型/函数**：暂不删，PR-2 内不动 SchedulesPage（保持调旧 invoke），等 PR-3 切换完成再删。

- [ ] **Step 3：tsc 检查**

```bash
pnpm exec tsc --noEmit
```
预期：可能在 SchedulesPage 用了旧的 `ScheduleRecord` 类型导致 error，这一步先确认 agenda 类型本身无 error；SchedulesPage 的 error 在 PR-3 修。

如果旧 schedule 类型本步还要被引用：保留 ScheduleRecord 定义在文件，避免编译 break。

- [ ] **Step 4：Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(agenda): TS types + invoke wrappers for 9 agenda commands"
```

---

## 任务 29：useAgendaItems hook

**Files:**
- Create: `src/hooks/useAgendaItems.ts`

- [ ] **Step 1：实现**

```typescript
import { useCallback, useEffect, useState } from 'react'

import { AgendaItem, ItemFilter, listAgendaItems } from '@/lib/tauri'

export function useAgendaItems(filter?: ItemFilter) {
  const [items, setItems] = useState<AgendaItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const next = await listAgendaItems(filter)
      setItems(next)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [filter])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const next = await listAgendaItems(filter)
        if (!cancelled) setItems(next)
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      cancelled = true
    }
  }, [filter])

  return { items, loading, error, refresh }
}
```

- [ ] **Step 2：tsc 检查**

```bash
pnpm exec tsc --noEmit
```

- [ ] **Step 3：Commit**

```bash
git add src/hooks/useAgendaItems.ts
git commit -m "feat(agenda): useAgendaItems hook"
```

---

## 任务 30：删除前端 schedule 类型与函数（保持 SchedulesPage 暂不动）

> 实际操作：PR-2 不删旧的 `ScheduleRecord` / `listSchedules`，让 SchedulesPage 仍能跑（虽然命令已不存在）。**这一步先跳过**，跳到任务 31。
> （留下来是为了 PR-3 切完 SchedulesPage 后再统一清。）

跳过 — 占位，无操作。

---

## 任务 31：review_agenda_runner_scope.rs（每 tick 重 resolve scope 锁住）

**Files:**
- Create: `src-tauri/tests/review_agenda_runner_scope.rs`

- [ ] **Step 1：写测试**

```rust
//! Architecture review: agenda runner must re-resolve scope every tick.
//! This locks in the fix that prevents scope-switch bug (the runner shouldn't
//! cache the AgendaStore across ticks).

#[test]
fn runner_module_re_resolves_scope_in_loop() {
    let source = std::fs::read_to_string("src/runtime/agenda/runner.rs").unwrap();
    assert!(
        source.contains("path_resolver.resolve_paths()"),
        "spawn_agenda_runner must call resolve_paths() inside the loop"
    );
    let lines: Vec<&str> = source.lines().collect();
    let resolve_idx = lines
        .iter()
        .position(|l| l.contains("path_resolver.resolve_paths()"))
        .expect("resolve_paths call not found");
    let loop_idx = lines
        .iter()
        .position(|l| l.contains("loop {"))
        .expect("loop block not found");
    assert!(
        resolve_idx > loop_idx,
        "resolve_paths must be inside the tick loop"
    );
}
```

- [ ] **Step 2：跑测试**

```bash
cd src-tauri && cargo test --test review_agenda_runner_scope
```
预期：PASS。

- [ ] **Step 3：Commit**

```bash
git add src-tauri/tests/review_agenda_runner_scope.rs
git commit -m "test(agenda): review test locking per-tick scope re-resolve"
```

---

## 任务 32：review_agenda_command_thinness.rs

**Files:**
- Create: `src-tauri/tests/review_agenda_command_thinness.rs`

- [ ] **Step 1：写测试**

```rust
//! Architecture review: tauri_commands/agenda.rs must be thin (no business logic).
//! Each #[tauri::command] body must only validate / convert / call store|dispatcher.

#[test]
fn agenda_commands_only_delegate_to_store_or_dispatcher() {
    let source = std::fs::read_to_string("src/transport/tauri_commands/agenda.rs").unwrap();
    let lines: Vec<&str> = source.lines().collect();
    let mut in_command = false;
    let mut current_fn = String::new();
    let mut body_lines: Vec<String> = Vec::new();

    for line in &lines {
        if line.contains("#[tauri::command]") {
            in_command = true;
            continue;
        }
        if in_command && line.contains("pub async fn ") {
            current_fn = line.trim().to_string();
            body_lines.clear();
            continue;
        }
        if in_command && line.starts_with("}") && !line.contains("=>") {
            // 命令体结束，断言函数体未超过 30 行（薄转发预算）
            assert!(
                body_lines.len() < 30,
                "command `{}` body has {} lines (limit 30, business logic should be in store/runtime)",
                current_fn,
                body_lines.len()
            );
            in_command = false;
            current_fn.clear();
            continue;
        }
        if in_command && !current_fn.is_empty() {
            body_lines.push(line.to_string());
        }
    }
}
```

- [ ] **Step 2：跑测试**

```bash
cd src-tauri && cargo test --test review_agenda_command_thinness
```
预期：PASS（如果 update 命令超过 30 行，要么把字段更新逻辑挪到 store::update 接受 partial request，要么放宽 limit 到 40）。

- [ ] **Step 3：Commit**

```bash
git add src-tauri/tests/review_agenda_command_thinness.rs
git commit -m "test(agenda): review test for tauri commands thinness"
```

---

**PR-2 收尾检查：**

```bash
cd src-tauri && cargo test
pnpm exec tsc --noEmit
```

后端可触发 + 前端可调用，完整运行端到端。Tag `agenda-pr2-done`。

---

# PR-3：前端 Sheet + hooks + 列表行补齐

> 这一段把 SchedulesPage 切到 useAgendaItems hook，新增 Editor / Detail Sheet，补齐 ScheduleTaskRow 的 4 个 hover 按钮和状态色条。完成后用户能完整地用新 UI 创建/编辑/暂停/立即运行/查看历史。

## 任务 33：SchedulesPage 切到 useAgendaItems

**Files:**
- Modify: `src/features/schedules/SchedulesPage.tsx`

- [ ] **Step 1：grep 现状**

```bash
grep -n "listSchedules\|ScheduleRecord\|useState.*Schedule" src/features/schedules/SchedulesPage.tsx
```

- [ ] **Step 2：替换 import + 类型 + state**

把：
```typescript
import { listSchedules, createSchedule, deleteSchedule, ScheduleRecord } from '@/lib/tauri'
```
替换为：
```typescript
import {
  AgendaItem,
  createAgendaItem,
  deleteAgendaItem,
  runAgendaItemNow,
  updateAgendaItem,
} from '@/lib/tauri'
import { useAgendaItems } from '@/hooks/useAgendaItems'
```

把组件内的 `const [schedules, setSchedules] = useState<ScheduleRecord[]>([])` 段以及 `refresh` callback 替换为：

```typescript
const { items, loading, error, refresh } = useAgendaItems()
```

把 `schedules` 引用全部改成 `items`，`ScheduleRecord` 类型改成 `AgendaItem`。

- [ ] **Step 3：把模板的"使用模板"按钮改成"预填表单"**

定位到 `TEMPLATES` 常量数组使用处（grep `TEMPLATES`），把"点击直接 createSchedule"的回调改成：

```typescript
const [draftFromTemplate, setDraftFromTemplate] = useState<Partial<CreateAgendaItemRequest> | null>(null)
const [editorOpen, setEditorOpen] = useState(false)

const handleTemplate = (tpl: typeof TEMPLATES[number]) => {
  setDraftFromTemplate({
    title: tpl.title,
    prompt: tpl.prompt,
    // 其他从 tpl 派生的字段
  })
  setEditorOpen(true)
}
```

> Editor 组件在任务 36 实现；这一步先建立 prop 钩子。

- [ ] **Step 4：删除按钮触发流程接 deleteAgendaItem**

把 `onConfirm={() => deleteSchedule(id).then(refresh)}` 改成 `onConfirm={() => deleteAgendaItem(id).then(refresh)}`。

- [ ] **Step 5：tsc**

```bash
pnpm exec tsc --noEmit
```
预期：可能有 import 顺序、未使用 import 等小 error，逐个修。

- [ ] **Step 6：Commit**

```bash
git add src/features/schedules/SchedulesPage.tsx
git commit -m "feat(agenda): SchedulesPage uses useAgendaItems hook + AgendaItem type"
```

---

## 任务 34：ScheduleTaskRow 补 4 个 hover 按钮

**Files:**
- Modify: `src/components/schedules/ScheduleTaskRow.tsx`

- [ ] **Step 1：grep 现状**

```bash
cat src/components/schedules/ScheduleTaskRow.tsx
```

- [ ] **Step 2：扩展 props**

```typescript
interface ScheduleTaskRowProps {
  item: AgendaItem
  onEdit: (item: AgendaItem) => void
  onDelete: (id: string) => void
  onRunNow: (id: string) => void
  onToggleStatus: (item: AgendaItem) => void
}
```

- [ ] **Step 3：替换 row UI**

把现有 row 的"删除按钮"段替换为 hover 显示 4 个图标按钮。Lucide icons：`Play / Pause / Pencil / Trash2`。完整 row JSX 大致：

```tsx
import { Pause, Pencil, Play, Trash2 } from 'lucide-react'

const statusColor = {
  active: 'border-l-2 border-blue-500',
  paused: 'opacity-70',
  completed: '',
  orphaned: 'border-l-2 border-red-500',
}[item.status]

return (
  <div className={`group flex items-center gap-3 px-4 py-2 hover:bg-muted/50 ${statusColor}`}>
    <PersonaAvatar personaId={item.organizerPersonaId} size="sm" />
    <div className="flex-1 min-w-0">
      <div className="font-medium truncate">{item.title}</div>
      <div className="text-xs text-muted-foreground">
        {describeFrequency(item.rule, item.startAt, item.timezone)}
      </div>
    </div>
    <div className="text-xs text-muted-foreground">
      {item.nextFireAt ? formatRelative(item.nextFireAt) : '-'}
    </div>
    <div className="opacity-0 group-hover:opacity-100 flex gap-1">
      <IconButton onClick={() => onRunNow(item.id)} title="立即运行">
        <Play className="w-4 h-4" />
      </IconButton>
      <IconButton onClick={() => onToggleStatus(item)} title={item.status === 'active' ? '暂停' : '启用'}>
        {item.status === 'active' ? <Pause className="w-4 h-4" /> : <Play className="w-4 h-4" />}
      </IconButton>
      <IconButton onClick={() => onEdit(item)} title="编辑">
        <Pencil className="w-4 h-4" />
      </IconButton>
      <IconButton onClick={() => onDelete(item.id)} title="删除">
        <Trash2 className="w-4 h-4 text-destructive" />
      </IconButton>
    </div>
  </div>
)
```

> `PersonaAvatar` / `IconButton` / `formatRelative` / `describeFrequency` 是辅助：分别在任务 35 / 已存在 / `date-fns` 内置 / 任务 35 实现。

- [ ] **Step 4：tsc**

```bash
pnpm exec tsc --noEmit
```
预期：缺 PersonaAvatar / describeFrequency。下一任务实现。

- [ ] **Step 5：Commit**

```bash
git add src/components/schedules/ScheduleTaskRow.tsx
git commit -m "feat(agenda): ScheduleTaskRow with 4 hover icon buttons + status visual"
```

---

## 任务 35：辅助组件 PersonaAvatar + describeFrequency

**Files:**
- Create: `src/components/agenda/PersonaAvatar.tsx`
- Create: `src/features/agenda/describeFrequency.ts`
- Test: `src/features/agenda/describeFrequency.test.ts`

- [ ] **Step 1：创建 describeFrequency 测试**

```typescript
import { describe, it, expect } from 'vitest'
import { describeFrequency } from './describeFrequency'

describe('describeFrequency', () => {
  it('one-shot returns formatted start time', () => {
    expect(describeFrequency(null, '2026-05-07T01:00:00Z', 'Asia/Shanghai'))
      .toContain('2026')
  })

  it('daily interval=1', () => {
    expect(
      describeFrequency(
        { freq: 'daily', interval: 1, endCondition: { kind: 'never' } },
        '2026-05-07T01:00:00Z',
        'Asia/Shanghai',
      ),
    ).toContain('每天')
  })

  it('daily interval=2', () => {
    expect(
      describeFrequency(
        { freq: 'daily', interval: 2, endCondition: { kind: 'never' } },
        '2026-05-07T01:00:00Z',
        'Asia/Shanghai',
      ),
    ).toContain('每 2 天')
  })

  it('weekly with count', () => {
    expect(
      describeFrequency(
        { freq: 'weekly', interval: 1, endCondition: { kind: 'count', n: 5 } },
        '2026-05-07T01:00:00Z',
        'Asia/Shanghai',
      ),
    ).toContain('共 5 次')
  })
})
```

- [ ] **Step 2：跑测试看失败**

```bash
pnpm exec vitest run src/features/agenda/describeFrequency.test.ts
```

- [ ] **Step 3：实现 describeFrequency.ts**

```typescript
import { format } from 'date-fns'
import { zhCN } from 'date-fns/locale'

import type { RecurrenceRule } from '@/lib/tauri'

export function describeFrequency(
  rule: RecurrenceRule | null,
  startAt: string,
  timezone: string,
): string {
  const dt = new Date(startAt)
  const timeStr = format(dt, 'HH:mm', { locale: zhCN })

  if (!rule) {
    return format(dt, 'yyyy-MM-dd HH:mm', { locale: zhCN })
  }

  const intervalLabel = rule.interval === 1
    ? { daily: '每天', weekly: '每周', monthly: '每月', yearly: '每年' }[rule.freq]
    : `每 ${rule.interval} ${ { daily: '天', weekly: '周', monthly: '月', yearly: '年' }[rule.freq] }`

  let endLabel = ''
  if (rule.endCondition.kind === 'count') {
    endLabel = `，共 ${rule.endCondition.n} 次`
  } else if (rule.endCondition.kind === 'until') {
    endLabel = `，至 ${format(new Date(rule.endCondition.at), 'yyyy-MM-dd', { locale: zhCN })}`
  }

  return `${intervalLabel} ${timeStr}${endLabel}`
}
```

- [ ] **Step 4：跑测试看通过**

```bash
pnpm exec vitest run src/features/agenda/describeFrequency.test.ts
```

- [ ] **Step 5：实现 PersonaAvatar**

```typescript
import { useEffect, useState } from 'react'

import { invoke } from '@tauri-apps/api/core'

interface PersonaAvatarProps {
  personaId: string
  size?: 'sm' | 'md'
}

export function PersonaAvatar({ personaId, size = 'md' }: PersonaAvatarProps) {
  const [name, setName] = useState<string>(personaId)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const persona = await invoke<{ name?: string; emoji?: string }>('get_persona', { id: personaId })
        if (!cancelled && persona?.name) setName(persona.name)
      } catch {}
    })()
    return () => { cancelled = true }
  }, [personaId])

  const dim = size === 'sm' ? 'w-5 h-5 text-[10px]' : 'w-7 h-7 text-xs'
  return (
    <div className={`rounded-full bg-muted flex items-center justify-center ${dim}`}>
      {name.slice(0, 1)}
    </div>
  )
}
```

> 注：`get_persona` 命令存在性需 grep 验证。如果不存在，PersonaAvatar 退化成只显示 personaId 首字。

- [ ] **Step 6：Commit**

```bash
git add src/features/agenda/describeFrequency.ts src/features/agenda/describeFrequency.test.ts src/components/agenda/PersonaAvatar.tsx
git commit -m "feat(agenda): describeFrequency util + PersonaAvatar component"
```

---

## 任务 36：AgendaItemEditor Sheet（创建/编辑）

**Files:**
- Create: `src/features/agenda/AgendaItemEditor.tsx`

- [ ] **Step 1：实现 Editor**

```typescript
import { useEffect, useState } from 'react'

import {
  AgendaItem,
  CreateAgendaItemRequest,
  Freq,
  RecurrenceRule,
  UpdateAgendaItemRequest,
  createAgendaItem,
  updateAgendaItem,
} from '@/lib/tauri'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

interface AgendaItemEditorProps {
  open: boolean
  initial?: AgendaItem | null
  initialDraft?: Partial<CreateAgendaItemRequest> | null
  organizerPersonaId: string
  onClose: () => void
  onSaved: () => void
}

type Frequency = 'one_shot' | Freq

export function AgendaItemEditor({
  open, initial, initialDraft, organizerPersonaId, onClose, onSaved,
}: AgendaItemEditorProps) {
  const [title, setTitle] = useState('')
  const [prompt, setPrompt] = useState('')
  const [startAtLocal, setStartAtLocal] = useState('')
  const [timezone, setTimezone] = useState('Asia/Shanghai')
  const [frequency, setFrequency] = useState<Frequency>('one_shot')
  const [interval, setInterval] = useState(1)
  const [endKind, setEndKind] = useState<'never' | 'count' | 'until'>('never')
  const [endCount, setEndCount] = useState(10)
  const [endUntilLocal, setEndUntilLocal] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (initial) {
      setTitle(initial.title)
      setPrompt(initial.prompt)
      setStartAtLocal(toLocalInput(initial.startAt))
      setTimezone(initial.timezone)
      setFrequency(initial.rule?.freq ?? 'one_shot')
      setInterval(initial.rule?.interval ?? 1)
      const ec = initial.rule?.endCondition
      if (!ec || ec.kind === 'never') setEndKind('never')
      else if (ec.kind === 'count') { setEndKind('count'); setEndCount(ec.n) }
      else { setEndKind('until'); setEndUntilLocal(toLocalInput(ec.at)) }
    } else if (initialDraft) {
      setTitle(initialDraft.title ?? '')
      setPrompt(initialDraft.prompt ?? '')
    }
  }, [initial, initialDraft])

  const buildRule = (): RecurrenceRule | null => {
    if (frequency === 'one_shot') return null
    const endCondition: RecurrenceRule['endCondition'] =
      endKind === 'never' ? { kind: 'never' }
      : endKind === 'count' ? { kind: 'count', n: endCount }
      : { kind: 'until', at: new Date(endUntilLocal).toISOString() }
    return { freq: frequency, interval, endCondition }
  }

  const handleSave = async () => {
    setSaving(true)
    try {
      const startAt = new Date(startAtLocal).toISOString()
      if (initial) {
        const req: UpdateAgendaItemRequest = {
          title, prompt, startAt, timezone, rule: buildRule(),
        }
        await updateAgendaItem(initial.id, req)
      } else {
        const req: CreateAgendaItemRequest = {
          title, prompt, startAt, timezone,
          organizerPersonaId,
          rule: buildRule(),
        }
        await createAgendaItem(req)
      }
      onSaved()
      onClose()
    } finally {
      setSaving(false)
    }
  }

  return (
    <Sheet open={open} onOpenChange={(v) => !v && onClose()}>
      <SheetContent className="w-[480px] flex flex-col gap-4">
        <SheetHeader>
          <SheetTitle>{initial ? '编辑日程' : '新建日程'}</SheetTitle>
        </SheetHeader>
        <Input placeholder="标题" value={title} onChange={(e) => setTitle(e.target.value)} />
        <Textarea placeholder="到点要做什么？" rows={4} value={prompt} onChange={(e) => setPrompt(e.target.value)} />

        <div className="space-y-2">
          <label className="text-xs">频率</label>
          <Select value={frequency} onValueChange={(v) => setFrequency(v as Frequency)}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="one_shot">一次性</SelectItem>
              <SelectItem value="daily">每天</SelectItem>
              <SelectItem value="weekly">每周</SelectItem>
              <SelectItem value="monthly">每月</SelectItem>
              <SelectItem value="yearly">每年</SelectItem>
            </SelectContent>
          </Select>
        </div>

        {frequency !== 'one_shot' && (
          <div className="space-y-2">
            <label className="text-xs">每 N {{ daily: '天', weekly: '周', monthly: '月', yearly: '年' }[frequency]}</label>
            <Input type="number" min={1} value={interval} onChange={(e) => setInterval(Number(e.target.value))} />

            <label className="text-xs mt-2">结束条件</label>
            <Select value={endKind} onValueChange={(v) => setEndKind(v as typeof endKind)}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="never">永不结束</SelectItem>
                <SelectItem value="count">N 次后结束</SelectItem>
                <SelectItem value="until">到日期</SelectItem>
              </SelectContent>
            </Select>
            {endKind === 'count' && (
              <Input type="number" min={1} value={endCount} onChange={(e) => setEndCount(Number(e.target.value))} />
            )}
            {endKind === 'until' && (
              <Input type="datetime-local" value={endUntilLocal} onChange={(e) => setEndUntilLocal(e.target.value)} />
            )}
          </div>
        )}

        <div className="space-y-2">
          <label className="text-xs">开始时间</label>
          <Input type="datetime-local" value={startAtLocal} onChange={(e) => setStartAtLocal(e.target.value)} />
        </div>

        <div className="flex gap-2 mt-auto">
          <Button variant="outline" onClick={onClose} disabled={saving}>取消</Button>
          <Button onClick={handleSave} disabled={saving || !title || !prompt || !startAtLocal}>保存</Button>
        </div>
      </SheetContent>
    </Sheet>
  )
}

function toLocalInput(iso: string): string {
  const d = new Date(iso)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}
```

> 注：`Sheet`/`Input`/`Textarea`/`Select` 等是项目已有 ui 组件（grep `from '@/components/ui/sheet'`）。如果不存在，改用项目已有的等价组件。

- [ ] **Step 2：tsc**

```bash
pnpm exec tsc --noEmit
```

- [ ] **Step 3：Commit**

```bash
git add src/features/agenda/AgendaItemEditor.tsx
git commit -m "feat(agenda): AgendaItemEditor sheet (create/edit)"
```

---

## 任务 37：AgendaItemEditor 测试

**Files:**
- Create: `src/features/agenda/AgendaItemEditor.test.tsx`

- [ ] **Step 1：写测试**

```typescript
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'

const invokeMock = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import { AgendaItemEditor } from './AgendaItemEditor'

describe('AgendaItemEditor', () => {
  beforeEach(() => { invokeMock.mockReset() })

  it('saves new one-shot item', async () => {
    invokeMock.mockResolvedValueOnce({ id: 'agenda-x' })
    const onSaved = vi.fn()

    render(
      <AgendaItemEditor
        open
        organizerPersonaId="p1"
        onClose={() => {}}
        onSaved={onSaved}
      />,
    )

    fireEvent.change(screen.getByPlaceholderText('标题'), { target: { value: 'T' } })
    fireEvent.change(screen.getByPlaceholderText('到点要做什么？'), { target: { value: 'P' } })

    const dt = screen.getByLabelText(/开始时间/i, { selector: 'input' }) as HTMLInputElement
    fireEvent.change(dt, { target: { value: '2026-05-07T09:00' } })

    fireEvent.click(screen.getByText('保存'))

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('create_agenda_item', expect.objectContaining({
        request: expect.objectContaining({ title: 'T', prompt: 'P', organizerPersonaId: 'p1' }),
      }))
      expect(onSaved).toHaveBeenCalled()
    })
  })

  it('renders frequency-conditional fields', async () => {
    render(
      <AgendaItemEditor
        open
        organizerPersonaId="p1"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    )
    expect(screen.queryByText(/结束条件/i)).toBeNull()
    fireEvent.change(screen.getByDisplayValue('一次性'), { target: { value: 'daily' } })
    await waitFor(() => expect(screen.getByText(/结束条件/i)).toBeInTheDocument())
  })
})
```

- [ ] **Step 2：跑测试**

```bash
pnpm exec vitest run src/features/agenda/AgendaItemEditor.test.tsx
```

> Select 组件触发方式按项目实际而定。如果 `fireEvent.change` 不工作，参考已有 `*.test.tsx` 看怎么 trigger。如果实在不好测，可以放宽到只测"渲染了某些字段"。

- [ ] **Step 3：Commit**

```bash
git add src/features/agenda/AgendaItemEditor.test.tsx
git commit -m "test(agenda): AgendaItemEditor save + frequency-conditional fields"
```

---

## 任务 38：AgendaItemDetail Sheet（3 Tab）

**Files:**
- Create: `src/features/agenda/AgendaItemDetail.tsx`

- [ ] **Step 1：实现**

```typescript
import { useEffect, useState } from 'react'

import {
  AgendaItem,
  Occurrence,
  listAgendaOccurrences,
  skipOccurrence,
} from '@/lib/tauri'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Button } from '@/components/ui/button'

import { AgendaItemEditor } from './AgendaItemEditor'
import { describeFrequency } from './describeFrequency'

interface AgendaItemDetailProps {
  open: boolean
  item: AgendaItem | null
  onClose: () => void
  onChanged: () => void
}

export function AgendaItemDetail({ open, item, onClose, onChanged }: AgendaItemDetailProps) {
  const [occs, setOccs] = useState<Occurrence[]>([])
  const [editorOpen, setEditorOpen] = useState(false)

  useEffect(() => {
    if (!item) return
    let cancelled = false
    void (async () => {
      const list = await listAgendaOccurrences(item.id, 50)
      if (!cancelled) setOccs(list)
    })()
    return () => { cancelled = true }
  }, [item])

  if (!item) return null

  return (
    <>
      <Sheet open={open} onOpenChange={(v) => !v && onClose()}>
        <SheetContent className="w-[520px] flex flex-col gap-4">
          <SheetHeader>
            <SheetTitle>{item.title}</SheetTitle>
          </SheetHeader>
          <Tabs defaultValue="overview">
            <TabsList>
              <TabsTrigger value="overview">概览</TabsTrigger>
              <TabsTrigger value="history">执行历史</TabsTrigger>
              <TabsTrigger value="settings">设置</TabsTrigger>
            </TabsList>
            <TabsContent value="overview" className="space-y-2">
              <Row label="组织者" value={item.organizerPersonaId} />
              <Row label="频率" value={describeFrequency(item.rule, item.startAt, item.timezone)} />
              <Row label="下次触发" value={item.nextFireAt ?? '-'} />
              <Row label="状态" value={item.status} />
            </TabsContent>
            <TabsContent value="history" className="space-y-1">
              {occs.map((o) => (
                <div key={o.id} className="flex items-center gap-2 text-sm py-1 border-b">
                  <span className="text-muted-foreground">{o.firedAt}</span>
                  <span className={o.status === 'succeeded' ? 'text-green-600' : o.status === 'failed' ? 'text-red-600' : 'text-yellow-600'}>
                    {o.status}
                  </span>
                  <span className="flex-1 truncate text-xs">{o.errorSummary ?? ''}</span>
                </div>
              ))}
            </TabsContent>
            <TabsContent value="settings">
              <Button onClick={() => setEditorOpen(true)}>编辑</Button>
            </TabsContent>
          </Tabs>
        </SheetContent>
      </Sheet>
      <AgendaItemEditor
        open={editorOpen}
        initial={item}
        organizerPersonaId={item.organizerPersonaId}
        onClose={() => setEditorOpen(false)}
        onSaved={() => { onChanged(); setEditorOpen(false); }}
      />
    </>
  )
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex gap-2 text-sm">
      <span className="text-muted-foreground w-20">{label}</span>
      <span>{value}</span>
    </div>
  )
}
```

- [ ] **Step 2：tsc**

```bash
pnpm exec tsc --noEmit
```

- [ ] **Step 3：Commit**

```bash
git add src/features/agenda/AgendaItemDetail.tsx
git commit -m "feat(agenda): AgendaItemDetail sheet with 3 tabs"
```

---

## 任务 39：AgendaItemDetail 测试

**Files:**
- Create: `src/features/agenda/AgendaItemDetail.test.tsx`

- [ ] **Step 1：写测试**

```typescript
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'

const invokeMock = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import { AgendaItemDetail } from './AgendaItemDetail'

const sampleItem = {
  id: 'agenda-1', title: 'T', prompt: 'P',
  organizerPersonaId: 'p1', participants: [],
  startAt: '2026-05-07T01:00:00Z', timezone: 'Asia/Shanghai',
  rule: null, skipDates: [], nextFireAt: '2026-05-07T01:00:00Z',
  occurrenceCount: 0, status: 'active' as const, overrideOf: null,
  createdAt: '', updatedAt: '',
}

describe('AgendaItemDetail', () => {
  beforeEach(() => { invokeMock.mockReset() })

  it('loads occurrences when opened', async () => {
    invokeMock.mockResolvedValueOnce([
      { id: 'occ-1', agendaItemId: 'agenda-1', firedAt: '2026-05-06T01:00:00Z',
        plannedFireAt: '2026-05-06T01:00:00Z', startedAt: '2026-05-06T01:00:00Z',
        finishedAt: '2026-05-06T01:01:00Z', primaryPersonaId: 'p1',
        conversationId: 'conv-1', sessionId: 'conv-1', runId: 'run-1',
        status: 'succeeded', errorSummary: null, triggerSource: 'scheduled' },
    ])

    render(<AgendaItemDetail open item={sampleItem} onClose={() => {}} onChanged={() => {}} />)

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('list_agenda_occurrences', { itemId: 'agenda-1', limit: 50 })
    })
  })

  it('renders overview tab fields', () => {
    invokeMock.mockResolvedValueOnce([])
    render(<AgendaItemDetail open item={sampleItem} onClose={() => {}} onChanged={() => {}} />)
    expect(screen.getByText(/组织者/)).toBeInTheDocument()
    expect(screen.getByText(/频率/)).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：跑测试**

```bash
pnpm exec vitest run src/features/agenda/AgendaItemDetail.test.tsx
```

- [ ] **Step 3：Commit**

```bash
git add src/features/agenda/AgendaItemDetail.test.tsx
git commit -m "test(agenda): AgendaItemDetail occurrences load + overview render"
```

---

## 任务 40：SchedulesPage 接 Editor + Detail

**Files:**
- Modify: `src/features/schedules/SchedulesPage.tsx`

- [ ] **Step 1：加 state + handler**

在 `SchedulesPage` 顶部加：

```typescript
import { AgendaItemEditor } from '@/features/agenda/AgendaItemEditor'
import { AgendaItemDetail } from '@/features/agenda/AgendaItemDetail'
import { invoke } from '@tauri-apps/api/core'

const [editing, setEditing] = useState<AgendaItem | null>(null)
const [detail, setDetail] = useState<AgendaItem | null>(null)
const [editorOpen, setEditorOpen] = useState(false)
const [draftFromTemplate, setDraftFromTemplate] = useState<Partial<CreateAgendaItemRequest> | null>(null)
const [activePersonaId, setActivePersonaId] = useState('default')

useEffect(() => {
  void invoke<{ id: string }>('get_active_persona').then((p) => p?.id && setActivePersonaId(p.id))
}, [])
```

> `get_active_persona` 命令名要 grep 验证。

- [ ] **Step 2：替换 ScheduleTaskRow 调用**

把现有 `<ScheduleTaskRow ... />` 调用改成传入新 props：

```tsx
<ScheduleTaskRow
  key={item.id}
  item={item}
  onEdit={(it) => { setEditing(it); setDraftFromTemplate(null); setEditorOpen(true); }}
  onDelete={(id) => deleteAgendaItem(id).then(refresh)}
  onRunNow={(id) => runAgendaItemNow(id).then(refresh)}
  onToggleStatus={(it) => updateAgendaItem(it.id, {
    status: it.status === 'active' ? 'paused' : 'active',
  }).then(refresh)}
/>
```

- [ ] **Step 3：渲染 Editor + Detail**

在组件 JSX 末尾加：

```tsx
<AgendaItemEditor
  open={editorOpen}
  initial={editing}
  initialDraft={draftFromTemplate}
  organizerPersonaId={editing?.organizerPersonaId ?? activePersonaId}
  onClose={() => { setEditorOpen(false); setEditing(null); setDraftFromTemplate(null); }}
  onSaved={() => { refresh(); setEditorOpen(false); setEditing(null); setDraftFromTemplate(null); }}
/>
<AgendaItemDetail
  open={detail !== null}
  item={detail}
  onClose={() => setDetail(null)}
  onChanged={refresh}
/>
```

- [ ] **Step 4：tsc**

```bash
pnpm exec tsc --noEmit
```

- [ ] **Step 5：Commit**

```bash
git add src/features/schedules/SchedulesPage.tsx
git commit -m "feat(agenda): SchedulesPage wires Editor + Detail sheets"
```

---

## 任务 41：模板系统改为预填表单

**Files:**
- Modify: `src/components/schedules/ScheduleTemplateCard.tsx`
- Modify: `src/features/schedules/SchedulesPage.tsx`

- [ ] **Step 1：改 ScheduleTemplateCard 的 onClick prop 语义**

把"点击直接 createSchedule"改成"点击 chip 调 onPick(template)"：

```typescript
interface ScheduleTemplateCardProps {
  template: { title: string; prompt: string; rule?: RecurrenceRule | null }
  onPick: (template: ScheduleTemplateCardProps['template']) => void
}
```

按钮文案从"使用模板"改成"用此模板"。

- [ ] **Step 2：SchedulesPage 中接入**

```tsx
{TEMPLATES.map((tpl) => (
  <ScheduleTemplateCard
    key={tpl.title}
    template={tpl}
    onPick={(t) => {
      setDraftFromTemplate({
        title: t.title,
        prompt: t.prompt,
        rule: t.rule ?? null,
      })
      setEditing(null)
      setEditorOpen(true)
    }}
  />
))}
```

- [ ] **Step 3：tsc**

```bash
pnpm exec tsc --noEmit
```

- [ ] **Step 4：Commit**

```bash
git add src/components/schedules/ScheduleTemplateCard.tsx src/features/schedules/SchedulesPage.tsx
git commit -m "feat(agenda): templates prefill editor instead of direct create"
```

---

## 任务 42：SchedulesPage 测试扩展

**Files:**
- Modify: `src/features/schedules/SchedulesPage.test.tsx`

- [ ] **Step 1：grep 现状**

```bash
cat src/features/schedules/SchedulesPage.test.tsx
```

- [ ] **Step 2：把现有 mock 改成 list_agenda_items**

把 `invokeMock.mockResolvedValueOnce([...])` 关联的 invoke 名从 `list_schedules` 改成 `list_agenda_items`，类型从 `ScheduleRecord[]` 改成 `AgendaItem[]`（简化字段：`{ id, title, prompt, organizerPersonaId: 'p1', participants: [], startAt: '', timezone: '', rule: null, skipDates: [], nextFireAt: null, occurrenceCount: 0, status: 'active', overrideOf: null, createdAt: '', updatedAt: '' }`）。

- [ ] **Step 3：加新测试**

```typescript
it('toggles status via row button', async () => {
  invokeMock.mockResolvedValueOnce([sampleItem({ status: 'active' })])
  invokeMock.mockResolvedValueOnce(sampleItem({ status: 'paused' })) // update return
  invokeMock.mockResolvedValueOnce([sampleItem({ status: 'paused' })]) // refresh

  render(<SchedulesPage />)
  await waitFor(() => screen.getByText(sampleItem().title))

  // hover 触发可见性 — 测试里 hover-only 元素直接 query
  const pauseBtn = await screen.findByTitle('暂停')
  fireEvent.click(pauseBtn)

  await waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('update_agenda_item', {
      id: sampleItem().id,
      request: { status: 'paused' },
    })
  })
})

it('run-now button triggers run_agenda_item_now', async () => {
  invokeMock.mockResolvedValueOnce([sampleItem({})])
  invokeMock.mockResolvedValueOnce('occ-x')
  invokeMock.mockResolvedValueOnce([sampleItem({})])

  render(<SchedulesPage />)
  await waitFor(() => screen.getByText(sampleItem().title))
  fireEvent.click(await screen.findByTitle('立即运行'))
  await waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith('run_agenda_item_now', { id: sampleItem().id })
  })
})
```

`sampleItem` 定义在 test 顶部：

```typescript
const sampleItem = (over: Partial<AgendaItem> = {}): AgendaItem => ({
  id: 'agenda-1', title: '测试日程', prompt: 'P',
  organizerPersonaId: 'p1', participants: [],
  startAt: '2026-05-07T01:00:00Z', timezone: 'Asia/Shanghai',
  rule: null, skipDates: [], nextFireAt: '2026-05-07T01:00:00Z',
  occurrenceCount: 0, status: 'active', overrideOf: null,
  createdAt: '', updatedAt: '',
  ...over,
})
```

- [ ] **Step 3：跑测试**

```bash
pnpm exec vitest run src/features/schedules/SchedulesPage.test.tsx
```

- [ ] **Step 4：Commit**

```bash
git add src/features/schedules/SchedulesPage.test.tsx
git commit -m "test(agenda): SchedulesPage toggle status + run-now coverage"
```

---

## 任务 43：清理前端旧 schedule 类型与 invoke 函数

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1：删除旧的 ScheduleRecord / ScheduleStatus / CreateScheduleRequest / 3 个 schedule 函数**

确认 `grep -rn "listSchedules\|createSchedule\|deleteSchedule\|ScheduleRecord" src/` 仅命中即将删的位置或测试快照（无实际依赖）。

删除这些定义。

- [ ] **Step 2：tsc 全量检查**

```bash
pnpm exec tsc --noEmit
```
预期：0 error。

- [ ] **Step 3：Commit**

```bash
git add src/lib/tauri.ts
git commit -m "refactor(agenda): remove obsolete schedule types and invoke wrappers"
```

---

## 任务 44：手动 UI 烟雾测试

**Files:** 无（人工执行）

- [ ] **Step 1：启动开发模式**

```bash
pnpm tauri:dev
```

- [ ] **Step 2：跑通 happy path**

进入"定时任务"页面：
1. 点"使用模板"chip → 打开 Editor，字段已预填 → 把开始时间改成 1 分钟后 → 保存 → 列表新行出现
2. hover 该行 → 看到 4 个图标 → 点"暂停" → 行变灰
3. 点"启用"恢复 → 点"立即运行" → 1-2 秒内能在该 persona 的对话里看到新对话被打开
4. 点"编辑" → 改标题 → 保存 → 列表行标题更新
5. 点"删除" → 确认 → 行消失
6. 等真正到点（创建一个 1 分钟后的）→ runner 自动触发 → 详情面板"执行历史" Tab 能看到 Succeeded 记录

- [ ] **Step 3：异常路径**

- 创建一个一次性日程，开始时间设为 1 分钟前 → 应该立刻一次也不触发（next_fire_at = None）
- 创建循环日程，count=2 → 触发 2 次后 status 自动变 Completed，列表自动隐藏（除非筛选 Completed）

- [ ] **Step 4：在 PR 描述里贴截图或简短结果**

如果 UI 烟雾测试有问题，回到对应任务修；通过即可。

- [ ] **Step 5：（无 commit；报告即可）**

---

**PR-3 收尾检查：**

```bash
cd src-tauri && cargo test
pnpm test
pnpm exec tsc --noEmit
pnpm tauri:dev    # 手动验证
```

Tag `agenda-pr3-done`。

---

# PR-4：Agent 工具 + Persona 删除联动 + review tests + 删旧代码

> 这一段补 6 个 RuntimeTool（让数字员工自管日程）、persona 删除时把 organizer 命中的 item 转 Orphaned、3 个 review test、删除老 schedule 模块。

## 任务 45：CapabilityContext 加 current_persona_id

**Files:**
- Modify: `src-tauri/src/runtime/tools/capability.rs`

- [ ] **Step 1：写测试**

在 `capability.rs` 末尾的 `#[cfg(test)] mod tests`（如无则新建）追加：

```rust
#[test]
fn current_persona_id_default_none() {
    let ctx = CapabilityContext::default();
    assert!(ctx.current_persona_id.is_none());
}

#[test]
fn current_persona_id_can_be_set() {
    let mut ctx = CapabilityContext::default();
    ctx.current_persona_id = Some("p1".into());
    assert_eq!(ctx.current_persona_id.as_deref(), Some("p1"));
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::tools::capability::tests::current_persona_id_default_none
```
预期：FAIL。

- [ ] **Step 3：在 struct 加字段**

```rust
pub struct CapabilityContext {
    // ... 现有字段
    pub current_persona_id: Option<String>,
}
```

`Default::default()` 实现里加：

```rust
current_persona_id: None,
```

- [ ] **Step 4：在编排层注入字段**

grep `CapabilityContext::new` 或 `CapabilityContext {` 找到所有构造点。在主路径（chat.rs / send_message_with_overrides 调用栈里 build CapabilityContext 的地方）填入：

```rust
current_persona_id: request.persona_id_override.clone()
    .or_else(|| self.services.db.get_active_persona_id().ok()),
```

- [ ] **Step 5：cargo check + 跑全部 lib 测试**

```bash
cd src-tauri && cargo check && cargo test --lib runtime::tools::capability
```

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/runtime/tools/capability.rs
git commit -m "feat(tools): CapabilityContext.current_persona_id field"
```

---

## 任务 46：AgendaToolDeps + builtin/agenda mod 雏形

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/agenda/mod.rs`
- Create: `src-tauri/src/runtime/tools/builtin/agenda/deps.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/mod.rs`

- [ ] **Step 1：注册 mod**

`runtime/tools/builtin/mod.rs` 加：

```rust
pub mod agenda;
```

- [ ] **Step 2：创建 mod.rs**

```rust
pub mod deps;
pub mod create;
pub mod list;
pub mod update;
pub mod cancel;
pub mod skip;
pub mod list_occurrences;

pub use deps::AgendaToolDeps;
pub use create::CreateAgendaItemRuntimeTool;
pub use list::ListAgendaItemsRuntimeTool;
pub use update::UpdateAgendaItemRuntimeTool;
pub use cancel::CancelAgendaItemRuntimeTool;
pub use skip::SkipOccurrenceRuntimeTool;
pub use list_occurrences::ListAgendaOccurrencesRuntimeTool;
```

- [ ] **Step 3：创建 deps.rs**

```rust
use std::path::PathBuf;
use std::sync::Arc;

use crate::runtime::agenda::AgendaStore;

pub struct AgendaToolDeps {
    pub store: Arc<AgendaStore>,
    pub current_persona_id: String,
}

impl AgendaToolDeps {
    pub fn new(base_dir: PathBuf, current_persona_id: String) -> Self {
        Self {
            store: Arc::new(AgendaStore::new(base_dir)),
            current_persona_id,
        }
    }
}
```

- [ ] **Step 4：6 个 tool stub 文件**

每个文件先创建占位 struct + impl 防 mod.rs 编译断：

`create.rs`:
```rust
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;

use super::deps::AgendaToolDeps;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct CreateAgendaItemRuntimeTool {
    pub deps: Arc<AgendaToolDeps>,
}

#[async_trait]
impl RuntimeTool for CreateAgendaItemRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("create_agenda_item", "创建日程")
    }

    async fn execute(&self, _input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        Err(ToolError::ExecutionFailed("not yet implemented".into()))
    }
}
```

类似创建 `list.rs / update.rs / cancel.rs / skip.rs / list_occurrences.rs`，struct 名分别 `ListAgendaItemsRuntimeTool / UpdateAgendaItemRuntimeTool / CancelAgendaItemRuntimeTool / SkipOccurrenceRuntimeTool / ListAgendaOccurrencesRuntimeTool`，工具 id 分别 `list_agenda_items / update_agenda_item / cancel_agenda_item / skip_occurrence / list_agenda_occurrences`。

- [ ] **Step 5：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/agenda/ src-tauri/src/runtime/tools/builtin/mod.rs
git commit -m "feat(agenda): scaffold builtin/agenda module with 6 tool stubs"
```

---

## 任务 47：实现 CreateAgendaItemRuntimeTool

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/agenda/create.rs`

- [ ] **Step 1：写测试**

在 `create.rs` 末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn make_tool(dir: &std::path::Path, persona: &str) -> CreateAgendaItemRuntimeTool {
        CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), persona.into())),
        }
    }

    #[tokio::test]
    async fn create_returns_item_with_organizer_forced_to_current_persona() {
        let dir = TempDir::new().unwrap();
        let tool = make_tool(dir.path(), "alice");
        let input = json!({
            "title": "T",
            "prompt": "P",
            "start_at": "2026-05-07T01:00:00Z",
        });
        let ctx = ToolExecutionContext::for_test("s", "r", "c");
        let result = tool.execute(input, ctx).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["organizerPersonaId"], "alice");
    }

    #[tokio::test]
    async fn create_rejects_when_title_missing() {
        let dir = TempDir::new().unwrap();
        let tool = make_tool(dir.path(), "alice");
        let input = json!({ "prompt": "P", "start_at": "2026-05-07T01:00:00Z" });
        let ctx = ToolExecutionContext::for_test("s", "r", "c");
        let err = tool.execute(input, ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_) | ToolError::InputValidationError { .. }));
    }
}
```

- [ ] **Step 2：跑测试看失败**

```bash
cd src-tauri && cargo test --lib runtime::tools::builtin::agenda::create::tests
```

- [ ] **Step 3：实现**

```rust
async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
    use chrono::{DateTime, Utc};
    use crate::runtime::agenda::{AgendaItem, AgendaItemId, ItemStatus, Participant, RecurrenceRule};

    let title = required_str(&input, "title")?.to_string();
    let prompt = required_str(&input, "prompt")?.to_string();
    let start_at: DateTime<Utc> = required_str(&input, "start_at")?
        .parse()
        .map_err(|e: chrono::ParseError| ToolError::ExecutionFailed(format!("start_at parse: {e}")))?;
    let timezone = input.get("timezone")
        .and_then(Value::as_str)
        .unwrap_or("Asia/Shanghai")
        .to_string();
    let rule: Option<RecurrenceRule> = match input.get("rule") {
        Some(Value::Null) | None => None,
        Some(v) => Some(serde_json::from_value(v.clone())
            .map_err(|e| ToolError::ExecutionFailed(format!("rule: {e}")))?),
    };

    let now = Utc::now();
    let mut item = AgendaItem {
        id: AgendaItemId::new(),
        title,
        prompt,
        organizer_persona_id: self.deps.current_persona_id.clone(),
        participants: vec![Participant {
            persona_id: self.deps.current_persona_id.clone(),
            joined_at: now,
        }],
        start_at,
        timezone,
        rule,
        skip_dates: vec![],
        next_fire_at: None,
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        created_at: now,
        updated_at: now,
    };
    item.next_fire_at = crate::runtime::agenda::compute_next_fire_at(&item, now);

    let saved = self.deps.store.create(item)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let json = serde_json::to_value(&saved).unwrap();
    Ok(ToolResult::new(
        "create_agenda_item",
        serde_json::to_string_pretty(&json).unwrap(),
        Some(json),
    ))
}
```

末尾加 helper：

```rust
fn required_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    input.get(key).and_then(Value::as_str)
        .ok_or_else(|| ToolError::InputValidationError {
            tool_name: "create_agenda_item".into(),
            message: format!("missing field '{}'", key),
        })
}
```

- [ ] **Step 4：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::tools::builtin::agenda::create::tests
```

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/agenda/create.rs
git commit -m "feat(agenda): CreateAgendaItemRuntimeTool with organizer forced to current persona"
```

---

## 任务 48：实现 ListAgendaItemsRuntimeTool

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/agenda/list.rs`

- [ ] **Step 1：写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn list_returns_only_current_persona_items() {
        let dir = TempDir::new().unwrap();
        // 准备数据：两个 persona 各创建一条
        for persona in ["alice", "bob"] {
            let tool = super::super::create::CreateAgendaItemRuntimeTool {
                deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), persona.into())),
            };
            tool.execute(
                json!({ "title": "T", "prompt": "P", "start_at": "2026-05-07T01:00:00Z" }),
                ToolExecutionContext::for_test("s", "r", "c"),
            ).await.unwrap();
        }

        let tool = ListAgendaItemsRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "alice".into())),
        };
        let result = tool.execute(json!({}), ToolExecutionContext::for_test("s", "r", "c")).await.unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&result.content).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["organizerPersonaId"], "alice");
    }
}
```

- [ ] **Step 2：实现 execute**

```rust
async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
    use crate::runtime::agenda::ItemStatus;
    let items = self.deps.store.list()
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let status_filter: Option<Vec<ItemStatus>> = input.get("status_in")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let limit = input.get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50) as usize;
    let mut filtered: Vec<_> = items.into_iter()
        .filter(|i| i.organizer_persona_id == self.deps.current_persona_id)
        .filter(|i| match &status_filter {
            Some(allowed) => allowed.contains(&i.status),
            None => true,
        })
        .collect();
    filtered.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    filtered.truncate(limit);
    let json = serde_json::to_value(&filtered).unwrap();
    Ok(ToolResult::new(
        "list_agenda_items",
        serde_json::to_string_pretty(&json).unwrap(),
        Some(json),
    ))
}
```

- [ ] **Step 3：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::tools::builtin::agenda::list::tests
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/agenda/list.rs
git commit -m "feat(agenda): ListAgendaItemsRuntimeTool filtered by current persona"
```

---

## 任务 49：实现 UpdateAgendaItemRuntimeTool（带 organizer 校验）

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/agenda/update.rs`

- [ ] **Step 1：写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    async fn create(dir: &std::path::Path, persona: &str) -> String {
        let tool = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), persona.into())),
        };
        let result = tool.execute(
            json!({ "title": "T", "prompt": "P", "start_at": "2026-05-07T01:00:00Z" }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        parsed["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn update_succeeds_for_owned_item() {
        let dir = TempDir::new().unwrap();
        let id = create(dir.path(), "alice").await;
        let tool = UpdateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "alice".into())),
        };
        let result = tool.execute(
            json!({ "id": id, "title": "T2" }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["title"], "T2");
    }

    #[tokio::test]
    async fn update_rejects_other_personas_item() {
        let dir = TempDir::new().unwrap();
        let id = create(dir.path(), "alice").await;
        let tool = UpdateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "bob".into())),
        };
        let err = tool.execute(
            json!({ "id": id, "title": "T2" }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_) | ToolError::ExecutionFailed(_)));
    }
}
```

- [ ] **Step 2：实现 execute**

```rust
async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
    use crate::runtime::agenda::{AgendaItemId, ItemStatus, RecurrenceRule};
    let id_str = input.get("id").and_then(Value::as_str)
        .ok_or_else(|| ToolError::InputValidationError {
            tool_name: "update_agenda_item".into(),
            message: "missing 'id'".into(),
        })?;
    let id = AgendaItemId(id_str.to_string());
    let mut item = self.deps.store.get(&id)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    if item.organizer_persona_id != self.deps.current_persona_id {
        return Err(ToolError::PermissionDenied(
            "can only update own agenda items".into(),
        ));
    }
    if let Some(t) = input.get("title").and_then(Value::as_str) {
        item.title = t.to_string();
    }
    if let Some(p) = input.get("prompt").and_then(Value::as_str) {
        item.prompt = p.to_string();
    }
    if let Some(rule_v) = input.get("rule") {
        item.rule = if rule_v.is_null() {
            None
        } else {
            Some(serde_json::from_value::<RecurrenceRule>(rule_v.clone())
                .map_err(|e| ToolError::ExecutionFailed(format!("rule: {e}")))?)
        };
    }
    if let Some(st) = input.get("status").and_then(Value::as_str) {
        item.status = match st {
            "active" => ItemStatus::Active,
            "paused" => ItemStatus::Paused,
            other => return Err(ToolError::InputValidationError {
                tool_name: "update_agenda_item".into(),
                message: format!("status only supports active|paused, got '{other}'"),
            }),
        };
    }
    item.updated_at = chrono::Utc::now();
    item.next_fire_at =
        crate::runtime::agenda::compute_next_fire_at(&item, chrono::Utc::now());
    let saved = self.deps.store.update(item)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let json = serde_json::to_value(&saved).unwrap();
    Ok(ToolResult::new(
        "update_agenda_item",
        serde_json::to_string_pretty(&json).unwrap(),
        Some(json),
    ))
}
```

- [ ] **Step 3：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::tools::builtin::agenda::update::tests
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/agenda/update.rs
git commit -m "feat(agenda): UpdateAgendaItemRuntimeTool with persona-ownership check"
```

---

## 任务 50：实现 CancelAgendaItemRuntimeTool

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/agenda/cancel.rs`

- [ ] **Step 1：写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    async fn make_item(dir: &std::path::Path, persona: &str) -> String {
        let tool = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), persona.into())),
        };
        let result = tool.execute(
            json!({ "title": "T", "prompt": "P", "start_at": "2026-05-07T01:00:00Z" }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap();
        serde_json::from_str::<serde_json::Value>(&result.content).unwrap()["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn cancel_owned_item_returns_true() {
        let dir = TempDir::new().unwrap();
        let id = make_item(dir.path(), "alice").await;
        let tool = CancelAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "alice".into())),
        };
        let result = tool.execute(json!({ "id": id }), ToolExecutionContext::for_test("s", "r", "c")).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["deleted"], true);
    }

    #[tokio::test]
    async fn cancel_others_item_denied() {
        let dir = TempDir::new().unwrap();
        let id = make_item(dir.path(), "alice").await;
        let tool = CancelAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "bob".into())),
        };
        let err = tool.execute(json!({ "id": id }), ToolExecutionContext::for_test("s", "r", "c")).await.unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
    }
}
```

- [ ] **Step 2：实现 execute**

```rust
async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
    use crate::runtime::agenda::AgendaItemId;
    let id_str = input.get("id").and_then(Value::as_str)
        .ok_or_else(|| ToolError::InputValidationError {
            tool_name: "cancel_agenda_item".into(),
            message: "missing 'id'".into(),
        })?;
    let id = AgendaItemId(id_str.to_string());
    let item = self.deps.store.get(&id)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    if item.organizer_persona_id != self.deps.current_persona_id {
        return Err(ToolError::PermissionDenied(
            "can only cancel own agenda items".into(),
        ));
    }
    let deleted = self.deps.store.delete(&id)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let json = serde_json::json!({ "id": id_str, "deleted": deleted });
    Ok(ToolResult::new(
        "cancel_agenda_item",
        serde_json::to_string_pretty(&json).unwrap(),
        Some(json),
    ))
}
```

- [ ] **Step 3：跑测试看通过**

```bash
cd src-tauri && cargo test --lib runtime::tools::builtin::agenda::cancel::tests
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/agenda/cancel.rs
git commit -m "feat(agenda): CancelAgendaItemRuntimeTool with persona-ownership check"
```

---

## 任务 51：实现 SkipOccurrenceRuntimeTool

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/agenda/skip.rs`

- [ ] **Step 1：写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    async fn make_recurring_item(dir: &std::path::Path, persona: &str) -> String {
        let tool = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.to_path_buf(), persona.into())),
        };
        let result = tool.execute(
            json!({
                "title": "T", "prompt": "P", "start_at": "2026-05-07T01:00:00Z",
                "rule": { "freq": "daily", "interval": 1, "endCondition": { "kind": "never" } },
            }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap();
        serde_json::from_str::<serde_json::Value>(&result.content).unwrap()["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn skip_adds_to_skip_dates() {
        let dir = TempDir::new().unwrap();
        let id = make_recurring_item(dir.path(), "alice").await;
        let tool = SkipOccurrenceRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "alice".into())),
        };
        let result = tool.execute(
            json!({ "id": id, "at": "2026-05-08T01:00:00Z" }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        let skip_dates = parsed["skipDates"].as_array().unwrap();
        assert!(skip_dates.iter().any(|s| s == "2026-05-08T01:00:00Z"));
    }
}
```

- [ ] **Step 2：实现 execute**

```rust
async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
    use chrono::{DateTime, Utc};
    use crate::runtime::agenda::AgendaItemId;
    let id_str = input.get("id").and_then(Value::as_str)
        .ok_or_else(|| ToolError::InputValidationError {
            tool_name: "skip_occurrence".into(),
            message: "missing 'id'".into(),
        })?;
    let at_str = input.get("at").and_then(Value::as_str)
        .ok_or_else(|| ToolError::InputValidationError {
            tool_name: "skip_occurrence".into(),
            message: "missing 'at'".into(),
        })?;
    let at: DateTime<Utc> = at_str.parse()
        .map_err(|e: chrono::ParseError| ToolError::ExecutionFailed(format!("at parse: {e}")))?;
    let id = AgendaItemId(id_str.to_string());

    let item = self.deps.store.get(&id)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    if item.organizer_persona_id != self.deps.current_persona_id {
        return Err(ToolError::PermissionDenied("can only skip own agenda items".into()));
    }

    let updated = self.deps.store.set_skip(&id, at)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let json = serde_json::to_value(&updated).unwrap();
    Ok(ToolResult::new(
        "skip_occurrence",
        serde_json::to_string_pretty(&json).unwrap(),
        Some(json),
    ))
}
```

- [ ] **Step 3：跑测试**

```bash
cd src-tauri && cargo test --lib runtime::tools::builtin::agenda::skip::tests
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/agenda/skip.rs
git commit -m "feat(agenda): SkipOccurrenceRuntimeTool"
```

---

## 任务 52：实现 ListAgendaOccurrencesRuntimeTool

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/agenda/list_occurrences.rs`

- [ ] **Step 1：写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn list_occurrences_for_owned_item() {
        let dir = TempDir::new().unwrap();
        // create item via create tool
        let create = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "alice".into())),
        };
        let result = create.execute(
            json!({ "title": "T", "prompt": "P", "start_at": "2026-05-07T01:00:00Z" }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap();
        let id = serde_json::from_str::<serde_json::Value>(&result.content).unwrap()["id"].as_str().unwrap().to_string();

        let tool = ListAgendaOccurrencesRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "alice".into())),
        };
        let result = tool.execute(
            json!({ "agenda_item_id": id }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&result.content).unwrap();
        assert_eq!(arr.len(), 0);
    }

    #[tokio::test]
    async fn list_occurrences_others_item_denied() {
        let dir = TempDir::new().unwrap();
        let create = super::super::create::CreateAgendaItemRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "alice".into())),
        };
        let result = create.execute(
            json!({ "title": "T", "prompt": "P", "start_at": "2026-05-07T01:00:00Z" }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap();
        let id = serde_json::from_str::<serde_json::Value>(&result.content).unwrap()["id"].as_str().unwrap().to_string();

        let tool = ListAgendaOccurrencesRuntimeTool {
            deps: Arc::new(AgendaToolDeps::new(dir.path().to_path_buf(), "bob".into())),
        };
        let err = tool.execute(
            json!({ "agenda_item_id": id }),
            ToolExecutionContext::for_test("s", "r", "c"),
        ).await.unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
    }
}
```

- [ ] **Step 2：实现 execute**

```rust
async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
    use crate::runtime::agenda::AgendaItemId;
    let id_str = input.get("agenda_item_id").and_then(Value::as_str)
        .ok_or_else(|| ToolError::InputValidationError {
            tool_name: "list_agenda_occurrences".into(),
            message: "missing 'agenda_item_id'".into(),
        })?;
    let id = AgendaItemId(id_str.to_string());

    let item = self.deps.store.get(&id)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    if item.organizer_persona_id != self.deps.current_persona_id {
        return Err(ToolError::PermissionDenied("can only list own agenda occurrences".into()));
    }

    let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
    let occs = self.deps.store.list_occurrences(&id, limit)
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let json = serde_json::to_value(&occs).unwrap();
    Ok(ToolResult::new(
        "list_agenda_occurrences",
        serde_json::to_string_pretty(&json).unwrap(),
        Some(json),
    ))
}
```

- [ ] **Step 3：跑测试**

```bash
cd src-tauri && cargo test --lib runtime::tools::builtin::agenda::list_occurrences::tests
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/agenda/list_occurrences.rs
git commit -m "feat(agenda): ListAgendaOccurrencesRuntimeTool"
```

---

## 任务 53：注册 6 个工具到 catalog + request-scoped builder

**Files:**
- Modify: `src-tauri/src/runtime/tools/catalog.rs`
- Modify: `src-tauri/src/plugin/registry.rs`

- [ ] **Step 1：grep CatalogEntry 和 build_default_catalog**

```bash
cd src-tauri && grep -n "build_default_catalog\|CatalogEntry::new" src/runtime/tools/catalog.rs
```

- [ ] **Step 2：添加 6 个 CatalogEntry**

在 `build_default_catalog()` 末尾追加（参考已有 entry 的写法）：

```rust
catalog.register_entry(CatalogEntry::new(
    ToolDefinition::new("create_agenda_item", "创建一条日程，到指定时间触发")
        .with_kind(ToolKind::Primitive)
        .with_read_only(false),
    serde_json::json!({
        "type": "object",
        "required": ["title", "prompt", "start_at"],
        "properties": {
            "title": { "type": "string" },
            "prompt": { "type": "string", "description": "到点要执行的内容" },
            "start_at": { "type": "string", "format": "date-time" },
            "timezone": { "type": "string", "default": "Asia/Shanghai" },
            "rule": {
                "type": "object",
                "properties": {
                    "freq": { "type": "string", "enum": ["daily", "weekly", "monthly", "yearly"] },
                    "interval": { "type": "integer", "minimum": 1 },
                    "endCondition": {
                        "oneOf": [
                            { "type": "object", "properties": { "kind": { "const": "never" } }, "required": ["kind"] },
                            { "type": "object", "properties": { "kind": { "const": "count" }, "n": { "type": "integer" } }, "required": ["kind", "n"] },
                            { "type": "object", "properties": { "kind": { "const": "until" }, "at": { "type": "string", "format": "date-time" } }, "required": ["kind", "at"] }
                        ]
                    }
                },
                "required": ["freq", "interval", "endCondition"]
            }
        }
    }),
));

catalog.register_entry(CatalogEntry::new(
    ToolDefinition::new("list_agenda_items", "列出当前数字员工的日程")
        .with_kind(ToolKind::Primitive)
        .with_read_only(true),
    serde_json::json!({
        "type": "object",
        "properties": {
            "status_in": { "type": "array", "items": { "type": "string", "enum": ["active", "paused", "completed", "orphaned"] } },
            "limit": { "type": "integer", "default": 50 }
        }
    }),
));

catalog.register_entry(CatalogEntry::new(
    ToolDefinition::new("update_agenda_item", "修改自己创建的日程")
        .with_kind(ToolKind::Primitive)
        .with_read_only(false),
    serde_json::json!({
        "type": "object",
        "required": ["id"],
        "properties": {
            "id": { "type": "string" },
            "title": { "type": "string" },
            "prompt": { "type": "string" },
            "rule": {},
            "status": { "type": "string", "enum": ["active", "paused"] }
        }
    }),
));

catalog.register_entry(CatalogEntry::new(
    ToolDefinition::new("cancel_agenda_item", "删除自己创建的日程")
        .with_kind(ToolKind::Primitive)
        .with_read_only(false)
        .with_destructive(true),
    serde_json::json!({
        "type": "object",
        "required": ["id"],
        "properties": { "id": { "type": "string" } }
    }),
));

catalog.register_entry(CatalogEntry::new(
    ToolDefinition::new("skip_occurrence", "跳过循环日程的某一次")
        .with_kind(ToolKind::Primitive)
        .with_read_only(false),
    serde_json::json!({
        "type": "object",
        "required": ["id", "at"],
        "properties": {
            "id": { "type": "string" },
            "at": { "type": "string", "format": "date-time" }
        }
    }),
));

catalog.register_entry(CatalogEntry::new(
    ToolDefinition::new("list_agenda_occurrences", "查看自己日程的执行历史")
        .with_kind(ToolKind::Primitive)
        .with_read_only(true),
    serde_json::json!({
        "type": "object",
        "required": ["agenda_item_id"],
        "properties": {
            "agenda_item_id": { "type": "string" },
            "limit": { "type": "integer", "default": 20 }
        }
    }),
));
```

> `with_destructive` 方法存在性请 grep 验证，不存在就用 `default_destructive` 字段直接构造。

- [ ] **Step 3：把 6 个工具加进 DAILY_ALLOWED_TOOLS**

grep `DAILY_ALLOWED_TOOLS` 找到常量。在数组里追加 6 个工具 id：

```rust
"create_agenda_item",
"list_agenda_items",
"update_agenda_item",
"cancel_agenda_item",
"skip_occurrence",
"list_agenda_occurrences",
```

- [ ] **Step 4：在 `try_build_request_scoped_tool` 注入 deps**

打开 `src-tauri/src/plugin/registry.rs`，找到 `try_build_request_scoped_tool` 方法。在 match name 分支里追加：

```rust
"create_agenda_item" | "list_agenda_items" | "update_agenda_item"
| "cancel_agenda_item" | "skip_occurrence" | "list_agenda_occurrences" => {
    let deps = self.try_build_agenda_deps(deps_ctx)?;
    Some(self.make_agenda_tool(name, deps))
}
```

并在 impl 块里加辅助方法：

```rust
fn try_build_agenda_deps(
    &self,
    deps_ctx: &RequestScopedRuntimeDeps,
) -> Option<Arc<crate::runtime::tools::builtin::agenda::AgendaToolDeps>> {
    let base_dir = deps_ctx.user_paths.as_ref()?.base_dir().to_path_buf();
    let persona_id = deps_ctx.current_persona_id.clone()?;
    Some(Arc::new(
        crate::runtime::tools::builtin::agenda::AgendaToolDeps::new(base_dir, persona_id),
    ))
}

fn make_agenda_tool(
    &self,
    name: &str,
    deps: Arc<crate::runtime::tools::builtin::agenda::AgendaToolDeps>,
) -> Arc<dyn RuntimeTool> {
    use crate::runtime::tools::builtin::agenda::*;
    match name {
        "create_agenda_item" => Arc::new(CreateAgendaItemRuntimeTool { deps }),
        "list_agenda_items" => Arc::new(ListAgendaItemsRuntimeTool { deps }),
        "update_agenda_item" => Arc::new(UpdateAgendaItemRuntimeTool { deps }),
        "cancel_agenda_item" => Arc::new(CancelAgendaItemRuntimeTool { deps }),
        "skip_occurrence" => Arc::new(SkipOccurrenceRuntimeTool { deps }),
        "list_agenda_occurrences" => Arc::new(ListAgendaOccurrencesRuntimeTool { deps }),
        _ => unreachable!(),
    }
}
```

> `RequestScopedRuntimeDeps.user_paths` 和 `current_persona_id` 字段存在性需 grep 验证。如果没有 `user_paths`，用 `deps_ctx.path_resolver.resolve_paths()`。如果没有 `current_persona_id` 字段，需要在 `RequestScopedRuntimeDeps` 加一个，由 `send_message_with_overrides` 注入。

- [ ] **Step 5：cargo check + 全量 lib 测试**

```bash
cd src-tauri && cargo check && cargo test --lib runtime::tools::builtin::agenda
```

- [ ] **Step 6：Commit**

```bash
git add src-tauri/src/runtime/tools/catalog.rs src-tauri/src/plugin/registry.rs
git commit -m "feat(agenda): register 6 agenda tools in catalog + request-scoped builder"
```

---

## 任务 54：Persona 删除联动

**Files:**
- Modify: `src-tauri/src/commands/persona.rs`

- [ ] **Step 1：grep 现状**

```bash
cd src-tauri && grep -n "delete_persona" src/commands/persona.rs
```

- [ ] **Step 2：在 delete_persona 命令体里追加 mark_orphaned 调用**

定位到 delete_persona 函数体，在 `facade.persona_store().delete_persona(&id)` 后追加：

```rust
let resolver = app.state::<Arc<dyn crate::storage::UserScopedPathResolver>>();
if let Ok(paths) = resolver.require_paths() {
    let agenda_store = crate::runtime::agenda::AgendaStore::new(paths.base_dir());
    if let Err(e) = agenda_store.mark_orphaned_by_organizer(&id) {
        tracing::warn!(error = %e, "failed to mark agenda items as orphaned");
    }
}
```

> 如果 `app.state::<Arc<dyn UserScopedPathResolver>>` 取不到（管理的是具体类型），用具体类型 `app.state::<Arc<CurrentUserStorage>>()`。

- [ ] **Step 3：cargo check**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 4：Commit**

```bash
git add src-tauri/src/commands/persona.rs
git commit -m "feat(agenda): persona deletion marks owned agenda items as Orphaned"
```

---

## 任务 55：集成测试 agenda_persona_delete_test.rs

**Files:**
- Create: `src-tauri/tests/agenda_persona_delete_test.rs`

- [ ] **Step 1：写测试（直接测 store 的 mark_orphaned）**

```rust
use chrono::Utc;
use tempfile::TempDir;

use app_lib::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaStore, ItemStatus, Participant,
};

fn make(persona: &str) -> AgendaItem {
    let now = Utc::now();
    AgendaItem {
        id: AgendaItemId::new(),
        title: "T".into(),
        prompt: "P".into(),
        organizer_persona_id: persona.into(),
        participants: vec![Participant { persona_id: persona.into(), joined_at: now }],
        start_at: now,
        timezone: "UTC".into(),
        rule: None,
        skip_dates: vec![],
        next_fire_at: None,
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn deleting_persona_orphans_their_active_items() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let alice = store.create(make("alice")).unwrap();
    let bob = store.create(make("bob")).unwrap();

    let count = store.mark_orphaned_by_organizer("alice").unwrap();
    assert_eq!(count, 1);
    assert_eq!(store.get(&alice.id).unwrap().status, ItemStatus::Orphaned);
    assert_eq!(store.get(&bob.id).unwrap().status, ItemStatus::Active);
}

#[test]
fn orphaned_items_can_be_revived_by_assigning_new_organizer() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make("alice")).unwrap();
    store.mark_orphaned_by_organizer("alice").unwrap();

    let mut revived = store.get(&item.id).unwrap();
    revived.organizer_persona_id = "carol".into();
    revived.participants = vec![Participant {
        persona_id: "carol".into(),
        joined_at: Utc::now(),
    }];
    revived.status = ItemStatus::Active;

    let updated = store.update(revived).unwrap();
    assert_eq!(updated.organizer_persona_id, "carol");
    assert_eq!(updated.status, ItemStatus::Active);
}
```

- [ ] **Step 2：跑测试**

```bash
cd src-tauri && cargo test --test agenda_persona_delete_test
```

- [ ] **Step 3：Commit**

```bash
git add src-tauri/tests/agenda_persona_delete_test.rs
git commit -m "test(agenda): persona deletion → orphaned, revive via update"
```

---

## 任务 56：集成测试 agenda_runner_scope_test.rs + review_agenda_session_id.rs + review_agenda_phase1_constraints.rs

**Files:**
- Create: `src-tauri/tests/agenda_runner_scope_test.rs`
- Create: `src-tauri/tests/review_agenda_session_id.rs`
- Create: `src-tauri/tests/review_agenda_phase1_constraints.rs`

- [ ] **Step 1：agenda_runner_scope_test.rs**

```rust
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;

use app_lib::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaRunDispatcher, AgendaStore, ItemStatus, Participant,
    TriggerSource, run_due_once,
};

struct CountingDispatcher {
    count: Mutex<usize>,
}

#[async_trait]
impl AgendaRunDispatcher for CountingDispatcher {
    async fn dispatch(
        &self,
        _item: AgendaItem,
        _planned: DateTime<Utc>,
        _src: TriggerSource,
        _now: DateTime<Utc>,
    ) -> anyhow::Result<String> {
        *self.count.lock().unwrap() += 1;
        Ok("occ-x".into())
    }
}

fn make(persona: &str, when: DateTime<Utc>) -> AgendaItem {
    AgendaItem {
        id: AgendaItemId::new(),
        title: "T".into(),
        prompt: "P".into(),
        organizer_persona_id: persona.into(),
        participants: vec![Participant { persona_id: persona.into(), joined_at: when }],
        start_at: when,
        timezone: "UTC".into(),
        rule: None,
        skip_dates: vec![],
        next_fire_at: Some(when),
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        created_at: when,
        updated_at: when,
    }
}

#[tokio::test]
async fn switching_scope_dirs_picks_up_new_items() {
    let dispatcher = Arc::new(CountingDispatcher { count: Mutex::new(0) });
    let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();

    let scope_a = TempDir::new().unwrap();
    let scope_b = TempDir::new().unwrap();
    let store_a = AgendaStore::new(scope_a.path());
    let store_b = AgendaStore::new(scope_b.path());

    let due_at = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
    store_a.create(make("alice", due_at)).unwrap();
    store_b.create(make("bob", due_at)).unwrap();

    // 模拟 runner 在 tick 1 用 scope A
    run_due_once(&store_a, dispatcher.as_ref(), now).await.unwrap();
    assert_eq!(*dispatcher.count.lock().unwrap(), 1);

    // tick 2 切到 scope B
    run_due_once(&store_b, dispatcher.as_ref(), now).await.unwrap();
    assert_eq!(*dispatcher.count.lock().unwrap(), 2);
}
```

- [ ] **Step 2：跑测试**

```bash
cd src-tauri && cargo test --test agenda_runner_scope_test
```

- [ ] **Step 3：review_agenda_session_id.rs**

```rust
//! Architecture review: agenda dispatcher must use SessionId/RunId from runtime/ids.
//! Locks in spec §4.4: 触发链路必经 SessionId/RunId.

#[test]
fn occurrence_struct_uses_session_id_and_run_id() {
    let source = std::fs::read_to_string("src/runtime/agenda/occurrence.rs").unwrap();
    assert!(source.contains("session_id: SessionId"),
        "Occurrence must record session_id: SessionId");
    assert!(source.contains("run_id: RunId"),
        "Occurrence must record run_id: RunId");
    assert!(source.contains("use crate::runtime::ids::"),
        "Occurrence must import ids from runtime::ids module");
}

#[test]
fn agenda_dispatcher_wires_run_id_into_occurrence() {
    let chat = std::fs::read_to_string("src/transport/tauri_commands/chat.rs").unwrap();
    let in_impl = chat
        .split("impl crate::runtime::agenda::AgendaRunDispatcher")
        .nth(1)
        .expect("AgendaRunDispatcher impl block not found");
    assert!(in_impl.contains("RunId::new"),
        "AgendaRunDispatcher impl must construct RunId explicitly");
    assert!(in_impl.contains("session_id"),
        "AgendaRunDispatcher impl must record session_id on Occurrence");
}
```

- [ ] **Step 4：review_agenda_phase1_constraints.rs**

```rust
//! Architecture review: phase-1 constraints declared in spec §1.9 must remain in store.
//! Failing this test means a future PR loosened the rules without spec update.

#[test]
fn store_validates_all_five_phase1_constraints() {
    let source = std::fs::read_to_string("src/runtime/agenda/store.rs").unwrap();
    let assertions = [
        "participants.len() must be 1",
        "organizer must equal participants[0]",
        "override_of must be None",
        "by_day / by_month_day must be empty",
        "skip_dates only valid when rule is Some",
    ];
    for needle in assertions {
        assert!(
            source.contains(needle),
            "phase-1 constraint missing in store.rs: '{}'",
            needle
        );
    }
}

#[test]
fn organizer_immutable_unless_orphaned_kept() {
    let source = std::fs::read_to_string("src/runtime/agenda/store.rs").unwrap();
    assert!(
        source.contains("organizer can only change when status was Orphaned"),
        "organizer-immutable rule missing in store update path"
    );
}
```

- [ ] **Step 5：跑全部 review tests**

```bash
cd src-tauri && cargo test --tests review_agenda
```
预期：全部 PASS。

- [ ] **Step 6：Commit**

```bash
git add src-tauri/tests/agenda_runner_scope_test.rs src-tauri/tests/review_agenda_session_id.rs src-tauri/tests/review_agenda_phase1_constraints.rs
git commit -m "test(agenda): runner-scope integration + 2 review tests (session-id, phase1-constraints)"
```

---

## 任务 57：删除旧 schedule 模块

**Files:**
- Delete: `src-tauri/src/runtime/schedule.rs`
- Delete: `src-tauri/src/runtime/schedule_runner.rs`
- Delete: `src-tauri/src/commands/schedules.rs`
- Delete: `src-tauri/tests/schedule_commands_test.rs`
- Modify: `src-tauri/src/runtime/mod.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1：删除文件**

```bash
cd src-tauri
rm src/runtime/schedule.rs
rm src/runtime/schedule_runner.rs
rm src/commands/schedules.rs
rm tests/schedule_commands_test.rs
```

- [ ] **Step 2：从 mod.rs 移除引用**

`src/runtime/mod.rs` 删 `pub mod schedule;` 和 `pub mod schedule_runner;`。
`src/commands/mod.rs` 删 `pub mod schedules;`。

- [ ] **Step 3：lib.rs 删 invoke handler**

grep `commands::schedules::` 找到 3 处旧的 invoke 注册并删除。
grep `spawn_schedule_runner` 删除剩余 import / 启动调用（任务 21 留下的旧 spawn 代码）。

- [ ] **Step 4：lib.rs 加启动 info log（老 schedules 目录非空时）**

定位到 `spawn_agenda_runner` 调用上方，加：

```rust
if let Some(paths) = current_user_storage.resolve_paths() {
    let legacy = paths.base_dir().join("schedules");
    if legacy.is_dir()
        && std::fs::read_dir(&legacy).map(|mut d| d.next().is_some()).unwrap_or(false)
    {
        tracing::info!(
            path = %legacy.display(),
            "legacy schedules directory found; migration not implemented (skipped)"
        );
    }
}
```

- [ ] **Step 5：cargo check + cargo test**

```bash
cd src-tauri && cargo check && cargo test
```
预期：无 schedule 引用，全量测试 PASS。

- [ ] **Step 6：删除 ChatRunDispatcher 旧 impl（若存在）**

grep `impl crate::runtime::schedule_runner::ScheduleRunDispatcher` 在 chat.rs，应该有一处 impl block。删除它。

- [ ] **Step 7：cargo check 二次**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 8：Commit**

```bash
git add -A src-tauri/src/runtime/ src-tauri/src/commands/ src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/lib.rs
git rm src-tauri/tests/schedule_commands_test.rs
git commit -m "refactor(agenda): remove legacy schedule module (replaced by agenda)"
```

---

## 任务 58：最终全量验证 + 烟雾测试

**Files:** 无（人工执行）

- [ ] **Step 1：跑全量 Rust 测试**

```bash
cd src-tauri && cargo test
```
预期：全部 PASS，**含 8 个新增 tests/ 文件**。

- [ ] **Step 2：跑前端测试**

```bash
pnpm test
```
预期：全部 PASS。

- [ ] **Step 3：tsc 全量**

```bash
pnpm exec tsc --noEmit
```
预期：0 error。

- [ ] **Step 4：clippy 警告检查**

```bash
cd src-tauri && cargo clippy --all-targets -- -D warnings 2>&1 | grep -i agenda
```
预期：无 agenda 相关警告。

- [ ] **Step 5：手动 e2e 烟雾测试**

```bash
pnpm tauri:dev
```

逐项确认：
- 创建一次性日程 → 列表新行 + 频率描述准确
- 创建每天循环 → 频率描述显示"每天 HH:mm"
- 暂停/启用切换 → 视觉反馈
- 立即运行 → 1-2 秒内对应 persona 收到新对话 + 正确 prompt
- 等真正到点（设 1 分钟后）→ runner 自动触发 + occurrence 列表新增 Succeeded
- 在对话里让数字员工调 `create_agenda_item` → 创建成功且 organizer = 当前 persona
- 在对话里让员工调 `update_agenda_item` 改别人的（手动改 id 测试）→ PermissionDenied
- 删除一个 persona → UI 看到该 persona 的日程显示警示色 + "该员工已删除"

- [ ] **Step 6：把验证结果记到 PR 描述**

无 commit；写 PR description 时贴。

---

**PR-4 收尾检查：**

```bash
cd src-tauri && cargo test
pnpm test && pnpm exec tsc --noEmit
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

Tag `agenda-pr4-done`。

---

# Self-Review

## Spec 覆盖检查（对照 spec 11 节 + 3 附录）

| Spec 章节 | 任务覆盖 |
|---|---|
| §1 领域建模 | 任务 4-9（item / occurrence / 5 条约束 / status 转换） |
| §2 背景与目标 | 任务 1（笔误修复，无功能任务） |
| §3 持久化 | 任务 5（路径布局）+ 6-10（atomic_write_json + Mutex + JSONL 两段写） |
| §4 模块结构 | 任务 4（mod scaffolding）+ 19-21（dispatcher/runner）+ 22-26（commands）+ 57（删旧） |
| §5 触发与执行 | 任务 14（take_due/advance）+ 19（dispatcher）+ 20-21（runner） |
| §6 Tauri 命令（9 个） | 任务 22-26（list/get/create/update/delete/run_now/skip/unskip/list_occurrences） |
| §7 Agent 工具（6 个） | 任务 46-53（deps + 6 tools + catalog/registry） |
| §8 前端改造 | 任务 28-44（types/hook/page/row/template/editor/detail + 测试） |
| §9 Persona 删除联动 | 任务 9（mark_orphaned）+ 54（commands hook）+ 55（integration test） |
| §10 测试 | 各任务的 #[cfg(test)] + 任务 27/55/56（integration）+ 31/32/56（review tests） |
| §11 落地顺序 | PR 切分对齐：1.类型/store → 2.runner/cmd → 3.UI → 4.工具/persona/cleanup |
| §12 待确认事项 | 无 |
| 附录 A 现状映射 | 任务 28（前端类型迁移）+ 57（删旧后端） |
| 附录 B 留口子 | 任务 4（OverrideRef + by_day/by_month_day 字段）+ 6（约束）+ 56（review test 锁住） |
| 附录 C iCalendar 对照 | 字段命名已对齐（任务 4），无独立任务 |

## Placeholder 扫描

无 "TBD" / "implement later" / "fill in details"。每个 step 含具体代码或 grep 命令。

## 类型一致性检查

- `AgendaItemId` / `Occurrence::new_id` / `AgendaStore::new(base_dir)` 在所有任务一致
- 工具 id 字符串：`create_agenda_item / list_agenda_items / update_agenda_item / cancel_agenda_item / skip_occurrence / list_agenda_occurrences` 在 catalog / registry / DAILY_ALLOWED_TOOLS / 测试断言中拼写一致
- Tauri 命令 id：`list_agenda_items / get_agenda_item / create_agenda_item / update_agenda_item / delete_agenda_item / run_agenda_item_now / skip_occurrence / unskip_occurrence / list_agenda_occurrences`（9 个）
- TS 字段 camelCase（`organizerPersonaId / nextFireAt / skipDates`）vs Rust serde rename_all="camelCase"，匹配

## 跨任务方法签名一致

- `AgendaStore::create / get / list / update / delete / take_due / advance_after_fire / append_occurrence / list_occurrences / set_skip / unset_skip / mark_orphaned_by_organizer` —— 任务 5-15 各定义一处，PR-2/PR-4 调用处签名匹配
- `AgendaRunDispatcher::dispatch(item, planned_fire_at, trigger_source, now) -> Result<String>` —— 任务 19 定义，任务 25/56 调用一致
- `compute_next_fire_at(item, now) -> Option<DateTime<Utc>>` —— 任务 11-12 定义，任务 14（advance_after_fire）+ 23（create）+ 24（update）+ 47/49（工具）调用一致

## 已知风险点（实现时关注）

1. **`send_message_with_overrides` 的 `request.permission_mode` 字段是否存在**：任务 18 备注已警告，需 grep 验证。
2. **`build_system_prompt` 当前签名是否带 `request: &ChatTurnRequest`**：任务 17 提到要扩展签名，需先 grep 现状决定是否调整。
3. **`get_persona_by_id` vs `PersonaStore::get_persona`**：任务 17 备注两种 fallback 方案。
4. **Editor Sheet 的 Select 组件触发方式**：任务 37 测试可能需要按项目实际 ui 库调整。
5. **`RequestScopedRuntimeDeps` 是否带 `current_persona_id` 字段**：任务 53 备注需验证，没有则需要先扩展。

## 失败回滚策略

每个 PR 独立可 ship。如果 PR-N 中途失败：
- 已提交的任务保留（每个任务独立 commit）
- 未完成的任务用 `git revert` 回滚到 PR-(N-1) 末尾的 tag
- 修正问题后重新从该任务继续
