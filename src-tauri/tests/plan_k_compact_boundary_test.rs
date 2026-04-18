use app_lib::runtime::chat::compaction::{
    build_compact_boundary_record, CompactTrigger,
};
use app_lib::runtime::store::{ConversationStore, InMemoryConversationStore};
use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

#[test]
fn k1_compact_boundary_record_fields_are_correct() {
    let record = build_compact_boundary_record(
        "conv-1",
        CompactTrigger::Auto,
        42_000,
        8_200,
        15,
    );

    assert_eq!(record.conversation_id, "conv-1");
    assert_eq!(record.trigger, CompactTrigger::Auto);
    assert_eq!(record.pre_tokens, 42_000);
    assert_eq!(record.post_tokens, 8_200);
    assert_eq!(record.messages_summarized, 15);
    assert!(!record.id.is_empty(), "compact boundary record must have an id");
    assert!(
        !record.created_at.is_empty(),
        "compact boundary record must have a timestamp"
    );
}

#[test]
fn k1_inmemory_store_append_and_list_compact_boundaries() {
    let store = InMemoryConversationStore::new();
    store.create_conversation("conv-k1", "Test").unwrap();

    let record = build_compact_boundary_record(
        "conv-k1",
        CompactTrigger::Auto,
        50_000,
        9_000,
        20,
    );
    store.append_compact_boundary(record.clone()).unwrap();

    let boundaries = store.list_compact_boundaries("conv-k1").unwrap();
    assert_eq!(boundaries.len(), 1);
    assert_eq!(boundaries[0].id, record.id);
    assert_eq!(boundaries[0].trigger, CompactTrigger::Auto);
    assert_eq!(boundaries[0].pre_tokens, 50_000);
}

#[test]
fn k1_inmemory_store_multiple_boundaries_are_ordered() {
    let store = InMemoryConversationStore::new();
    store.create_conversation("conv-k1b", "Test").unwrap();

    for i in 0u64..3 {
        let record = build_compact_boundary_record(
            "conv-k1b",
            CompactTrigger::Manual,
            10_000 * (i + 1),
            2_000,
            5,
        );
        store.append_compact_boundary(record).unwrap();
    }

    let boundaries = store.list_compact_boundaries("conv-k1b").unwrap();
    assert_eq!(boundaries.len(), 3);
    assert_eq!(boundaries[0].pre_tokens, 10_000);
    assert_eq!(boundaries[1].pre_tokens, 20_000);
    assert_eq!(boundaries[2].pre_tokens, 30_000);
}

#[test]
fn k1_app_storage_persists_compact_boundaries() {
    let dir = TempDir::new().unwrap();
    let storage = AppStorage::new(dir.path()).unwrap();
    let store: &dyn ConversationStore = &storage;
    store.create_conversation("conv-file", "File Test").unwrap();

    let first = build_compact_boundary_record(
        "conv-file",
        CompactTrigger::Auto,
        61_000,
        7_500,
        22,
    );
    let second = build_compact_boundary_record(
        "conv-file",
        CompactTrigger::Manual,
        31_000,
        6_000,
        10,
    );

    store.append_compact_boundary(first.clone()).unwrap();
    store.append_compact_boundary(second.clone()).unwrap();

    let boundaries = store.list_compact_boundaries("conv-file").unwrap();
    assert_eq!(boundaries.len(), 2);
    assert_eq!(boundaries[0].id, first.id);
    assert_eq!(boundaries[1].id, second.id);

    let reopened = AppStorage::new(dir.path()).unwrap();
    let reopened_store: &dyn ConversationStore = &reopened;
    let persisted = reopened_store.list_compact_boundaries("conv-file").unwrap();
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[0].pre_tokens, 61_000);
    assert_eq!(persisted[1].trigger, CompactTrigger::Manual);
}
