use app_lib::runtime::employee::store::{
    CreateEmployeeRequest, EmployeeStore, KnowledgeSourceStatus,
};
use serde_json::json;

fn tmp_store() -> (tempfile::TempDir, EmployeeStore) {
    let tmp = tempfile::tempdir().unwrap();
    let store = EmployeeStore::new(tmp.path().to_path_buf());
    (tmp, store)
}

#[test]
fn update_knowledge_source_status_round_trip() {
    let (_t, store) = tmp_store();
    let rec = store
        .create(CreateEmployeeRequest {
            template_id: Some("builtin:xiaoke".into()),
            avatar: "💬".into(),
            name: "小客".into(),
            role: "客服支持".into(),
            description: "".into(),
            tool_whitelist: Some(vec![]),
            cron: None,
            timezone: None,
            lifecycle: None,
            cron_enabled: None,
            system_prompt_extra: Some("".into()),
            default_skill_id: None,
            skill_ids: None,
            resource_config: Some(json!({
                "knowledgeSources": [
                    { "path": "/tmp/faq.md", "originalName": "faq.md", "size": 1024,
                      "status": "pending", "slicedCount": 0 }
                ]
            })),
        })
        .unwrap();

    store
        .update_knowledge_source_status(
            &rec.id,
            "/tmp/faq.md",
            KnowledgeSourceStatus::Indexing,
            0,
            None,
        )
        .unwrap();

    let after = store.get(&rec.id).unwrap();
    let sources = after
        .resource_config
        .get("knowledgeSources")
        .and_then(|v| v.as_array())
        .expect("knowledgeSources exists");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].get("status").unwrap().as_str(), Some("indexing"));

    store
        .update_knowledge_source_status(
            &rec.id,
            "/tmp/faq.md",
            KnowledgeSourceStatus::Done,
            42,
            None,
        )
        .unwrap();

    let after = store.get(&rec.id).unwrap();
    let s = &after.resource_config["knowledgeSources"][0];
    assert_eq!(s["status"], "done");
    assert_eq!(s["slicedCount"], 42);
}

#[test]
fn update_knowledge_source_status_records_error() {
    let (_t, store) = tmp_store();
    let rec = store
        .create(CreateEmployeeRequest {
            template_id: Some("builtin:xiaoke".into()),
            avatar: "💬".into(),
            name: "小客".into(),
            role: "客服".into(),
            description: "".into(),
            tool_whitelist: Some(vec![]),
            cron: None,
            timezone: None,
            lifecycle: None,
            cron_enabled: None,
            system_prompt_extra: Some("".into()),
            default_skill_id: None,
            skill_ids: None,
            resource_config: Some(json!({
                "knowledgeSources": [
                    { "path": "/tmp/x.md", "originalName": "x.md", "size": 10,
                      "status": "pending", "slicedCount": 0 }
                ]
            })),
        })
        .unwrap();

    store
        .update_knowledge_source_status(
            &rec.id,
            "/tmp/x.md",
            KnowledgeSourceStatus::Failed,
            0,
            Some("file unreadable".into()),
        )
        .unwrap();

    let after = store.get(&rec.id).unwrap();
    let s = &after.resource_config["knowledgeSources"][0];
    assert_eq!(s["status"], "failed");
    assert_eq!(s["error"], "file unreadable");
}
