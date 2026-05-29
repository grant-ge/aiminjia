//! 集成测试：set_conversation_source 双写 conv.json + index.json 一致性。

use std::path::PathBuf;

use app_lib::storage::file_store::conversations::{
    create_conversation, read_conversation_workspace, set_conversation_source,
    set_conversation_workspace,
};
use app_lib::storage::file_store::types::{ConversationSource, PersistedAuthorizedWorkspace};
use tempfile::TempDir;

fn fresh_base(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let base = dir.path().to_path_buf();
    // Ensure conversations index file exists for the helpers to find/update.
    std::fs::create_dir_all(base.join("conversations")).unwrap();
    let _ = name; // name unused, kept for readability
    (dir, base)
}

#[test]
fn set_to_expert_team_updates_both_conv_and_index() {
    let (_dir, base) = fresh_base("expert-team");

    let conv_id = "c-test-1";
    create_conversation(&base, conv_id, "test title").unwrap();

    set_conversation_source(
        &base,
        conv_id,
        ConversationSource::ExpertTeam {
            expert_team_id: "marketing".to_string(),
        },
        Some("市场专家团".to_string()),
    )
    .unwrap();

    // Verify conv.json
    let conv_path = base.join("conversations").join(conv_id).join("conv.json");
    let conv_content = std::fs::read_to_string(&conv_path).unwrap();
    let conv: serde_json::Value = serde_json::from_str(&conv_content).unwrap();
    assert_eq!(conv["source"]["kind"], "expertTeam");
    assert_eq!(conv["source"]["expertTeamId"], "marketing");
    assert_eq!(conv["sourceLabel"], "市场专家团");

    // Verify index.json mirror (index.json lives at base_dir root, not in conversations/)
    let index_path = base.join("index.json");
    let index_content = std::fs::read_to_string(&index_path).unwrap();
    let index: serde_json::Value = serde_json::from_str(&index_content).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .expect("entry missing");
    assert_eq!(entry["kind"], "expertTeam");
    assert_eq!(entry["sourceLabel"], "市场专家团");
}

#[test]
fn set_to_user_clears_label() {
    let (_dir, base) = fresh_base("user");
    let conv_id = "c-test-2";
    create_conversation(&base, conv_id, "test").unwrap();

    // First set to expert team
    set_conversation_source(
        &base,
        conv_id,
        ConversationSource::ExpertTeam {
            expert_team_id: "x".to_string(),
        },
        Some("X 团".to_string()),
    )
    .unwrap();
    // Then clear
    set_conversation_source(&base, conv_id, ConversationSource::User, None).unwrap();

    // Verify source is User and label is cleared
    let conv_path = base.join("conversations").join(conv_id).join("conv.json");
    let conv: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&conv_path).unwrap()).unwrap();
    assert_eq!(conv["source"]["kind"], "user");
    // sourceLabel field is skip_serializing_if Option::is_none — should be missing from JSON
    assert!(
        conv.get("sourceLabel").is_none() || conv["sourceLabel"].is_null(),
        "sourceLabel should be absent or null when cleared, got: {:?}",
        conv.get("sourceLabel")
    );

    let index_path = base.join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["kind"], "user");
    assert!(
        entry.get("sourceLabel").is_none() || entry["sourceLabel"].is_null(),
        "index.json sourceLabel should be absent or null when cleared"
    );
}

#[test]
fn set_to_employee_with_label() {
    let (_dir, base) = fresh_base("employee");
    let conv_id = "c-test-3";
    create_conversation(&base, conv_id, "test").unwrap();

    set_conversation_source(
        &base,
        conv_id,
        ConversationSource::Employee {
            employee_id: "emp-001".to_string(),
        },
        Some("小销".to_string()),
    )
    .unwrap();

    let conv_path = base.join("conversations").join(conv_id).join("conv.json");
    let conv: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&conv_path).unwrap()).unwrap();
    assert_eq!(conv["source"]["kind"], "employee");
    assert_eq!(conv["source"]["employeeId"], "emp-001");
    assert_eq!(conv["sourceLabel"], "小销");

    let index_path = base.join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["kind"], "employee");
    assert_eq!(entry["sourceLabel"], "小销");
}

#[test]
fn set_and_read_workspace() {
    let (_dir, base) = fresh_base("workspace");
    let conv_id = "c-ws-1";
    create_conversation(&base, conv_id, "test").unwrap();

    let ws = PersistedAuthorizedWorkspace {
        id: "ws-1".to_string(),
        root_path: PathBuf::from("/tmp/foo"),
        display_name: "foo".to_string(),
        authorized_at: "2026-05-20T00:00:00+00:00".to_string(),
    };

    set_conversation_workspace(&base, conv_id, Some(&ws)).unwrap();

    let read_back = read_conversation_workspace(&base, conv_id).unwrap();
    assert_eq!(read_back, Some(ws.clone()));

    // Verify index.json mirror
    let index_path = base.join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["workspaceName"], "foo");
}

#[test]
fn clear_workspace_removes_mirror() {
    let (_dir, base) = fresh_base("workspace-clear");
    let conv_id = "c-ws-2";
    create_conversation(&base, conv_id, "test").unwrap();

    let ws = PersistedAuthorizedWorkspace {
        id: "ws-1".to_string(),
        root_path: PathBuf::from("/tmp/foo"),
        display_name: "foo".to_string(),
        authorized_at: "t".to_string(),
    };
    set_conversation_workspace(&base, conv_id, Some(&ws)).unwrap();

    // Clear
    set_conversation_workspace(&base, conv_id, None).unwrap();

    assert!(read_conversation_workspace(&base, conv_id)
        .unwrap()
        .is_none());

    let index_path = base.join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert!(
        entry.get("workspaceName").is_none() || entry["workspaceName"].is_null(),
        "workspaceName should be absent or null after clear"
    );
}

#[test]
fn workspace_no_sessionid_in_disk_format() {
    let (_dir, base) = fresh_base("no-session-id");
    let conv_id = "c-ws-3";
    create_conversation(&base, conv_id, "test").unwrap();

    let ws = PersistedAuthorizedWorkspace {
        id: "ws-1".to_string(),
        root_path: PathBuf::from("/x"),
        display_name: "x".to_string(),
        authorized_at: "t".to_string(),
    };
    set_conversation_workspace(&base, conv_id, Some(&ws)).unwrap();

    // Read conv.json raw and assert sessionId is NOT present
    let conv_path = base.join("conversations").join(conv_id).join("conv.json");
    let raw = std::fs::read_to_string(&conv_path).unwrap();
    assert!(
        !raw.contains("\"sessionId\""),
        "conv.json must NOT contain sessionId in authorizedWorkspace; got: {}",
        raw
    );
}
