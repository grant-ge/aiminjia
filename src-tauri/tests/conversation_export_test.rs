use app_lib::runtime::export::conversation_exporter::{
    ConversationExportRequest, ConversationExporter, ExportPaths,
};
use app_lib::storage::file_store::types::StoredMessage;
use app_lib::storage::file_store::AppStorage;
use app_lib::storage::{AiJiaHome, CurrentUserStorage, UserScope};
use app_lib::transport::tauri_commands::conversation_export::{
    active_export_storage, conversation_export_root, validate_export_zip_path,
};
use chrono::{Duration, Utc};
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
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
    assert!(result.file_name.starts_with("aijia-export-"));
    assert!(!result.file_name.contains("Rust"));
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
fn export_conversation_html_renders_markdown_with_raw_toggle_and_escapes_html() {
    let dir = TempDir::new().unwrap();
    let app_home = dir.path().join("home");
    let export_root = dir.path().join("exports");
    let storage = AppStorage::new(&app_home).unwrap();

    storage
        .create_conversation("conv-markdown", "Markdown export")
        .unwrap();
    insert_message(
        &storage,
        "m-markdown",
        "conv-markdown",
        "assistant",
        "# Markdown 标题\n\n**重点**\n\n- 第一项\n\n`inline`\n\n[链接](https://example.com)\n\n<script>alert(1)</script>",
    );

    let exporter = ConversationExporter::new(ExportPaths {
        app_home,
        export_root,
    });
    let result = exporter
        .export(
            &storage,
            ConversationExportRequest {
                conversation_id: "conv-markdown".to_string(),
                app_version: "0.5.test".to_string(),
                platform: "test-os".to_string(),
                arch: "test-arch".to_string(),
            },
        )
        .unwrap();

    let html = read_zip_entry(&result.zip_path, "conversation.html");
    assert!(html.contains("data-view-toggle"));
    assert!(html.contains("data-view-panel=\"markdown\""));
    assert!(html.contains("data-view-panel=\"raw\""));
    assert!(html.contains("<h1>Markdown 标题</h1>"));
    assert!(html.contains("<strong>重点</strong>"));
    assert!(html.contains("<li>第一项</li>"));
    assert!(html.contains("<code>inline</code>"));
    assert!(html.contains(
        "<a href=\"https://example.com\" target=\"_blank\" rel=\"noreferrer noopener\">链接</a>"
    ));
    assert!(html.contains("# Markdown 标题"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<script>alert(1)</script>"));
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

#[test]
fn export_result_serializes_for_tauri_with_camel_case_path_fields() {
    let value = serde_json::to_value(
        app_lib::runtime::export::conversation_exporter::ConversationExportResult {
            zip_path: std::path::PathBuf::from("/tmp/export.zip"),
            file_name: "export.zip".to_string(),
            size_bytes: 42,
        },
    )
    .unwrap();

    assert_eq!(value["zipPath"], "/tmp/export.zip");
    assert_eq!(value["fileName"], "export.zip");
    assert_eq!(value["sizeBytes"], 42);
}

#[test]
fn command_helpers_use_current_user_storage_but_global_export_root() {
    let dir = TempDir::new().unwrap();
    let home = Arc::new(AiJiaHome::from_path(dir.path().join("home")));
    home.ensure_dirs().unwrap();
    home.ensure_global_dirs().unwrap();

    let root_storage = Arc::new(AppStorage::new(home.root()).unwrap());
    let current_user_storage = Arc::new(CurrentUserStorage::new(home.clone()));
    let scope = UserScope::new(10, 20);
    current_user_storage.activate_scope(scope.clone()).unwrap();

    let selected_storage = active_export_storage(&current_user_storage, &root_storage);
    assert_eq!(selected_storage.base_dir(), home.user_dir(&scope));
    assert_eq!(
        conversation_export_root(&home),
        home.root().join("exports").join("conversations")
    );
}

#[test]
fn command_helpers_fall_back_to_root_storage_when_no_user_is_active() {
    let dir = TempDir::new().unwrap();
    let home = Arc::new(AiJiaHome::from_path(dir.path().join("home")));
    home.ensure_dirs().unwrap();
    home.ensure_global_dirs().unwrap();

    let root_storage = Arc::new(AppStorage::new(home.root()).unwrap());
    let current_user_storage = Arc::new(CurrentUserStorage::new(home));

    let selected_storage = active_export_storage(&current_user_storage, &root_storage);
    assert_eq!(selected_storage.base_dir(), root_storage.base_dir());
}

#[test]
fn reveal_path_validation_only_accepts_zip_inside_global_export_root() {
    let dir = TempDir::new().unwrap();
    let home = AiJiaHome::from_path(dir.path().join("home"));
    let export_root = conversation_export_root(&home);
    std::fs::create_dir_all(&export_root).unwrap();

    let allowed_zip = export_root.join("conversation.zip");
    write_file(&allowed_zip, "zip body");
    let outside_zip = dir.path().join("outside.zip");
    write_file(&outside_zip, "zip body");
    let text_file = export_root.join("conversation.txt");
    write_file(&text_file, "text body");

    assert_eq!(
        validate_export_zip_path(&home, &allowed_zip).unwrap(),
        allowed_zip.canonicalize().unwrap()
    );
    assert!(validate_export_zip_path(&home, &outside_zip).is_err());
    assert!(validate_export_zip_path(&home, &text_file).is_err());
    assert!(validate_export_zip_path(&home, &export_root.join("missing.zip")).is_err());
}

#[test]
fn conversation_export_commands_are_registered_and_exposed_to_typescript() {
    let command_source =
        std::fs::read_to_string("src/transport/tauri_commands/conversation_export.rs").unwrap();
    assert!(command_source.contains("pub async fn export_conversation"));
    assert!(command_source.contains("ConversationExporter::new"));
    assert!(command_source.contains("pub async fn reveal_export_in_folder"));
    assert!(command_source.contains("导出文件不存在"));

    let command_mod = std::fs::read_to_string("src/transport/tauri_commands/mod.rs").unwrap();
    assert!(command_mod.contains("pub mod conversation_export;"));

    let lib_source = std::fs::read_to_string("src/lib.rs").unwrap();
    assert!(
        lib_source.contains("transport::tauri_commands::conversation_export::export_conversation")
    );
    assert!(lib_source
        .contains("transport::tauri_commands::conversation_export::reveal_export_in_folder"));

    let tauri_ts = std::fs::read_to_string("../src/lib/tauri.ts").unwrap();
    assert!(tauri_ts.contains("export interface ExportConversationResult"));
    assert!(tauri_ts.contains("zipPath: string"));
    assert!(tauri_ts.contains("fileName: string"));
    assert!(tauri_ts.contains("sizeBytes: number"));
    assert!(tauri_ts.contains("export function exportConversation"));
    assert!(tauri_ts.contains("invoke<ExportConversationResult>('export_conversation'"));
    assert!(tauri_ts.contains("export function revealExportInFolder"));
    assert!(tauri_ts.contains("invoke<void>('reveal_export_in_folder'"));
}
