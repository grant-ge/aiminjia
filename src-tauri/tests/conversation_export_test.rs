use app_lib::runtime::export::conversation_exporter::{
    ConversationExportRequest, ConversationExporter, ExportPaths,
};
use app_lib::storage::file_store::types::StoredMessage;
use app_lib::storage::file_store::AppStorage;
use chrono::{Duration, Utc};
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;
use zip::ZipArchive;

fn write_file(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn read_zip_entry(zip_path: &std::path::Path, name: &str) -> String {
    let file = std::fs::File::open(zip_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name(name).unwrap();
    let mut out = String::new();
    entry.read_to_string(&mut out).unwrap();
    out
}

fn zip_names(zip_path: &std::path::Path) -> Vec<String> {
    let file = std::fs::File::open(zip_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut names = Vec::new();
    for index in 0..archive.len() {
        names.push(archive.by_index(index).unwrap().name().to_string());
    }
    names.sort();
    names
}

fn insert_message(storage: &AppStorage, id: &str, conversation_id: &str, role: &str, text: &str) {
    let msg = StoredMessage {
        seq: None,
        rev: None,
        id: id.to_string(),
        conversation_id: conversation_id.to_string(),
        role: role.to_string(),
        content: serde_json::json!({ "text": text }),
        created_at: "2026-06-03T00:00:00Z".to_string(),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: Some("run-1".to_string()),
        schema_version: Some(2),
        sequence: None,
        error: None,
    };
    storage.insert_chat_message_record(&msg).unwrap();
}

#[test]
fn export_zip_contains_readable_html_manifest_raw_messages_and_full_logs() {
    let dir = TempDir::new().unwrap();
    let app_home = dir.path().join("home");
    let export_root = dir.path().join("exports");
    let storage = AppStorage::new(&app_home).unwrap();

    storage
        .create_conversation("conv-1", "Rust <debug>")
        .unwrap();
    insert_message(
        &storage,
        "m-user",
        "conv-1",
        "user",
        "你好 <script>alert(1)</script>",
    );
    insert_message(&storage, "m-assistant", "conv-1", "assistant", "已收到");

    let logs_dir = app_home.join("logs");
    write_file(
        &logs_dir.join("metrics.jsonl"),
        r#"{"category":"diagnostics","ts":"2026-06-03T00:00:00Z","source":"backend","level":"info","event":"turn.started","conversationId":"conv-1","runId":"run-1","ok":true}"#,
    );
    write_file(
        &logs_dir.join("metrics.1.jsonl"),
        &format!(
            r#"{{"category":"diagnostics","ts":"{}","source":"backend","level":"error","event":"tool.execute.failed","conversationId":"other-conv","runId":"run-2","ok":false,"error":"boom"}}"#,
            Utc::now().to_rfc3339()
        ),
    );
    write_file(&logs_dir.join("renlijia.log"), "runtime log body\n");
    write_file(&logs_dir.join("gate.log"), "gate log body\n");

    let exporter = ConversationExporter::new(ExportPaths {
        app_home: app_home.clone(),
        export_root: export_root.clone(),
    });
    let result = exporter
        .export(
            &storage,
            ConversationExportRequest {
                conversation_id: "conv-1".to_string(),
                app_version: "0.5.test".to_string(),
                platform: "test-os".to_string(),
                arch: "test-arch".to_string(),
            },
        )
        .unwrap();

    assert!(result.zip_path.exists());
    assert_eq!(
        result.file_name,
        result.zip_path.file_name().unwrap().to_string_lossy()
    );
    assert!(result.size_bytes > 0);

    let names = zip_names(&result.zip_path);
    assert!(names.contains(&"README.txt".to_string()));
    assert!(names.contains(&"conversation.html".to_string()));
    assert!(names.contains(&"diagnostics-summary.html".to_string()));
    assert!(names.contains(&"manifest.json".to_string()));
    assert!(names.contains(&"raw/messages.jsonl".to_string()));
    assert!(names.contains(&"raw/current-conversation-diagnostics.jsonl".to_string()));
    assert!(names.contains(&"raw/recent-warn-error.jsonl".to_string()));
    assert!(names.contains(&"raw/renlijia.log".to_string()));
    assert!(names.contains(&"raw/gate.log".to_string()));

    let html = read_zip_entry(&result.zip_path, "conversation.html");
    assert!(html.contains("Rust &lt;debug&gt;"));
    assert!(html.contains("你好 &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<script>alert(1)</script>"));

    let current_diag = read_zip_entry(
        &result.zip_path,
        "raw/current-conversation-diagnostics.jsonl",
    );
    assert!(current_diag.contains("turn.started"));
    assert!(!current_diag.contains("other-conv"));

    let recent = read_zip_entry(&result.zip_path, "raw/recent-warn-error.jsonl");
    assert!(recent.contains("tool.execute.failed"));
    assert!(recent.contains("other-conv"));

    assert_eq!(
        read_zip_entry(&result.zip_path, "raw/renlijia.log"),
        "runtime log body\n"
    );
    assert_eq!(
        read_zip_entry(&result.zip_path, "raw/gate.log"),
        "gate log body\n"
    );

    let manifest: serde_json::Value =
        serde_json::from_str(&read_zip_entry(&result.zip_path, "manifest.json")).unwrap();
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["conversation"]["id"], "conv-1");
    assert_eq!(manifest["app"]["version"], "0.5.test");
    assert_eq!(manifest["logs"][0]["included"], true);
    assert!(manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["name"] == "manifest.json"));
}

#[test]
fn export_uses_unique_zip_paths_for_same_title_within_same_second() {
    let dir = TempDir::new().unwrap();
    let app_home = dir.path().join("home");
    let export_root = dir.path().join("exports");
    let storage = AppStorage::new(&app_home).unwrap();

    storage
        .create_conversation("conv-unique", "Repeated")
        .unwrap();
    insert_message(&storage, "m1", "conv-unique", "user", "hello");

    let exporter = ConversationExporter::new(ExportPaths {
        app_home,
        export_root,
    });
    let request = ConversationExportRequest {
        conversation_id: "conv-unique".to_string(),
        app_version: "0.5.test".to_string(),
        platform: "test-os".to_string(),
        arch: "test-arch".to_string(),
    };

    let first = exporter.export(&storage, request.clone()).unwrap();
    let second = exporter.export(&storage, request).unwrap();

    assert_ne!(first.zip_path, second.zip_path);
    assert!(first.zip_path.exists());
    assert!(second.zip_path.exists());
}

#[test]
fn export_skips_unreadable_metrics_entries_and_still_succeeds() {
    let dir = TempDir::new().unwrap();
    let app_home = dir.path().join("home");
    let export_root = dir.path().join("exports");
    let storage = AppStorage::new(&app_home).unwrap();

    storage
        .create_conversation("conv-metrics-best-effort", "Metrics best effort")
        .unwrap();
    insert_message(&storage, "m1", "conv-metrics-best-effort", "user", "hello");

    let logs_dir = app_home.join("logs");
    write_file(
        &logs_dir.join("metrics.jsonl"),
        &format!(
            r#"{{"category":"diagnostics","ts":"{}","source":"backend","level":"error","event":"kept.error","conversationId":"conv-metrics-best-effort","ok":false}}"#,
            Utc::now().to_rfc3339()
        ),
    );
    let unreadable = logs_dir.join("metrics.unreadable.jsonl");
    write_file(&unreadable, "not readable\n");
    #[cfg(unix)]
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();

    let exporter = ConversationExporter::new(ExportPaths {
        app_home,
        export_root,
    });
    let result = exporter
        .export(
            &storage,
            ConversationExportRequest {
                conversation_id: "conv-metrics-best-effort".to_string(),
                app_version: "0.5.test".to_string(),
                platform: "test-os".to_string(),
                arch: "test-arch".to_string(),
            },
        )
        .unwrap();

    let recent = read_zip_entry(&result.zip_path, "raw/recent-warn-error.jsonl");
    assert!(recent.contains("kept.error"));
}

#[test]
fn export_recent_warn_error_excludes_records_older_than_24h_or_without_parseable_ts() {
    let dir = TempDir::new().unwrap();
    let app_home = dir.path().join("home");
    let export_root = dir.path().join("exports");
    let storage = AppStorage::new(&app_home).unwrap();

    storage
        .create_conversation("conv-old-warn", "Old warn")
        .unwrap();
    insert_message(&storage, "m1", "conv-old-warn", "user", "hello");

    let logs_dir = app_home.join("logs");
    let old_ts = (Utc::now() - Duration::hours(48)).to_rfc3339();
    write_file(
        &logs_dir.join("metrics.jsonl"),
        &format!(
            r#"{{"category":"diagnostics","ts":"{}","source":"backend","level":"error","event":"stale.error","conversationId":"conv-old-warn","ok":false}}
{{"category":"diagnostics","source":"backend","level":"warn","event":"missing.ts","conversationId":"conv-old-warn","ok":false}}
{{"category":"diagnostics","ts":"not-a-date","source":"backend","level":"warn","event":"bad.ts","conversationId":"conv-old-warn","ok":false}}"#,
            old_ts
        ),
    );

    let exporter = ConversationExporter::new(ExportPaths {
        app_home,
        export_root,
    });
    let result = exporter
        .export(
            &storage,
            ConversationExportRequest {
                conversation_id: "conv-old-warn".to_string(),
                app_version: "0.5.test".to_string(),
                platform: "test-os".to_string(),
                arch: "test-arch".to_string(),
            },
        )
        .unwrap();

    let recent = read_zip_entry(&result.zip_path, "raw/recent-warn-error.jsonl");
    assert!(!recent.contains("stale.error"));
    assert!(!recent.contains("missing.ts"));
    assert!(!recent.contains("bad.ts"));
}

#[test]
fn export_succeeds_when_logs_are_missing_and_marks_manifest_entries() {
    let dir = TempDir::new().unwrap();
    let app_home = dir.path().join("home");
    let export_root = dir.path().join("exports");
    let storage = AppStorage::new(&app_home).unwrap();

    storage
        .create_conversation("conv-missing", "No logs")
        .unwrap();
    insert_message(&storage, "m1", "conv-missing", "user", "hello");

    let exporter = ConversationExporter::new(ExportPaths {
        app_home,
        export_root,
    });
    let result = exporter
        .export(
            &storage,
            ConversationExportRequest {
                conversation_id: "conv-missing".to_string(),
                app_version: "0.5.test".to_string(),
                platform: "test-os".to_string(),
                arch: "test-arch".to_string(),
            },
        )
        .unwrap();

    let names = zip_names(&result.zip_path);
    assert!(names.contains(&"raw/messages.jsonl".to_string()));
    assert!(!names.contains(&"raw/renlijia.log".to_string()));
    assert!(!names.contains(&"raw/gate.log".to_string()));

    let manifest: serde_json::Value =
        serde_json::from_str(&read_zip_entry(&result.zip_path, "manifest.json")).unwrap();
    let logs = manifest["logs"].as_array().unwrap();
    assert!(logs
        .iter()
        .any(|entry| entry["name"] == "raw/renlijia.log" && entry["included"] == false));
    assert!(logs
        .iter()
        .any(|entry| entry["name"] == "raw/gate.log" && entry["included"] == false));
}
