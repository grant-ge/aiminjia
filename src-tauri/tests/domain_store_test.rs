use app_lib::runtime::store::{
    AuditStore, ConversationStore, FileRecordStore, MemoryStore, PersonaRecord, PersonaStore,
    SessionStore, SettingsStore,
};
use app_lib::storage::file_store::RuntimeRepositoryFacade;

#[test]
fn file_store_exposes_domain_repositories() {
    let facade = RuntimeRepositoryFacade::for_test();
    let _: &dyn SessionStore = facade.session_store();
    let _: &dyn SettingsStore = facade.settings_store();
    let _: &dyn MemoryStore = facade.memory_store();
    let _: &dyn AuditStore = facade.audit_store();
    let _: &dyn ConversationStore = facade.conversation_store();
    let _: &dyn PersonaStore = facade.persona_store();
    let _: &dyn FileRecordStore = facade.file_record_store();
}

#[test]
fn settings_store_full_api_works_in_memory() {
    let facade = RuntimeRepositoryFacade::for_test();
    let store = facade.settings_store();

    // get on missing key returns None
    assert_eq!(store.get("foo").unwrap(), None);

    // set + get roundtrip
    store.set("theme", "dark").unwrap();
    assert_eq!(store.get("theme").unwrap(), Some("dark".to_string()));

    // get_all returns all entries
    store.set("lang", "zh").unwrap();
    let all = store.get_all().unwrap();
    assert_eq!(all.get("theme").map(|s| s.as_str()), Some("dark"));
    assert_eq!(all.get("lang").map(|s| s.as_str()), Some("zh"));

    // get_by_prefix
    store.set("apiKey:deepseek-v3", "sk-xxx").unwrap();
    store.set("apiKey:openai", "sk-yyy").unwrap();
    let api_keys = store.get_by_prefix("apiKey:").unwrap();
    assert_eq!(api_keys.len(), 2);
    assert!(api_keys.contains_key("apiKey:deepseek-v3"));

    // delete
    store.delete("theme").unwrap();
    assert_eq!(store.get("theme").unwrap(), None);
}

#[test]
fn persona_store_crud_works_in_memory() {
    let facade = RuntimeRepositoryFacade::for_test();
    let store = facade.persona_store();

    // Initially empty
    assert!(store.list_personas().unwrap().is_empty());

    // Save a persona
    let persona = PersonaRecord {
        id: "p1".to_string(),
        version: 1,
        builtin: false,
        name: "Test Persona".to_string(),
        icon: "🤖".to_string(),
        description: "A test persona".to_string(),
        name_en: "Test Persona".to_string(),
        description_en: "A test persona".to_string(),
        identity: "You are a test persona".to_string(),
        expertise: vec!["testing".to_string()],
        memory_hints: vec![],
        linked_categories: vec!["general".to_string()],
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
    };
    store.save_persona(&persona).unwrap();

    // List shows summary
    let summaries = store.list_personas().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "p1");
    assert_eq!(summaries[0].name, "Test Persona");

    // Get full record
    let retrieved = store.get_persona("p1").unwrap();
    assert_eq!(retrieved.identity, "You are a test persona");

    // Set/get active
    store.set_active_persona("p1").unwrap();
    assert_eq!(store.get_active_persona_id().unwrap(), "p1");

    // Export/import roundtrip
    let exported = store.export_persona("p1").unwrap();
    let imported_id = store.import_persona(&exported).unwrap();
    assert_ne!(imported_id, "p1"); // new UUID assigned

    // Delete
    store.delete_persona("p1").unwrap();
    assert!(store.get_persona("p1").is_err());
}

#[test]
fn file_record_store_roundtrip_in_memory() {
    let facade = RuntimeRepositoryFacade::for_test();
    let store = facade.file_record_store();

    // Upload file
    store
        .insert_uploaded_file(
            "uf1",
            "conv1",
            "data.csv",
            "uploads/data.csv",
            "csv",
            512,
            Some("100 rows"),
        )
        .unwrap();

    let record = store
        .get_uploaded_file_for_conversation("uf1", "conv1")
        .unwrap();
    assert!(record.is_some());
    assert_eq!(
        record.unwrap().get("originalName").and_then(|v| v.as_str()),
        Some("data.csv")
    );

    // Wrong conversation returns None
    let none = store
        .get_uploaded_file_for_conversation("uf1", "other")
        .unwrap();
    assert!(none.is_none());

    // Generated file
    store
        .insert_generated_file(
            "gf1",
            "conv1",
            None,
            "report.html",
            "exports/report.html",
            "html",
            2048,
            "report",
            Some("Conversation export"),
            1,
            true,
            None,
            None,
            None,
        )
        .unwrap();

    let files = store.get_generated_files_for_conversation("conv1").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].get("fileName").and_then(|v| v.as_str()),
        Some("report.html")
    );

    // Delete uploaded file
    store.delete_uploaded_file("uf1", "conv1").unwrap();
    assert!(store
        .get_uploaded_file_for_conversation("uf1", "conv1")
        .unwrap()
        .is_none());
}

#[test]
fn conversation_store_new_methods_in_memory() {
    let facade = RuntimeRepositoryFacade::for_test();
    let store = facade.conversation_store();

    store.create_conversation("c1", "My Conversation").unwrap();

    // get_conversations returns JSON value with id and title
    let convs = store.get_conversations().unwrap();
    assert_eq!(convs.len(), 1);
    assert_eq!(convs[0].get("id").and_then(|v| v.as_str()), Some("c1"));
    assert_eq!(
        convs[0].get("title").and_then(|v| v.as_str()),
        Some("My Conversation")
    );

    // get_messages returns empty vec for new conversation
    let msgs = store.get_messages("c1").unwrap();
    assert!(msgs.is_empty());
}
