use std::sync::Arc;

use app_lib::llm::gateway::LlmGateway;
use app_lib::runtime::conversation_service;
use app_lib::runtime::{RunId, RuntimeRunRegistry};
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

#[tokio::test]
async fn conversation_runtime_service_handles_crud_and_busy_cleanup_without_tauri() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(AppStorage::new(dir.path()).unwrap());
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let file_mgr = Arc::new(FileManager::new(&workspace));
    let run_registry = Arc::new(RuntimeRunRegistry::new());
    let gateway = Arc::new(LlmGateway::new_with_registry(db.clone(), run_registry));
    let session_mgr = Arc::new(app_lib::python::session::PythonSessionManager::new(
        workspace, None,
    ));

    let conversation_id = conversation_service::create_conversation(db.clone())
        .await
        .unwrap();
    assert!(!conversation_id.is_empty());
    assert_eq!(
        conversation_service::get_messages(db.clone(), conversation_id.clone())
            .await
            .unwrap(),
        Vec::<serde_json::Value>::new()
    );

    let rename = conversation_service::rename_conversation(
        db.clone(),
        conversation_id.clone(),
        "Renamed".to_string(),
    )
    .await
    .unwrap();
    assert_eq!(rename.conversation_id, conversation_id);
    assert_eq!(rename.new_title, "Renamed");

    gateway
        .set_busy_for_run(&conversation_id, RunId::new("run-delete"))
        .unwrap();
    let outcome = conversation_service::delete_conversation(
        db.clone(),
        gateway.clone(),
        file_mgr,
        session_mgr,
        conversation_id.clone(),
    )
    .await
    .unwrap();

    assert!(outcome.cancelled_active_agent);
    assert_eq!(outcome.conversation_id, conversation_id);
    assert!(!gateway.is_conversation_busy(&conversation_id));
    assert!(conversation_service::get_conversations(db)
        .await
        .unwrap()
        .iter()
        .all(|item| item.get("id").and_then(|v| v.as_str()) != Some(conversation_id.as_str())));
}

#[tokio::test]
async fn delete_conversation_clears_persisted_active_skill_state() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(AppStorage::new(dir.path()).unwrap());
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let file_mgr = Arc::new(FileManager::new(&workspace));
    let run_registry = Arc::new(RuntimeRunRegistry::new());
    let gateway = Arc::new(LlmGateway::new_with_registry(db.clone(), run_registry));
    let session_mgr = Arc::new(app_lib::python::session::PythonSessionManager::new(
        workspace, None,
    ));

    let conversation_id = conversation_service::create_conversation(db.clone())
        .await
        .unwrap();
    db.set_memory(
        &format!("note:{}:active_skill_state", conversation_id),
        r#"{"skillId":"comp-analysis","currentStep":"step1"}"#,
        Some("test"),
    )
    .unwrap();
    assert!(db
        .get_memory(&format!("note:{}:active_skill_state", conversation_id))
        .unwrap()
        .is_some());

    conversation_service::delete_conversation(
        db.clone(),
        gateway,
        file_mgr,
        session_mgr,
        conversation_id.clone(),
    )
    .await
    .unwrap();

    assert_eq!(
        db.get_memory(&format!("note:{}:active_skill_state", conversation_id))
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn delete_conversation_returns_error_when_associated_file_delete_fails() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(AppStorage::new(dir.path()).unwrap());
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let file_mgr = Arc::new(FileManager::new(&workspace));
    let run_registry = Arc::new(RuntimeRunRegistry::new());
    let gateway = Arc::new(LlmGateway::new_with_registry(db.clone(), run_registry));
    let session_mgr = Arc::new(app_lib::python::session::PythonSessionManager::new(
        workspace.clone(),
        None,
    ));

    let conversation_id = conversation_service::create_conversation(db.clone())
        .await
        .unwrap();
    std::fs::create_dir_all(workspace.join("uploads/bad-dir")).unwrap();
    db.insert_uploaded_file(
        "file-1",
        &conversation_id,
        "bad-dir",
        "uploads/bad-dir",
        "folder",
        0,
        None,
    )
    .unwrap();

    let result = conversation_service::delete_conversation(
        db.clone(),
        gateway,
        file_mgr,
        session_mgr,
        conversation_id.clone(),
    )
    .await;

    assert!(result.is_err());
    assert!(conversation_service::get_conversations(db)
        .await
        .unwrap()
        .iter()
        .any(|item| item.get("id").and_then(|v| v.as_str()) == Some(conversation_id.as_str())));
}
