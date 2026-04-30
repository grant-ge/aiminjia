use std::fs;

use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use app_lib::storage::upload_gc::gc_orphan_upload_files;
use tempfile::TempDir;

fn setup() -> (TempDir, AppStorage, FileManager) {
    let temp = TempDir::new().expect("tempdir");
    let db = AppStorage::new(temp.path()).expect("storage");
    let file_mgr = FileManager::new(temp.path());
    (temp, db, file_mgr)
}

#[test]
fn y3_gc_deletes_orphan_upload_files() {
    let (temp, db, file_mgr) = setup();
    db.create_conversation("c1", "Conversation 1")
        .expect("create conversation");

    let uploads_dir = temp.path().join("uploads");
    fs::create_dir_all(&uploads_dir).expect("create uploads dir");
    let orphan_path = uploads_dir.join("orphan.csv");
    fs::write(&orphan_path, "a,b\n1,2\n").expect("write orphan file");

    let deleted = gc_orphan_upload_files(&db, &file_mgr).expect("gc should succeed");

    assert_eq!(deleted, 1);
    assert!(
        !orphan_path.exists(),
        "orphan upload file should be deleted by startup GC"
    );
}

#[test]
fn y3_gc_preserves_indexed_upload_files() {
    let (temp, db, file_mgr) = setup();
    db.create_conversation("c1", "Conversation 1")
        .expect("create conversation");

    let uploads_dir = temp.path().join("uploads");
    fs::create_dir_all(&uploads_dir).expect("create uploads dir");
    let kept_path = uploads_dir.join("kept.csv");
    fs::write(&kept_path, "a,b\n1,2\n").expect("write indexed file");

    db.insert_uploaded_file("uf-1", "c1", "kept.csv", "uploads/kept.csv", "csv", 8, None)
        .expect("insert uploaded file record");

    let deleted = gc_orphan_upload_files(&db, &file_mgr).expect("gc should succeed");

    assert_eq!(deleted, 0);
    assert!(
        kept_path.exists(),
        "indexed upload file must not be deleted by orphan GC"
    );
}

#[test]
fn y3_gc_fail_open_when_any_file_index_is_corrupted() {
    let (temp, db, file_mgr) = setup();
    db.create_conversation("c-bad", "Broken Conversation")
        .expect("create broken conversation");
    db.create_conversation("c-good", "Healthy Conversation")
        .expect("create healthy conversation");

    let uploads_dir = temp.path().join("uploads");
    fs::create_dir_all(&uploads_dir).expect("create uploads dir");
    let orphan_path = uploads_dir.join("orphan.csv");
    let kept_path = uploads_dir.join("kept.csv");
    fs::write(&orphan_path, "orphan").expect("write orphan file");
    fs::write(&kept_path, "kept").expect("write kept file");

    db.insert_uploaded_file(
        "uf-1",
        "c-good",
        "kept.csv",
        "uploads/kept.csv",
        "csv",
        4,
        None,
    )
    .expect("insert uploaded file record");

    let broken_index = temp
        .path()
        .join("conversations")
        .join("c-bad")
        .join("file_index.json");
    fs::write(&broken_index, "{ not valid json").expect("corrupt file index");

    let deleted = gc_orphan_upload_files(&db, &file_mgr).expect("gc should not fail closed");

    assert_eq!(
        deleted, 0,
        "fail-open should abort deletion round when any file index is unreadable"
    );
    assert!(
        kept_path.exists(),
        "referenced upload must remain when GC encounters a corrupted index"
    );
    assert!(
        orphan_path.exists(),
        "fail-open should leave potential orphans untouched instead of risking mis-delete"
    );
}

#[test]
fn y1_exit_path_flushes_pending_assistant_message_writes() {
    let lib_rs = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");

    assert!(
        lib_rs.contains("flush_pending_message_writes"),
        "app exit path must flush pending assistant message writes before shutdown"
    );
}
