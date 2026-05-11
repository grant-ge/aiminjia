//! 集成测试：router migration + build_conversation_snapshot 协作。
//!
//! 不直接测 ChannelManager（依赖 AppHandle，集成测试里不便构造），
//! 而是测 ChannelManager 内部纯函数 + ChannelSessionRouter 这两个组件
//! 的组合行为，覆盖 spec §测试 中的 hydrate / 迁移 / refresh 场景。

use std::sync::Arc;

use app_lib::connector::channel::manager::build_conversation_snapshot;
use app_lib::connector::channel::router::ChannelSessionRouter;
use app_lib::connector::channel::types::ConversationType;
use app_lib::runtime::store::{ConversationStore, InMemoryConversationStore};
use tempfile::TempDir;

#[test]
fn legacy_v1_sessions_trigger_full_wipe_on_startup() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("channels/dingtalk/sessions.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    // 模拟 v1：无 schemaVersion，key 没有 robot_code 段
    let v1 = serde_json::json!({
        "sessions": {
            "group:cid-A": "sess-legacy-1",
            "private:user-B": "sess-legacy-2",
        }
    });
    std::fs::write(&path, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

    let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
    conv_store.create_conversation("sess-legacy-1", "old1").unwrap();
    conv_store.create_conversation("sess-legacy-2", "old2").unwrap();

    let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();

    assert!(router.entries().is_empty(), "router should be empty after wipe");
    assert!(
        conv_store.list_conversation_ids().unwrap().is_empty(),
        "conversation store should be empty after wipe"
    );

    let raw = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["schemaVersion"], 2);
    assert!(parsed["sessions"].as_object().unwrap().is_empty());
}

#[test]
fn hydrate_populates_conversations_from_router() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("channels/dingtalk/sessions.json");
    let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());

    // 写入 v2 数据
    {
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();
        let _ = router
            .get_or_create_session(&ConversationType::Private, "robot-current", "user-1", || {
                conv_store.create_conversation("sess-1", "姚斌权").unwrap();
                Ok("sess-1".to_string())
            })
            .unwrap();
        let _ = router
            .get_or_create_session(&ConversationType::Group, "robot-current", "cid-2", || {
                conv_store
                    .create_conversation("sess-2", "钉钉群 cid-2")
                    .unwrap();
                Ok("sess-2".to_string())
            })
            .unwrap();
    }

    let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();
    let snapshot = build_conversation_snapshot(
        &router.entries(),
        conv_store.as_ref(),
        Some("robot-current"),
    );

    assert_eq!(snapshot.len(), 2);
    assert!(snapshot.iter().all(|c| c.is_active_robot));
    let names: std::collections::HashSet<_> =
        snapshot.iter().map(|c| c.display_name.clone()).collect();
    assert!(names.contains("姚斌权"));
    assert!(names.contains("钉钉群 cid-2"));
}

#[test]
fn hydrate_marks_only_current_robot_as_active() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("channels/dingtalk/sessions.json");
    let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());

    {
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();
        router
            .get_or_create_session(&ConversationType::Private, "robot-A", "user-1", || {
                conv_store.create_conversation("sess-a", "user-A").unwrap();
                Ok("sess-a".to_string())
            })
            .unwrap();
        router
            .get_or_create_session(&ConversationType::Private, "robot-B", "user-2", || {
                conv_store.create_conversation("sess-b", "user-B").unwrap();
                Ok("sess-b".to_string())
            })
            .unwrap();
    }

    let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();
    let snapshot =
        build_conversation_snapshot(&router.entries(), conv_store.as_ref(), Some("robot-A"));

    let active: Vec<_> = snapshot.iter().filter(|c| c.is_active_robot).collect();
    let inactive: Vec<_> = snapshot.iter().filter(|c| !c.is_active_robot).collect();

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].robot_code, "robot-A");
    assert_eq!(inactive.len(), 1);
    assert_eq!(inactive[0].robot_code, "robot-B");
}

#[test]
fn hydrate_with_no_current_robot_marks_all_inactive() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("channels/dingtalk/sessions.json");
    let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());

    {
        let mut router = ChannelSessionRouter::load_for_test(&path).unwrap();
        router
            .get_or_create_session(&ConversationType::Private, "robot-old", "user-1", || {
                conv_store.create_conversation("sess-1", "old-user").unwrap();
                Ok("sess-1".to_string())
            })
            .unwrap();
    }

    let router = ChannelSessionRouter::migrate_or_load(&path, conv_store.as_ref()).unwrap();
    let snapshot = build_conversation_snapshot(&router.entries(), conv_store.as_ref(), None);

    assert_eq!(snapshot.len(), 1);
    assert!(!snapshot[0].is_active_robot);
}
