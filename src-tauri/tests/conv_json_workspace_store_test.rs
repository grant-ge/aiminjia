//! 集成测试：ConvJsonAuthorizedWorkspaceStore trait 行为 + Persisted ↔ AuthorizedWorkspace 映射。

use std::path::PathBuf;
use std::sync::Arc;

use app_lib::runtime::ids::SessionId;
use app_lib::runtime::store::{
    AuthorizedWorkspace, AuthorizedWorkspaceStore, ConvJsonAuthorizedWorkspaceStore,
};
use app_lib::storage::file_store::conversations::create_conversation;
use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

fn fresh_storage(_name: &str) -> (TempDir, Arc<AppStorage>) {
    let dir = TempDir::new().expect("tempdir");
    let storage = Arc::new(AppStorage::new(dir.path()).expect("AppStorage::new"));
    (dir, storage)
}

fn make_ws(conv_id: &str, root: &str, name: &str) -> AuthorizedWorkspace {
    AuthorizedWorkspace {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: SessionId::new(conv_id.to_string()),
        root_path: PathBuf::from(root),
        display_name: name.to_string(),
        authorized_at: "2026-05-20T00:00:00+00:00".to_string(),
    }
}

#[test]
fn replace_and_get_round_trip() {
    let (_dir, storage) = fresh_storage("replace-get");
    let conv_id = "c-1";
    create_conversation(storage.base_dir(), conv_id, "test").unwrap();

    let store = ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    };
    let sid = SessionId::new(conv_id.to_string());
    let ws = make_ws(conv_id, "/tmp/foo", "foo");

    store.replace_for_session(conv_id, &ws).unwrap();
    let got = store.get_current_for_session(conv_id, &sid).unwrap();
    assert!(got.is_some(), "expected Some workspace");
    let got = got.unwrap();
    assert_eq!(got.display_name, "foo");
    assert_eq!(got.root_path, PathBuf::from("/tmp/foo"));
    // session_id is filled back from the passed-in sid
    assert_eq!(got.session_id, sid);
}

#[test]
fn clear_for_session_removes_workspace() {
    let (_dir, storage) = fresh_storage("clear");
    let conv_id = "c-2";
    create_conversation(storage.base_dir(), conv_id, "test").unwrap();

    let store = ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    };
    let sid = SessionId::new(conv_id.to_string());
    let ws = make_ws(conv_id, "/tmp/bar", "bar");

    store.replace_for_session(conv_id, &ws).unwrap();
    store.clear_for_session(conv_id, &sid).unwrap();
    let got = store.get_current_for_session(conv_id, &sid).unwrap();
    assert!(got.is_none());
}

#[test]
fn replace_overwrites_previous() {
    let (_dir, storage) = fresh_storage("overwrite");
    let conv_id = "c-3";
    create_conversation(storage.base_dir(), conv_id, "test").unwrap();

    let store = ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    };
    let sid = SessionId::new(conv_id.to_string());

    store
        .replace_for_session(conv_id, &make_ws(conv_id, "/tmp/old", "old"))
        .unwrap();
    store
        .replace_for_session(conv_id, &make_ws(conv_id, "/tmp/new", "new"))
        .unwrap();

    let got = store
        .get_current_for_session(conv_id, &sid)
        .unwrap()
        .unwrap();
    assert_eq!(got.display_name, "new");
    assert_eq!(got.root_path, PathBuf::from("/tmp/new"));
}

#[test]
fn workspace_mirrors_to_index_json() {
    let (_dir, storage) = fresh_storage("index-mirror");
    let conv_id = "c-4";
    create_conversation(storage.base_dir(), conv_id, "test").unwrap();

    let store = ConvJsonAuthorizedWorkspaceStore {
        storage: storage.clone(),
    };
    store
        .replace_for_session(conv_id, &make_ws(conv_id, "/tmp/proj", "proj"))
        .unwrap();

    // index.json lives at base_dir/index.json (NOT inside conversations/)
    let index_path = storage.base_dir().join("index.json");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    let entry = index["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == conv_id)
        .unwrap();
    assert_eq!(entry["workspaceName"], "proj");
}
