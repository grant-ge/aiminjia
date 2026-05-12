use super::types::*;

#[test]
fn pending_item_roundtrip_camel_case() {
    let item = PendingItem {
        id: "pend-abc".into(),
        source: PendingSource::ImDingtalk,
        text: "hello".into(),
        sender_nick: Some("张三".into()),
        attachments: vec![PendingAttachment {
            id: "att-1".into(),
            file_path: "/tmp/foo.png".into(),
            mime: Some("image/png".into()),
            size_bytes: Some(1024),
        }],
        received_at: "2026-05-11T03:21:00Z".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("\"senderNick\":\"张三\""));
    assert!(json.contains("\"im-dingtalk\""));
    let back: PendingItem = serde_json::from_str(&json).unwrap();
    assert_eq!(back, item);
}

#[test]
fn pending_file_format_default_empty() {
    let f = PendingFileFormat::default();
    assert_eq!(f.schema_version, 0);
    assert!(f.items.is_empty());
}

#[test]
fn pending_file_format_v1_serializes() {
    let f = PendingFileFormat {
        schema_version: 1,
        items: vec![],
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"schemaVersion\":1"));
}

#[test]
fn pending_source_kebab_case() {
    let s = serde_json::to_string(&PendingSource::ImDingtalk).unwrap();
    assert_eq!(s, "\"im-dingtalk\"");
    let s2 = serde_json::to_string(&PendingSource::App).unwrap();
    assert_eq!(s2, "\"app\"");
}
