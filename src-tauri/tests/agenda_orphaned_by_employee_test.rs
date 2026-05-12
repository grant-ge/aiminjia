//! 验证：删除（archive 或 purge）一个 employee 时，所有 organizer_employee_id
//! 指向它的 agenda item 必须能被 mark_orphaned_by_organizer 标记为 Orphaned。
//!
//! 这是 store 级测试（无 Tauri 依赖），契合 PR-5 Task 6 的钩子设计：
//! commands/employees.rs 中的 employee_delete / employee_purge 入口仅负责调用此 store 方法。

use anyhow::Result;
use chrono::Utc;
use tempfile::TempDir;

use app_lib::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaStore, ItemStatus, Participant,
};

#[test]
fn purging_employee_marks_dependent_agenda_orphaned() -> Result<()> {
    let tmp = TempDir::new()?;
    let store = AgendaStore::new(tmp.path());

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
    store.create(item)?;

    let count = store.mark_orphaned_by_organizer("emp-1")?;
    assert_eq!(count, 1, "exactly the matching item must be orphaned");

    let item_after = store.get(&AgendaItemId("agenda-test".into()))?;
    assert_eq!(item_after.status, ItemStatus::Orphaned);

    Ok(())
}
