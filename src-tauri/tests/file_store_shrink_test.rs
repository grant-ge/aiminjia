// Phase 4 Task 1 Step 4: file_store shrinkage via ConversationStore domain trait
// TDD — these tests fail until ConversationStore is wired through RuntimeRepositoryFacade.

use app_lib::runtime::store::ConversationStore;
use app_lib::storage::file_store::RuntimeRepositoryFacade;

/// RuntimeRepositoryFacade must expose a ConversationStore so runtime code
/// can manage conversation lifecycle without importing AppStorage directly.
#[test]
fn facade_exposes_conversation_store() {
    let facade = RuntimeRepositoryFacade::for_test();
    let store: &dyn ConversationStore = facade.conversation_store();
    // Round-trip: create + list
    store.create_conversation("c-facade-1", "Facade Test").unwrap();
    let ids = store.list_conversation_ids().unwrap();
    assert!(ids.contains(&"c-facade-1".to_string()));
}

/// Conversation store can be used to remove an active task (crash-recovery lock).
#[test]
fn conversation_store_task_lock_roundtrip() {
    let facade = RuntimeRepositoryFacade::for_test();
    let store: &dyn ConversationStore = facade.conversation_store();
    store.create_conversation("c-lock-1", "Lock Test").unwrap();
    store.insert_active_task("c-lock-1").unwrap();
    store.remove_active_task("c-lock-1").unwrap();
    // Should not error even if lock is already gone
    store.remove_active_task("c-lock-1").unwrap();
}
