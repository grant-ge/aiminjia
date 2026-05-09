/// Architecture regression: conversations must be written to the user-scoped
/// directory after login, not to the root ~/.renlijia/ directory.
///
/// Guards the fix for the bug where TauriChatCommandAdapter held a snapshot of
/// root AppStorage at startup and never switched to the user dir after login.
use std::sync::Arc;
use tempfile::TempDir;

use app_lib::storage::current_user_storage::CurrentUserStorage;
use app_lib::storage::file_store::AppStorage;
use app_lib::storage::{AiJiaHome, UserScope};

fn make_cus(tmp: &TempDir) -> Arc<CurrentUserStorage> {
    let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
    Arc::new(CurrentUserStorage::new(home))
}

#[test]
fn get_or_returns_root_when_not_logged_in() {
    let root_tmp = TempDir::new().unwrap();
    let cus_tmp = TempDir::new().unwrap();
    let root_db = Arc::new(AppStorage::new(root_tmp.path()).unwrap());
    let cus = make_cus(&cus_tmp);

    assert_eq!(
        cus.get_or(&root_db).base_dir(),
        root_db.base_dir(),
        "before login, db must resolve to root_db"
    );
}

#[test]
fn get_or_returns_user_dir_after_login() {
    let root_tmp = TempDir::new().unwrap();
    let cus_tmp = TempDir::new().unwrap();
    let root_db = Arc::new(AppStorage::new(root_tmp.path()).unwrap());
    let cus = make_cus(&cus_tmp);

    cus.activate_scope(UserScope::new(1, 2)).unwrap();

    let resolved = cus.get_or(&root_db);
    let expected = cus_tmp.path().join("users").join("t_1__u_2");
    assert_eq!(
        resolved.base_dir(),
        expected.as_path(),
        "after login, db must resolve to user-scoped dir"
    );
    assert_ne!(
        resolved.base_dir(),
        root_db.base_dir(),
        "after login, db must NOT point at root"
    );
}

#[test]
fn get_or_falls_back_to_root_after_logout() {
    let root_tmp = TempDir::new().unwrap();
    let cus_tmp = TempDir::new().unwrap();
    let root_db = Arc::new(AppStorage::new(root_tmp.path()).unwrap());
    let cus = make_cus(&cus_tmp);

    cus.activate_scope(UserScope::new(1, 2)).unwrap();
    cus.deactivate();

    assert_eq!(
        cus.get_or(&root_db).base_dir(),
        root_db.base_dir(),
        "after logout, db must fall back to root_db"
    );
}

/// Core regression: after login, create_conversation must write to user dir, not root.
#[test]
fn conversation_written_to_user_dir_not_root_after_login() {
    let root_tmp = TempDir::new().unwrap();
    let cus_tmp = TempDir::new().unwrap();
    let root_db = Arc::new(AppStorage::new(root_tmp.path()).unwrap());
    let cus = make_cus(&cus_tmp);

    cus.activate_scope(UserScope::new(1, 2)).unwrap();

    // This is exactly what TauriChatServices::db() does
    let db = cus.get_or(&root_db);
    db.create_conversation("conv-abc", "测试对话").unwrap();

    let user_conv = cus_tmp
        .path()
        .join("users")
        .join("t_1__u_2")
        .join("conversations")
        .join("conv-abc");
    let root_conv = root_tmp.path().join("conversations").join("conv-abc");

    assert!(
        user_conv.exists(),
        "conversation must be written to user dir after login"
    );
    assert!(
        !root_conv.exists(),
        "conversation must NOT be written to root dir after login"
    );
}

/// Reproduces the original bug scenario: db resolved after startup (pre-login)
/// but used post-login. Old code would have captured root_db at construction time.
/// New code calls get_or() dynamically each time, so it sees the user dir.
#[test]
fn conversation_goes_to_user_dir_even_when_cus_captured_before_login() {
    let root_tmp = TempDir::new().unwrap();
    let cus_tmp = TempDir::new().unwrap();
    let root_db = Arc::new(AppStorage::new(root_tmp.path()).unwrap());
    let cus = make_cus(&cus_tmp);

    // capture cus reference at startup (before login) — old code would snapshot db here
    let cus_at_startup = cus.clone();

    // user logs in later
    cus.activate_scope(UserScope::new(3, 7)).unwrap();

    // new code: resolve db dynamically through the same cus reference
    let db = cus_at_startup.get_or(&root_db);
    db.create_conversation("conv-xyz", "登录后创建").unwrap();

    let user_conv = cus_tmp
        .path()
        .join("users")
        .join("t_3__u_7")
        .join("conversations")
        .join("conv-xyz");
    let root_conv = root_tmp.path().join("conversations").join("conv-xyz");

    assert!(user_conv.exists(), "conversation must be in user dir");
    assert!(!root_conv.exists(), "conversation must not be in root dir");
}
