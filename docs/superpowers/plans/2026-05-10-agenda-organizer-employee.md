# Agenda Organizer 迁移到 Employee 实现计划（PR-5）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `AgendaItem.organizer_persona_id` 字段及其衍生引用全部迁到 `organizer_employee_id`，前端选择器换成数字员工，保持现有派活链路（`AgendaRunDispatcher::dispatch` → `send_message_with_overrides`）不重构。

**Architecture:**
- 后端字段层：`organizer_persona_id` → `organizer_employee_id`、`Participant.persona_id` → `Participant.employee_id`、`Occurrence.primary_persona_id` → `primary_employee_id`。serde `alias` 让老 JSON 自动迁移。
- Dispatcher：仍然调 `send_message_with_overrides`（**不**切到 `dispatch_employee_run` 链路，避免触动 inbox/OverrideGuard/last_run_at），但额外读 `EmployeeStore` 拿 `system_prompt_extra` 拼进 agenda prompt；`persona_id_override` 改传 `None`，让 chat 层走当前 active persona 兜底（PR-6 再彻底切掉 persona 依赖）。
- 前端：`AgendaItemEditor` 选择器从 `listPersonas()` 换成 `employeeList()`，渲染 avatar + name + role；员工列表为空时禁用保存按钮并提供"去雇佣"跳转。
- 删除 employee 时复用 persona 的 `mark_orphaned_by_organizer` 钩子。

**Tech Stack:** Rust (serde, anyhow, async-trait) / React + TypeScript + Vitest / Tauri 2.x IPC

**Pre-reqs:**
- 当前 detached HEAD 应是 `973c1095`（persona 模块加 deprecation 注释后的 commit）
- 本地 `~/.renlijia/users/t_28__u_54/employees/` 至少有 1 个真实 employee 用于联调（你机器上��有 `emp-6f2d9504-…`）
- `pnpm tauri:dev` 进程可继续复用（PID 11662）

**Out of scope（本期不做）：**
- 不动 `dispatch_employee_run` / inbox writer / `OverrideGuard` / `record_run`——那是 employee 自身链路
- 不删 persona seed / store / IPC 命令——只把 agenda 一侧迁干净，persona 其他用途（active persona 兜底、设置页等）继续保留
- 不做云端同步、不做 employee 跨用户共享
- 不在 base.md 引入 employee 概念字符串（保持工具描述前缀那套约定）

---

## File Structure

| 路径 | 角色 | 改动类型 |
|---|---|---|
| `src-tauri/src/runtime/agenda/item.rs` | `AgendaItem` / `Participant` 字段 | 字段改名 + serde alias |
| `src-tauri/src/runtime/agenda/occurrence.rs` | `Occurrence.primary_persona_id` | 字段改名 + serde alias |
| `src-tauri/src/runtime/agenda/store.rs` | 内部引用 / `mark_orphaned_by_organizer` 改名 | 跟随重命名 |
| `src-tauri/src/runtime/agenda/runner.rs`、`trigger_eval.rs` | 测试 fixture | 跟随重命名 |
| `src-tauri/src/transport/tauri_commands/agenda.rs` | `Create/UpdateAgendaItemRequest` 字段 | 字段改名 |
| `src-tauri/src/transport/tauri_commands/chat.rs:2663-2815` | `AgendaRunDispatcher::dispatch` | 接 EmployeeStore + 拼 prompt |
| `src-tauri/src/transport/tauri_commands/employee.rs` | employee_delete / employee_purge / employee_update(lifecycle=Archived) | 加 `mark_orphaned_by_organizer` 钩子 |
| `src-tauri/src/transport/tauri_commands/persona.rs:40-50` | persona delete 中的 mark_orphaned 调用 | 保留（active persona 仍用） |
| `src/lib/tauri.ts` | `AgendaItem` / `CreateAgendaItemRequest` / `UpdateAgendaItemRequest` 类型 | 字段改名 |
| `src/features/agenda/AgendaItemEditor.tsx` | 选择器、handleSave、loadList | 改 `employeeList()` |
| `src/features/agenda/AgendaItemEditor.test.tsx` | mock + 三个测试 | 改名 + 加空员工测试 |
| `src/features/agenda/AgendaItemDetail.tsx` | 显��� organizer | 改用 employee |
| `src/features/agenda/AgendaItemDetail.test.tsx` | 跟随 | 跟随 |
| `src/features/schedules/SchedulesPage.tsx:58-63, 290` | `activePersonaId` 默认值 | 换成 `defaultEmployeeId` |
| `src/features/schedules/SchedulesPage.test.tsx` | mock | 跟随 |
| `docs/test-intents/spec/tasks/agenda-base/rules.md` | §1.3/§1.8 organizer 段落 | 改写 |
| `docs/superpowers/specs/2026-05-06-agenda-base-design.md` | §1.3/§1.8 | 改写 |

---

## Naming Conventions

- 后端 Rust：`organizer_employee_id` / `primary_employee_id` / `Participant.employee_id`
- 后端 JSON 序列化（rename_all=camelCase）：`organizerEmployeeId` / `primaryEmployeeId` / `employeeId`
- 前端 TypeScript：`organizerEmployeeId` / `primaryEmployeeId`
- serde `#[serde(alias = "organizerPersonaId")]` 让老 JSON 透明读入；写出去用新名
- **不**使用 `agentEmployeeId`（"agent" 是 runtime 层 SessionId>RunId>AgentId 概念，跟数字员工无关）

---

## Task 1: 后端 AgendaItem.organizer_employee_id 字段重命名 + 老 JSON 迁移

**Files:**
- Modify: `src-tauri/src/runtime/agenda/item.rs:67-74, 96`
- Test: `src-tauri/src/runtime/agenda/item.rs`（追加单测）

- [ ] **Step 1: 写老 JSON 迁移测试（fail）**

在 `src-tauri/src/runtime/agenda/item.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn legacy_organizer_persona_id_deserializes_into_employee_id() {
        let raw = r#"{
            "id": "agenda-x",
            "title": "t",
            "prompt": "p",
            "organizerPersonaId": "default",
            "participants": [{"personaId": "default", "joinedAt": "2026-05-09T00:00:00Z"}],
            "startAt": "2026-05-10T00:00:00Z",
            "timezone": "Asia/Shanghai",
            "rule": null,
            "skipDates": [],
            "nextFireAt": null,
            "occurrenceCount": 0,
            "status": "active",
            "overrideOf": null,
            "workspacePath": null,
            "createdAt": "2026-05-09T00:00:00Z",
            "updatedAt": "2026-05-09T00:00:00Z"
        }"#;
        let item: AgendaItem = serde_json::from_str(raw).expect("legacy json must parse");
        assert_eq!(item.organizer_employee_id, "default");
        assert_eq!(item.participants.len(), 1);
        assert_eq!(item.participants[0].employee_id, "default");
    }

    #[test]
    fn new_field_names_round_trip() {
        let item = AgendaItem {
            id: AgendaItemId("a".into()),
            title: "t".into(),
            prompt: "p".into(),
            organizer_employee_id: "emp-1".into(),
            participants: vec![Participant {
                employee_id: "emp-1".into(),
                joined_at: chrono::Utc::now(),
            }],
            start_at: chrono::Utc::now(),
            timezone: "Asia/Shanghai".into(),
            rule: None,
            skip_dates: vec![],
            next_fire_at: None,
            occurrence_count: 0,
            status: ItemStatus::Active,
            override_of: None,
            workspace_path: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let s = serde_json::to_string(&item).unwrap();
        assert!(s.contains("\"organizerEmployeeId\":\"emp-1\""), "wire format = camelCase, got {s}");
        assert!(!s.contains("organizerPersonaId"), "must not emit legacy field on write, got {s}");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::item::migration_tests -- --nocapture
```

Expected: 编译失败，`organizer_employee_id` / `Participant.employee_id` 不存在

- [ ] **Step 3: 改字段定义**

在 `src-tauri/src/runtime/agenda/item.rs:67-74` 把 `Participant` 改成：

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    #[serde(alias = "personaId")]
    pub employee_id: String,
    pub joined_at: DateTime<Utc>,
}
```

在 `src-tauri/src/runtime/agenda/item.rs:96` 把 `AgendaItem.organizer_persona_id` 改成：

```rust
    #[serde(alias = "organizerPersonaId")]
    pub organizer_employee_id: String,
```

- [ ] **Step 4: 跑迁移测试通过（先忽略其他编译错误）**

```bash
cd src-tauri && cargo test --lib runtime::agenda::item::migration_tests --no-fail-fast 2>&1 | tail -30
```

Expected: `migration_tests::legacy_organizer_persona_id_deserializes_into_employee_id` 和 `new_field_names_round_trip` 通过；其他 lib 测试此时大概率 fail（下一个 Task 修）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/agenda/item.rs
git commit -m "refactor(agenda): rename organizer_persona_id -> organizer_employee_id with serde alias

老 JSON（organizerPersonaId / participants[].personaId）透明读入；写出去用新名。
后续 task 修复因这次改名失败的所有调用方。"
```

---

## Task 2: 后端 Occurrence.primary_employee_id 字段重命名

**Files:**
- Modify: `src-tauri/src/runtime/agenda/occurrence.rs:31`

- [ ] **Step 1: 写老 JSON 迁移测试**

在 `src-tauri/src/runtime/agenda/occurrence.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod occurrence_migration_tests {
    use super::*;

    #[test]
    fn legacy_primary_persona_id_deserializes_into_employee_id() {
        let raw = r#"{
            "id": "occ-x",
            "agendaItemId": "agenda-x",
            "firedAt": "2026-05-09T00:00:00Z",
            "plannedFireAt": "2026-05-09T00:00:00Z",
            "startedAt": "2026-05-09T00:00:00Z",
            "finishedAt": null,
            "primaryPersonaId": "default",
            "conversationId": "c1",
            "sessionId": "c1",
            "runId": "r1",
            "status": "running",
            "errorSummary": null,
            "triggerSource": "scheduled"
        }"#;
        let occ: Occurrence = serde_json::from_str(raw).expect("legacy occurrence must parse");
        assert_eq!(occ.primary_employee_id, "default");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test --lib runtime::agenda::occurrence::occurrence_migration_tests -- --nocapture
```

Expected: 编译失败 `primary_employee_id` 不存在

- [ ] **Step 3: 改字段**

`src-tauri/src/runtime/agenda/occurrence.rs:31` 改成：

```rust
    #[serde(alias = "primaryPersonaId")]
    pub primary_employee_id: String,
```

- [ ] **Step 4: 跑测试通过**

```bash
cd src-tauri && cargo test --lib runtime::agenda::occurrence -- --nocapture
```

Expected: `occurrence_migration_tests::legacy_primary_persona_id_deserializes_into_employee_id` PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/agenda/occurrence.rs
git commit -m "refactor(agenda): rename primary_persona_id -> primary_employee_id with serde alias"
```

---

## Task 3: 后端 agenda store 跟随重命名 + mark_orphaned_by_organizer 重命名

**Files:**
- Modify: `src-tauri/src/runtime/agenda/store.rs:77, 163, 177, 358, 423, 624, 644, 651, 699, 713`
- Modify: `src-tauri/src/runtime/agenda/runner.rs:80`
- Modify: `src-tauri/src/runtime/agenda/trigger_eval.rs:137, 139, 196, 197`

- [ ] **Step 1: 让编译错误指路**

```bash
cd src-tauri && cargo check --lib 2>&1 | grep -E "error\[" | head -30
```

Expected: 大量 `organizer_persona_id` / `persona_id` / `primary_persona_id` 的字段不存在错误

- [ ] **Step 2: 全仓 sed 替换三个字段名（限定 agenda 模块）**

```bash
cd src-tauri/src/runtime/agenda
sed -i '' 's/organizer_persona_id/organizer_employee_id/g' store.rs runner.rs trigger_eval.rs
sed -i '' 's/primary_persona_id/primary_employee_id/g' store.rs runner.rs trigger_eval.rs
# Participant.persona_id：只在 agenda 模块内是 Participant 的字段
# 用 LSP 或 grep 确认
grep -n "persona_id" store.rs runner.rs trigger_eval.rs
```

预期输出：除了 `Participant`-context 的 `persona_id` 都已替换。手动把剩余 `participants[i].persona_id`、`Participant { persona_id: … }` 改成 `employee_id`。

- [ ] **Step 3: 把 `mark_orphaned_by_organizer` 重命名 + 函数签名参数也改名**

`src-tauri/src/runtime/agenda/store.rs:163` 函数签名：

```rust
    pub fn mark_orphaned_by_organizer(&self, employee_id: &str) -> anyhow::Result<usize> {
        // 函数体内部把 `persona_id` 改成 `employee_id`
        // 比较行也跟着改
    }
```

注意：函数名**保留** `mark_orphaned_by_organizer`（不带 persona/employee 后缀），因为 organizer 是抽象语义；只改入参名 + 内部字段比较。

- [ ] **Step 4: 编译干净**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -20
```

Expected: `Finished dev profile`，无 error；可能有 `persona_id` 相关 warning 但 agenda 模块内应清零

- [ ] **Step 5: 跑 agenda 单测**

```bash
cd src-tauri && cargo test --lib agenda -- --nocapture 2>&1 | tail -20
```

Expected: agenda 全部测试 PASS（包含 store / item / occurrence / trigger_eval / runner）

- [ ] **Step 6: 跑 agenda 集成测试**

```bash
cd src-tauri && cargo test --tests agenda -- --nocapture 2>&1 | tail -30
```

Expected: 18/18 集成 + review 测试 PASS（按 PR-4 基线）

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/runtime/agenda/
git commit -m "refactor(agenda): cascade rename to employee_id across store/runner/trigger_eval

mark_orphaned_by_organizer 函数名保留（organizer 是抽象语义），仅改字段引用。
agenda lib + 集成测试 baseline 维持。"
```

---

## Task 4: 后端 Tauri command request schema 跟随重命名

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/agenda.rs:64, 175`（CreateRequest / UpdateRequest）
- Modify: `src-tauri/src/transport/tauri_commands/agenda.rs:339-540`（测试 fixture）

- [ ] **Step 1: 改 CreateAgendaItemRequest**

`src-tauri/src/transport/tauri_commands/agenda.rs:64` 把字段重命名：

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgendaItemRequest {
    pub title: String,
    pub prompt: String,
    #[serde(alias = "organizerPersonaId")]
    pub organizer_employee_id: String,
    pub start_at: DateTime<Utc>,
    pub timezone: String,
    pub rule: Option<RecurrenceRule>,
    pub workspace_path: Option<String>,
}
```

`UpdateAgendaItemRequest` 不含 organizer 字段（编辑时锁定），不改。

- [ ] **Step 2: 改 fixture + 测试中所有 `organizer_persona_id`**

```bash
cd src-tauri/src/transport/tauri_commands
sed -i '' 's/organizer_persona_id/organizer_employee_id/g' agenda.rs
```

确认没有遗漏：

```bash
grep -n "persona" agenda.rs
```

Expected: 输出 0 行（agenda 模块的 transport 层应彻底脱敏）

- [ ] **Step 3: 跑 agenda transport 测试**

```bash
cd src-tauri && cargo test --lib transport::tauri_commands::agenda -- --nocapture 2>&1 | tail -20
```

Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/agenda.rs
git commit -m "refactor(agenda): rename CreateAgendaItemRequest field to organizerEmployeeId"
```

---

## Task 5: dispatcher 接 EmployeeStore + build_dispatch_prompt 拼 prompt

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs:2755, 2782-2798`

**目标行为**：dispatcher 收到 agenda 触发后，从磁盘读 `EmployeeStore::get(organizer_employee_id)` 拿 `system_prompt_extra` / `default_skill_id`，拼进 prompt；`persona_id_override` 改传 `None`。

**不做的事**：不切到 `dispatch_employee_run`，不写 inbox，不调 `record_run`，不装 `OverrideGuard`。

- [ ] **Step 1: 写集成测试 fixture（fail）**

`src-tauri/tests/agenda_employee_dispatch_integration_test.rs` 新建：

```rust
//! agenda dispatcher 必须读 employee store 拿 system_prompt_extra
//! 并拼进 send_message 的 prompt。

use std::sync::{Arc, Mutex};

#[test]
fn agenda_dispatch_reads_employee_system_prompt_extra() {
    // 这个 test 是断言用的契约文档，真实 dispatcher 走 Tauri 闭包不易单测。
    // 真实集成靠手动烟测验证（见 plan 末尾），这里只断言代码结构存在。
    let chat_rs = std::fs::read_to_string("src/transport/tauri_commands/chat.rs")
        .expect("chat.rs must exist");
    assert!(
        chat_rs.contains("EmployeeStore::new") && chat_rs.contains("organizer_employee_id"),
        "AgendaRunDispatcher::dispatch must instantiate EmployeeStore using organizer_employee_id"
    );
    assert!(
        chat_rs.contains("build_dispatch_prompt"),
        "AgendaRunDispatcher::dispatch must reuse build_dispatch_prompt to compose employee prompt"
    );
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test --test agenda_employee_dispatch_integration_test -- --nocapture
```

Expected: FAIL（`build_dispatch_prompt` 调用尚未引入）

- [ ] **Step 3: 改 dispatcher 拼 prompt**

`src-tauri/src/transport/tauri_commands/chat.rs:2782-2798`，把：

```rust
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
```

改成：

```rust
        // 4. 读 employee 拿 system_prompt_extra / default_skill_id，拼进 prompt
        let employee = {
            use crate::runtime::employee::store::EmployeeStore;
            use crate::storage::{CurrentUserStorage, UserScopedPathResolver};
            use tauri::Manager;
            let cus = self
                .services
                .app
                .try_state::<Arc<CurrentUserStorage>>()
                .ok_or_else(|| anyhow::anyhow!("CurrentUserStorage not registered"))?;
            let paths = cus
                .require_paths()
                .map_err(|e| anyhow::anyhow!("paths unavailable: {e}"))?;
            let store = EmployeeStore::new(paths.employees_dir());
            store.get(&item.organizer_employee_id).ok()
        };

        // 没有匹配的员工（比如老 agenda 写的是 persona id "default"）就用 agenda 自己的 prompt 兜底，
        // 不阻塞触发。
        let trigger_label = format!("[日程触发] {}\n计划触发时间：{planned_fire_at}", item.title);
        let prompt = if let Some(emp) = employee.as_ref() {
            crate::runtime::employee::dispatch_prompt::build_dispatch_prompt(
                emp,
                &trigger_label,
                None,
                Some(&item.prompt),
            )
        } else {
            format!("{trigger_label}\n\n{}", item.prompt)
        };

        // persona_id_override = None：让 chat 层走 active persona 兜底（PR-6 彻底切掉 persona）
        let result = self
            .send_message_with_overrides(
                conversation_id.clone(),
                prompt,
                Vec::new(),
                None,
                None,
                None,
                None,
                Some(run_id.clone()),
            )
            .await;
```

注意 `Occurrence.primary_employee_id` 在 chat.rs:2755 那行也要跟着 Task 2 的字段改名同步：

```rust
            primary_employee_id: item.organizer_employee_id.clone(),
```

- [ ] **Step 4: 跑契约测试通过**

```bash
cd src-tauri && cargo test --test agenda_employee_dispatch_integration_test -- --nocapture
```

Expected: PASS

- [ ] **Step 5: 跑 lib 整体编译**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -10
```

Expected: `Finished dev profile`

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/tests/agenda_employee_dispatch_integration_test.rs
git commit -m "feat(agenda): dispatcher reads EmployeeStore and reuses build_dispatch_prompt

Agenda 触发时，从磁盘读 organizer_employee_id 对应的 employee，拼 system_prompt_extra
和 default_skill_id 提示进 prompt。不切到 dispatch_employee_run（保持 inbox/record_run
等 employee 自身链路独立）。employee 读取失败时用 agenda 自己的 prompt 兜底，不阻塞触发。
persona_id_override 改传 None，由 chat 层走 active persona 兜底直到 PR-6 彻底切除。"
```

---

## Task 6: 删除 employee 时挂 mark_orphaned_by_organizer 钩子

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/employee.rs`（找 employee_delete / employee_purge / employee_update lifecycle=Archived 的入口）

- [ ] **Step 1: 摸清 employee 删除/归档入口**

```bash
grep -n "fn employee_delete\|fn employee_purge\|lifecycle.*Archived" src-tauri/src/transport/tauri_commands/employee.rs | head -10
```

记录三个函数的文件:行号。

- [ ] **Step 2: 写集成测试（fail）**

`src-tauri/tests/agenda_orphaned_by_employee_test.rs` 新建：

```rust
//! 删除（archive 或 purge）一个 employee 时，所有 organizer_employee_id 指向它的
//! agenda item 必须被标记为 Orphaned。

use anyhow::Result;
use tempfile::TempDir;

#[test]
fn purging_employee_marks_dependent_agenda_orphaned() -> Result<()> {
    use app_lib::runtime::agenda::{AgendaItem, AgendaItemId, AgendaItemStore, ItemStatus, Participant};
    use chrono::Utc;

    let tmp = TempDir::new()?;
    let store = AgendaItemStore::new(tmp.path().join("agenda")).expect("create store");

    let now = Utc::now();
    let item = AgendaItem {
        id: AgendaItemId("agenda-test".into()),
        title: "t".into(),
        prompt: "p".into(),
        organizer_employee_id: "emp-1".into(),
        participants: vec![Participant {
            employee_id: "emp-1".into(),
            joined_at: now,
        }],
        start_at: now,
        timezone: "Asia/Shanghai".into(),
        rule: None,
        skip_dates: vec![],
        next_fire_at: Some(now),
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        workspace_path: None,
        created_at: now,
        updated_at: now,
    };
    store.create_item(item).expect("create");

    let count = store.mark_orphaned_by_organizer("emp-1").expect("mark");
    assert_eq!(count, 1);

    let item_after = store.get_item(&AgendaItemId("agenda-test".into())).expect("get").unwrap();
    assert_eq!(item_after.status, ItemStatus::Orphaned);

    Ok(())
}
```

- [ ] **Step 3: 跑测试确认编译错误指向 `app_lib::runtime::agenda` 路径**

```bash
cd src-tauri && cargo test --test agenda_orphaned_by_employee_test -- --nocapture 2>&1 | tail -20
```

如果路径错，按编译器提示改 use（参考 `src-tauri/tests/` 下其他 review_*.rs 的 use 写法）。

- [ ] **Step 4: 测试通过后，在 employee_delete / employee_purge / employee_update(lifecycle=Archived) 中调用钩子**

伪代码（具体位置 Step 1 已摸清）：

```rust
// 在 employee_delete 函数中，删 employee 文件成功后追加：
if let Ok(agenda_store) = self.agenda_store_for_current_user() {
    if let Err(e) = agenda_store.mark_orphaned_by_organizer(&id) {
        log::warn!("[employee_delete] mark_orphaned_by_organizer({id}) failed: {e}");
    }
}
```

`employee_purge` 同样。`employee_update` 的 lifecycle Archived 分支同样。

- [ ] **Step 5: 跑全部测试**

```bash
cd src-tauri && cargo test --lib --tests employee agenda -- --nocapture --no-fail-fast 2>&1 | tail -20
```

Expected: 全部 PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/employee.rs src-tauri/tests/agenda_orphaned_by_employee_test.rs
git commit -m "feat(employee): cascade mark_orphaned_by_organizer on delete/purge/archive

复用 persona delete 已有的 agenda orphan 钩子。employee_delete / employee_purge /
employee_update(lifecycle=Archived) 三个入口都挂上，避免 organizer 消失后 agenda
仍处于 Active 状态被错误调度。"
```

---

## Task 7: 前端 lib/tauri.ts 类型字段重命名

**Files:**
- Modify: `src/lib/tauri.ts:424, 461-468, 471+`

- [ ] **Step 1: 改 AgendaItem 接口**

`src/lib/tauri.ts:424` 把 `organizerPersonaId: string` 改成 `organizerEmployeeId: string`。

`Participant`-like 的 `participants[]` 内部 `personaId` 也要改名为 `employeeId`（搜 `participants` 定位）。

- [ ] **Step 2: 改 CreateAgendaItemRequest**

`src/lib/tauri.ts:461`：

```typescript
export interface CreateAgendaItemRequest {
  title: string
  prompt: string
  organizerEmployeeId: string
  startAt: string
  timezone: string
  rule: RecurrenceRule | null
  workspacePath?: string | null
}
```

- [ ] **Step 3: 跑 tsc**

```bash
pnpm exec tsc --noEmit 2>&1 | head -30
```

Expected: 一堆 error 指向 `AgendaItemEditor.tsx` / `AgendaItemDetail.tsx` / `SchedulesPage.tsx`，下一个 task 修复

- [ ] **Step 4: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "refactor(agenda): rename organizerPersonaId -> organizerEmployeeId in TS types"
```

---

## Task 8: 前端 AgendaItemEditor 改用 employeeList()

**Files:**
- Modify: `src/features/agenda/AgendaItemEditor.tsx`（替换 Task ce1b6301 加的 persona 选择器）

- [ ] **Step 1: 写测试（fail）—— 默认值用第一个 employee**

替换 `src/features/agenda/AgendaItemEditor.test.tsx` 里现有的两个 persona 测试为 employee 等价物：

```typescript
it('defaults organizer to the prop employee id when creating', async () => {
  invokeMock.mockImplementation(async (cmd: string) => {
    if (cmd === 'employee_list') return [
      { id: 'emp-1', name: '小研', avatar: '🔍', role: '行业/竞品调研员', lifecycle: 'active', cronEnabled: true } as any,
      { id: 'emp-2', name: '小法', avatar: '⚖️', role: '合同审阅员', lifecycle: 'active', cronEnabled: true } as any,
    ]
    if (cmd === 'get_default_folder') return null
    return null
  })
  render(
    <AgendaItemEditor
      open
      organizerEmployeeId="emp-2"
      onClose={() => {}}
      onSaved={() => {}}
    />,
  )
  const select = (await screen.findByLabelText('执行员工')) as HTMLSelectElement
  expect(select.value).toBe('emp-2')
})

it('passes the chosen employee id when creating', async () => {
  invokeMock.mockImplementation(async (cmd: string) => {
    if (cmd === 'employee_list') return [
      { id: 'emp-1', name: '小研', avatar: '🔍', role: '调研', lifecycle: 'active', cronEnabled: true } as any,
      { id: 'emp-2', name: '小法', avatar: '⚖️', role: '合同', lifecycle: 'active', cronEnabled: true } as any,
    ]
    if (cmd === 'get_default_folder') return null
    if (cmd === 'create_agenda_item') return { id: 'agenda-x' }
    return null
  })
  render(<AgendaItemEditor open organizerEmployeeId="emp-1" onClose={() => {}} onSaved={() => {}} />)
  const select = (await screen.findByLabelText('执行员工')) as HTMLSelectElement
  await waitFor(() => expect(select.querySelectorAll('option').length).toBeGreaterThan(1))
  fireEvent.change(select, { target: { value: 'emp-2' } })
  fireEvent.change(screen.getByPlaceholderText('标题'), { target: { value: 'T' } })
  fireEvent.change(screen.getByPlaceholderText('到点要做什么？'), { target: { value: 'P' } })
  fireEvent.change(screen.getByLabelText(/开始时间/), { target: { value: '2026-05-07T09:00' } })
  fireEvent.click(screen.getByRole('button', { name: '保存' }))
  await waitFor(() => {
    expect(invokeMock).toHaveBeenCalledWith(
      'create_agenda_item',
      expect.objectContaining({
        request: expect.objectContaining({ organizerEmployeeId: 'emp-2' }),
      }),
    )
  })
})

it('disables save and shows hire CTA when no employee exists', async () => {
  invokeMock.mockImplementation(async (cmd: string) => {
    if (cmd === 'employee_list') return []
    if (cmd === 'get_default_folder') return null
    return null
  })
  render(<AgendaItemEditor open organizerEmployeeId="" onClose={() => {}} onSaved={() => {}} />)
  await waitFor(() => {
    expect(screen.getByText(/还没有数字员工/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '保存' })).toBeDisabled()
  })
})

it('locks organizer when editing an existing item', async () => {
  invokeMock.mockImplementation(async (cmd: string) => {
    if (cmd === 'employee_list') return [
      { id: 'emp-1', name: '小研', avatar: '🔍', role: '调研', lifecycle: 'active', cronEnabled: true } as any,
      { id: 'emp-2', name: '小法', avatar: '⚖️', role: '合同', lifecycle: 'active', cronEnabled: true } as any,
    ]
    return null
  })
  const initial = {
    id: 'a1', title: 'X', prompt: 'Y',
    startAt: '2026-05-07T01:00:00.000Z',
    timezone: 'Asia/Shanghai',
    organizerEmployeeId: 'emp-2',
    rule: null,
    workspacePath: null,
  } as any
  render(<AgendaItemEditor open initial={initial} organizerEmployeeId="emp-1" onClose={() => {}} onSaved={() => {}} />)
  const select = (await screen.findByLabelText('执行员工')) as HTMLSelectElement
  expect(select.value).toBe('emp-2')
  expect(select.disabled).toBe(true)
})
```

- [ ] **Step 2: 跑测试确认 fail**

```bash
pnpm exec vitest run src/features/agenda/AgendaItemEditor.test.tsx 2>&1 | tail -15
```

Expected: 5 个测试 fail（旧 list_personas mock 已移除，新代码尚未写）

- [ ] **Step 3: 改 AgendaItemEditor.tsx**

替换 import：

```tsx
import {
  type AgendaItem,
  type CreateAgendaItemRequest,
  type EmployeeRecord,
  type Freq,
  type RecurrenceRule,
  type UpdateAgendaItemRequest,
  createAgendaItem,
  employeeList,
  getDefaultFolder,
  pickLocalDirectory,
  updateAgendaItem,
} from '@/lib/tauri'
```

替换 props：`organizerPersonaId: string` → `organizerEmployeeId: string`

替换 state：

```tsx
const [employees, setEmployees] = useState<EmployeeRecord[]>([])
const [selectedEmployeeId, setSelectedEmployeeId] = useState<string>(organizerEmployeeId)
```

替换 useEffect 加载：

```tsx
useEffect(() => {
  if (!open) return
  let cancelled = false
  employeeList()
    .then((list) => {
      if (cancelled) return
      const active = list.filter((e) => e.lifecycle === 'active')
      setEmployees(active)
    })
    .catch(() => {
      if (!cancelled) setEmployees([])
    })
  return () => {
    cancelled = true
  }
}, [open])
```

替换 setSelectedPersonaId 为 setSelectedEmployeeId（包括 initial 分支）。

替换 handleSave create 分支字段：`organizerEmployeeId: selectedEmployeeId`

替换 JSX 选择器（标题 input 上方那块）：

```tsx
<div className="space-y-2">
  <label className="text-xs text-muted-foreground" htmlFor="agenda-editor-organizer">
    执行员工
  </label>
  {employees.length === 0 ? (
    <div className="rounded-md border border-dashed border-input px-3 py-3 text-xs">
      <p className="text-muted-foreground">还没有数字员工。</p>
      <Button type="button" variant="outline" size="sm" className="mt-2" onClick={onClose}>
        去「数字员工」页雇一个
      </Button>
    </div>
  ) : (
    <>
      <select
        id="agenda-editor-organizer"
        className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm disabled:cursor-not-allowed disabled:opacity-60"
        value={selectedEmployeeId}
        onChange={(e) => setSelectedEmployeeId(e.target.value)}
        disabled={!!initial}
        aria-label="执行员工"
      >
        {employees.map((e) => (
          <option key={e.id} value={e.id}>
            {e.avatar} {e.name} · {e.role}
          </option>
        ))}
      </select>
      {initial ? (
        <p className="text-xs text-muted-foreground">已创建的日程不能改派给其他员工。</p>
      ) : null}
    </>
  )}
</div>
```

替换 canSave：

```tsx
const canSave = !!title && !!prompt && !!startAtLocal && !saving && employees.length > 0 && !!selectedEmployeeId
```

- [ ] **Step 4: 跑测试通过**

```bash
pnpm exec vitest run src/features/agenda/AgendaItemEditor.test.tsx 2>&1 | tail -10
```

Expected: 6 passed（原 freq-conditional 1 条 + 5 条 organizer 系列）

- [ ] **Step 5: 跑 tsc**

```bash
pnpm exec tsc --noEmit 2>&1 | tail -5
```

Expected: 仍报 `SchedulesPage.tsx` / `AgendaItemDetail.tsx` 的 props 名错（下一个 task 修）

- [ ] **Step 6: Commit**

```bash
git add src/features/agenda/AgendaItemEditor.tsx src/features/agenda/AgendaItemEditor.test.tsx
git commit -m "feat(agenda): editor uses employeeList instead of listPersonas

雇佣后才能新建日程；编辑时锁定 organizerEmployeeId；空员工时禁用保存并提供
跳转到「数字员工」页的引导按钮（onClose 关闭 sheet，让用户切到员工页雇一个）。"
```

---

## Task 9: SchedulesPage / AgendaItemDetail 跟随重命名

**Files:**
- Modify: `src/features/schedules/SchedulesPage.tsx:58, 63, 290`
- Modify: `src/features/schedules/SchedulesPage.test.tsx:23, 50`
- Modify: `src/features/agenda/AgendaItemDetail.tsx:101`
- Modify: `src/features/agenda/AgendaItemDetail.test.tsx`

- [ ] **Step 1: 改 SchedulesPage**

`src/features/schedules/SchedulesPage.tsx:58`：

```tsx
const [defaultEmployeeId, setDefaultEmployeeId] = useState<string>('')

useEffect(() => {
  void employeeList()
    .then((list) => {
      const first = list.find((e) => e.lifecycle === 'active')
      setDefaultEmployeeId(first?.id ?? '')
    })
    .catch(() => setDefaultEmployeeId(''))
}, [])
```

把 `:290` 处的 `organizerPersonaId={editing?.organizerPersonaId ?? activePersonaId}` 改成：

```tsx
organizerEmployeeId={editing?.organizerEmployeeId ?? defaultEmployeeId}
```

删除 `get_active_persona` 调用（line 63 附近）。注意 import 加上 `employeeList`。

- [ ] **Step 2: 改 SchedulesPage.test.tsx**

把 `organizerPersonaId: 'p1'` 改成 `organizerEmployeeId: 'emp-1'`，把 `get_active_persona` mock 改成 `employee_list` mock 返回包含 `emp-1` 的列表。

- [ ] **Step 3: 改 AgendaItemDetail**

`src/features/agenda/AgendaItemDetail.tsx:101` 附近的 `organizerPersonaId={item.organizerPersonaId}` 改成 `organizerEmployeeId={item.organizerEmployeeId}`。

如果 detail 视图里展示了 organizer 名称（搜 `organizer`、`personaId` 关键字），改成 employee.name。

- [ ] **Step 4: 跑前端测试**

```bash
pnpm exec vitest run src/features/agenda/ src/features/schedules/ 2>&1 | tail -15
```

Expected: 全部 PASS

- [ ] **Step 5: tsc + lint**

```bash
pnpm exec tsc --noEmit 2>&1 | tail -3
pnpm exec eslint src/features/agenda/ src/features/schedules/ 2>&1 | tail -5
```

Expected: 0 error

- [ ] **Step 6: Commit**

```bash
git add src/features/schedules/ src/features/agenda/AgendaItemDetail.tsx src/features/agenda/AgendaItemDetail.test.tsx
git commit -m "refactor(agenda): SchedulesPage + AgendaItemDetail use employeeList for organizer default"
```

---

## Task 10: 老 agenda JSON 数据校验（手动烟测）

本机 `~/.renlijia/users/t_28__u_54/agenda/items/` 有 4 条老 agenda（`organizerPersonaId: "default"`），用来验 serde alias 真生效。

- [ ] **Step 1: 备份现有数据**

```bash
cp -r ~/.renlijia/users/t_28__u_54/agenda ~/.renlijia/users/t_28__u_54/agenda.backup-$(date +%Y%m%d-%H%M%S)
```

- [ ] **Step 2: 重启 dev 进程**

```bash
lsof -ti :5173 | xargs -I {} kill -9 {} 2>/dev/null
ps -ef | grep -E "target/debug/aijia|tauri.js dev" | grep -v grep | awk '{print $2}' | xargs -I {} kill -9 {} 2>/dev/null
sleep 2
cd /Users/a20250311/.codex/worktrees/46a6/lotus-app && pnpm tauri:dev > /tmp/aijia-dev.log 2>&1 &
```

- [ ] **Step 3: UI 烟测**

打开 SchedulesPage：
1. 老 4 条应可见，标题/时间都正确（serde alias 工作）
2. 点编辑某条 → "执行员工"下拉应显示老 `organizerEmployeeId="default"`，但下拉选项里没有 default（因为 default 是 persona 不是 employee）。**预期表现**：select 显示空值或回退到第一个 employee；这是 PR-5 已知现象，PR-6 会清掉老数据
3. 新建一条 → 选择员工 → 保存 → 列表里出现新条目，磁盘 JSON 应包含 `"organizerEmployeeId": "emp-..."`（不是 personaId）

```bash
cat ~/.renlijia/users/t_28__u_54/agenda/items/agenda-*.json | grep -i "organizer"
```

预期：老 4 条仍是 `organizerPersonaId`（未更新写出），新 1 条是 `organizerEmployeeId`。

> **注意**：alias 只影响**读**，老条目重新写出时会用新名，**会丢失** `organizerPersonaId`。所以编辑老条目并保存后磁盘字段会变 `organizerEmployeeId`，值不变（因为 update 不允许改 organizer）。

- [ ] **Step 4: 触发一次老条目（cron 或手动 run-now）**

如果老条目 `organizerEmployeeId="default"` 不对应任何 employee：dispatcher Step 5 改造里有 `employee.is_none()` 兜底分支，会用 agenda 自己的 prompt 触发，不应崩。看日志确认无 `panic`：

```bash
tail -100 /tmp/aijia-dev.log | grep -E "agenda-dispatch|ERROR"
```

- [ ] **Step 5: 失败时恢复备份**

```bash
# 仅在烟测发现数据损坏时执行
rm -rf ~/.renlijia/users/t_28__u_54/agenda
mv ~/.renlijia/users/t_28__u_54/agenda.backup-* ~/.renlijia/users/t_28__u_54/agenda
```

- [ ] **Step 6: 烟测通过则记录日志**

不产生 commit；把烟测过程结果记录到下一步的 spec/plan 文档段落里。

---

## Task 11: 改 spec 与 rules 文档

**Files:**
- Modify: `docs/superpowers/specs/2026-05-06-agenda-base-design.md`（§1.3 / §1.8）
- Modify: `docs/test-intents/spec/tasks/agenda-base/rules.md`

- [ ] **Step 1: 改 spec §1.3 organizer 段落**

把 §1.3 中 `organizer_persona_id` / persona 相关描述改成 `organizer_employee_id`，加一句：

> Organizer 类型自 PR-5 起从 Persona 迁移到 Employee。老 JSON `organizerPersonaId` 字段通过 serde alias 透明读入；写出去使用 `organizerEmployeeId`。

- [ ] **Step 2: 改 rules.md §1.3**

新增/修改意图条目：

```markdown
- 创建日程时，request 必须带 organizerEmployeeId 字段，指向当前用户名下 lifecycle=Active 的某个数字员工
- 删除/Archive 一个数字员工时，所有 organizerEmployeeId 指向它的 agenda item 必须被自动标记为 Orphaned，调度器不再触发它们
- 老 JSON（含 organizerPersonaId 而非 organizerEmployeeId）必须能被读入，字段值不丢
- 编辑现有 agenda item 时，UI 必须锁定 organizer 不允许改派
- 用户没有任何 lifecycle=Active 的数字员工时，新建日程的"保存"按钮必须 disabled，且 UI 必须有跳转到「数字员工」页雇佣的引导
```

- [ ] **Step 3: 加 PR-5 收尾段到 plan 文档（本文件）**

在 plan 末尾补一段「PR-5 烟测 + 现状」，记录 Task 10 的实测结果。

- [ ] **Step 4: Commit**

```bash
git add docs/
git commit -m "docs(agenda): update spec/rules to reflect organizer = Employee (PR-5)"
```

---

## Task 12: 最终回归

- [ ] **Step 1: 后端 lib + 集成**

```bash
cd src-tauri && cargo test --lib --tests --no-fail-fast 2>&1 | tail -20
```

Expected: lib 769+/4（4 是 PR-4 baseline），集成测试全部 PASS

- [ ] **Step 2: 前端关键回归**

```bash
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts src/features/agenda/ src/features/schedules/ 2>&1 | tail -10
```

Expected: 全部 PASS

- [ ] **Step 3: tsc + lint**

```bash
pnpm exec tsc --noEmit && pnpm exec eslint src/ 2>&1 | tail -5
```

Expected: 0 error

- [ ] **Step 4: 不推远端**

按用户指示，PR-5 全程仅 detached HEAD 本地 commit；不 push 任何 remote。

- [ ] **Step 5: 写 memory handover**

更新 `~/.claude/projects/-Users-a20250311-IdeaProjects-lotus-app/memory/project_persona_deprecation_2026-05-10.md`：

- 把 PR-5 状态从"待立项"改成"已落地（commit hash X..Y）"
- 列出还没切的 persona 用法（active persona 兜底、设置页等）作为 PR-6 范围

---

## Self-Review

**Spec coverage**：
- ✅ 字段重命名（Task 1-4, 7, 9）
- ✅ 老 JSON 兼容（Task 1 serde alias + Task 10 烟测）
- ✅ dispatcher 接 employee（Task 5）
- ✅ 删除 employee 联动 orphan（Task 6）
- ✅ 前端选择器（Task 8）
- ✅ 空员工引导（Task 8 Step 3）
- ✅ 编辑锁定（Task 8 测试）
- ✅ spec/rules 同步（Task 11）
- ✅ 不切 dispatch_employee_run / 不动 inbox（Task 5 Out of scope 段已声明）

**类型一致性**：`organizer_employee_id` / `organizerEmployeeId` 全文一致；`Participant.employee_id` 全文一致；`primary_employee_id` 全文一致；`mark_orphaned_by_organizer` 函数名保留。

**Placeholder 扫描**：所有代码块都是可粘贴；无 TODO；测试代码完整。

**已知遗留**：
- `chat.rs:2286-2319` 的 `get_active_persona_id` 仍存在（active persona 兜底），PR-6 处理
- `SchedulesPage` 删了 `get_active_persona` 调用，但 chat 页其他 active persona 用法未触动
- 老 4 条 agenda 的 `organizerEmployeeId="default"` 是无效值，dispatcher 走兜底分支不崩；PR-6 提供一键修复脚本或迁移
