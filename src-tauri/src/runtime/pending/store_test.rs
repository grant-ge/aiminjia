use tempfile::TempDir;

use super::store::*;
use super::types::*;

fn sample_item(id: &str) -> PendingItem {
    PendingItem {
        id: id.into(),
        source: PendingSource::App,
        text: "hi".into(),
        sender_nick: None,
        attachments: vec![],
        skill_command: None,
        received_at: "2026-05-11T03:21:00Z".into(),
        origin: Default::default(),
        output_binding: Default::default(),
    }
}

#[test]
fn read_pending_returns_empty_when_file_missing() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    let items = read_pending(&path).unwrap();
    assert!(items.is_empty());
}

#[test]
fn write_then_read_pending() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    let items = vec![sample_item("a"), sample_item("b")];
    write_pending(&path, &items).unwrap();
    let back = read_pending(&path).unwrap();
    assert_eq!(back, items);
}

#[test]
fn write_empty_creates_v1_empty_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    write_pending(&path, &[]).unwrap();
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("\"schemaVersion\": 1"));
    assert!(content.contains("\"items\": []"));
}

#[test]
fn read_pending_corrupt_file_returns_empty_and_logs() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    std::fs::write(&path, "{this is not json").unwrap();
    let items = read_pending(&path).unwrap();
    assert!(items.is_empty());
}

#[test]
fn read_pending_with_unknown_schema_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    std::fs::write(&path, r#"{"schemaVersion":99,"items":[]}"#).unwrap();
    let items = read_pending(&path).unwrap();
    assert!(items.is_empty());
}

#[test]
fn read_pending_v1_item_without_source_defaults_to_app() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pending.json");
    std::fs::write(
        &path,
        r#"{
            "schemaVersion": 1,
            "items": [{
                "id": "legacy-1",
                "text": "legacy pending text",
                "receivedAt": "2026-05-11T03:21:00Z"
            }]
        }"#,
    )
    .unwrap();

    let items = read_pending(&path).unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "legacy-1");
    assert_eq!(items[0].source, PendingSource::App);
    assert_eq!(items[0].text, "legacy pending text");
}

#[test]
fn scan_pending_files_under_dir() {
    let tmp = TempDir::new().unwrap();
    let conv_a = tmp.path().join("conv-a");
    let conv_b = tmp.path().join("conv-b");
    std::fs::create_dir_all(&conv_a).unwrap();
    std::fs::create_dir_all(&conv_b).unwrap();
    write_pending(&conv_a.join("pending.json"), &[sample_item("a1")]).unwrap();
    write_pending(&conv_b.join("pending.json"), &[]).unwrap();
    let found = scan_conversation_pending(tmp.path()).unwrap();
    let conv_a_id = "conv-a".to_string();
    let entry = found.iter().find(|(id, _)| *id == conv_a_id);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().1.len(), 1);
}
