//! End-to-end: spawn_index_all drives knowledge sources to "done" status.
//!
//! The Tauri command itself is a thin pass-through to spawn_index_all, so we
//! validate the underlying behaviour directly rather than injecting Tauri state.
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use app_lib::runtime::employee::store::{CreateEmployeeRequest, EmployeeStore};
use app_lib::storage::file_store::AppStorage;

#[tokio::test(flavor = "multi_thread")]
async fn end_to_end_index_completes() {
    let tmp = tempfile::tempdir().unwrap();
    let renlijia = tmp.path().to_path_buf();

    // Create a minimal FAQ file with two H2 sections so chunk_markdown produces 2 chunks.
    let faq = renlijia.join("faq.md");
    std::fs::write(
        &faq,
        "## 注册\n\n点击右上角注册按钮，输入手机号验证。\n\n## 充值\n\n进入控制台 → 余额 → 充值，支持微信 / 银行转账。\n",
    )
    .unwrap();

    let employees_dir = renlijia.join("employees");
    std::fs::create_dir_all(&employees_dir).unwrap();

    let store = Arc::new(EmployeeStore::new(employees_dir));
    let app_storage = Arc::new(AppStorage::new(&renlijia).expect("AppStorage::new failed"));

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
                    {
                        "path": faq.to_string_lossy(),
                        "originalName": "faq.md",
                        "size": 100,
                        "status": "pending",
                        "slicedCount": 0
                    }
                ]
            })),
        })
        .unwrap();

    app_lib::runtime::employee::knowledge::spawn_index_all(
        Arc::clone(&store),
        Arc::clone(&app_storage),
        rec.id.clone(),
        vec![(faq.clone(), "faq.md".into())],
    );

    // Poll up to 5 s for the indexer to finish.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let r = store.get(&rec.id).unwrap();
        let status = r.resource_config["knowledgeSources"][0]["status"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if status == "done" {
            let count = r.resource_config["knowledgeSources"][0]["slicedCount"]
                .as_u64()
                .unwrap_or(0);
            assert!(count >= 2, "expected at least 2 chunks, got {}", count);
            return;
        }
        if status == "failed" {
            let err = r.resource_config["knowledgeSources"][0]["error"]
                .as_str()
                .unwrap_or("(no error message)");
            panic!("indexing failed: {}", err);
        }
        if std::time::Instant::now() > deadline {
            panic!("indexing did not complete in 5s, last status = {}", status);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reindexing_same_file_same_day_still_marks_done() {
    use app_lib::runtime::employee::knowledge;
    use app_lib::runtime::employee::store::{CreateEmployeeRequest, EmployeeStore};
    use app_lib::storage::file_store::AppStorage;
    use serde_json::json;
    use std::time::Duration;

    let tmp = tempfile::tempdir().unwrap();
    let renlijia = tmp.path().to_path_buf();

    let faq = renlijia.join("faq.md");
    std::fs::write(
        &faq,
        "## 注册\n\n点击右上角注册按钮。\n\n## 充值\n\n进入控制台 → 余额 → 充值。\n",
    )
    .unwrap();

    let store = std::sync::Arc::new(EmployeeStore::new(renlijia.join("employees")));
    let app_storage =
        std::sync::Arc::new(AppStorage::new(&renlijia).expect("AppStorage::new failed"));

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
                    { "path": faq.to_string_lossy(), "originalName": "faq.md", "size": 100,
                      "status": "pending", "slicedCount": 0 }
                ]
            })),
        })
        .unwrap();

    // First index — writes 2 chunks.
    knowledge::spawn_index_all(
        std::sync::Arc::clone(&store),
        std::sync::Arc::clone(&app_storage),
        rec.id.clone(),
        vec![(faq.clone(), "faq.md".into())],
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let r = store.get(&rec.id).unwrap();
        if r.resource_config["knowledgeSources"][0]["status"].as_str() == Some("done") {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("first index didn't complete");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Reset to pending and re-index — should still mark Done (duplicate is benign).
    store
        .update_knowledge_source_status(
            &rec.id,
            &faq.to_string_lossy(),
            app_lib::runtime::employee::store::KnowledgeSourceStatus::Pending,
            0,
            None,
        )
        .unwrap();

    knowledge::spawn_index_all(
        std::sync::Arc::clone(&store),
        std::sync::Arc::clone(&app_storage),
        rec.id.clone(),
        vec![(faq.clone(), "faq.md".into())],
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let r = store.get(&rec.id).unwrap();
        let status = r.resource_config["knowledgeSources"][0]["status"]
            .as_str()
            .unwrap_or("");
        if status == "done" {
            assert_eq!(
                r.resource_config["knowledgeSources"][0]["slicedCount"].as_u64(),
                Some(2),
                "second index should still report sliced count, not 0",
            );
            return;
        }
        if status == "failed" {
            panic!(
                "second index failed: {:?}",
                r.resource_config["knowledgeSources"][0]
            );
        }
        if std::time::Instant::now() > deadline {
            panic!("second index timeout, last status = {}", status);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
