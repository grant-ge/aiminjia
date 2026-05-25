use std::fs;
use tempfile::TempDir;

#[test]
fn migrate_legacy_conversations_to_user_scope() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();

    fs::create_dir_all(root_path.join("conversations/conv1")).unwrap();
    fs::write(
        root_path.join("conversations/conv1/conv.json"),
        r#"{"id":"conv1"}"#,
    )
    .unwrap();
    fs::write(root_path.join("conversations/conv1/messages.jsonl"), "{}").unwrap();
    fs::write(
        root_path.join("index.json"),
        r#"{"conversations":[{"id":"conv1"}]}"#,
    )
    .unwrap();
    fs::create_dir_all(root_path.join("shared/memory")).unwrap();
    fs::write(root_path.join("shared/memory/memory.jsonl"), "[]").unwrap();
    fs::write(root_path.join("mcp_servers.json"), "[]").unwrap();
    fs::write(root_path.join("permissions.json"), "{}").unwrap();
    fs::create_dir_all(root_path.join("global")).unwrap();

    let scope_key = "t_1__u_2";
    let user_dir = root_path.join("users").join(scope_key);

    app_lib::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path,
        &user_dir,
        scope_key,
        &root_path.join("global/state.json"),
    )
    .unwrap();

    assert!(user_dir.join("conversations/conv1/conv.json").exists());
    assert!(user_dir.join("index.json").exists());
    assert!(user_dir.join("shared/memory/memory.jsonl").exists());
    assert!(user_dir.join("mcp_servers.json").exists());
    assert!(user_dir.join("permissions.json").exists());
    assert!(root_path.join("conversations/conv1/conv.json").exists());

    let state_text = fs::read_to_string(root_path.join("global/state.json")).unwrap();
    assert!(state_text.contains("claimedBy"));
    assert!(state_text.contains(scope_key));
}

#[test]
fn second_scope_blocked_from_auto_migration() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();

    fs::create_dir_all(root_path.join("conversations/conv1")).unwrap();
    fs::write(root_path.join("conversations/conv1/conv.json"), "data").unwrap();
    fs::write(root_path.join("index.json"), r#"{"conversations":[]}"#).unwrap();
    fs::create_dir_all(root_path.join("global")).unwrap();

    let state_path = root_path.join("global/state.json");
    let scope_a = "t_1__u_2";
    let user_dir_a = root_path.join("users").join(scope_a);
    app_lib::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path,
        &user_dir_a,
        scope_a,
        &state_path,
    )
    .unwrap();
    assert!(user_dir_a.join("conversations/conv1/conv.json").exists());

    let scope_b = "t_3__u_4";
    let user_dir_b = root_path.join("users").join(scope_b);
    app_lib::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path,
        &user_dir_b,
        scope_b,
        &state_path,
    )
    .unwrap();
    assert!(!user_dir_b.join("conversations/conv1/conv.json").exists());
}

#[test]
fn new_user_no_legacy_no_marker() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();
    fs::create_dir_all(root_path.join("global")).unwrap();

    let scope_key = "t_1__u_2";
    let user_dir = root_path.join("users").join(scope_key);
    let state_path = root_path.join("global/state.json");

    app_lib::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path,
        &user_dir,
        scope_key,
        &state_path,
    )
    .unwrap();

    assert!(!state_path.exists());
}

#[test]
fn migrate_config_splits_keys() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();
    let legacy_config = serde_json::json!({
        "cloud_auth": "encrypted_blob",
        "workspacePath": "/some/path",
        "primaryModel": "gpt-4",
        "theme": "dark",
        "autoModelRouting": "true"
    });
    fs::write(root_path.join("config.json"), legacy_config.to_string()).unwrap();
    fs::create_dir_all(root_path.join("global/auth")).unwrap();

    let user_dir = root_path.join("users/t_1__u_2");
    fs::create_dir_all(&user_dir).unwrap();

    app_lib::storage::migration_user_scope::migrate_legacy_config_if_needed(
        root_path,
        &user_dir,
        &root_path.join("global"),
    )
    .unwrap();

    assert!(root_path.join("global/auth/cloud_auth").exists());
    let user_cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(user_dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(user_cfg["workspacePath"], "/some/path");
    assert_eq!(user_cfg["primaryModel"], "gpt-4");

    let global_cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root_path.join("global/config.json")).unwrap())
            .unwrap();
    assert_eq!(global_cfg.get("cloud_auth"), None);
}

#[test]
fn bootstrap_cloud_auth_before_restore() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();
    let legacy_config = serde_json::json!({
        "cloud_auth": "encrypted_or_plaintext_blob",
        "workspacePath": "/legacy/workspace"
    });
    fs::write(root_path.join("config.json"), legacy_config.to_string()).unwrap();

    let global_dir = root_path.join("global");
    app_lib::storage::migration_user_scope::bootstrap_cloud_auth_if_needed(root_path, &global_dir)
        .unwrap();

    assert_eq!(
        fs::read_to_string(global_dir.join("auth/cloud_auth")).unwrap(),
        "encrypted_or_plaintext_blob"
    );
}

#[test]
fn bootstrap_cloud_auth_does_not_overwrite_existing_global_auth() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();
    fs::write(root_path.join("config.json"), r#"{"cloud_auth":"legacy"}"#).unwrap();
    let global_dir = root_path.join("global");
    fs::create_dir_all(global_dir.join("auth")).unwrap();
    fs::write(global_dir.join("auth/cloud_auth"), "existing").unwrap();

    app_lib::storage::migration_user_scope::bootstrap_cloud_auth_if_needed(root_path, &global_dir)
        .unwrap();

    assert_eq!(
        fs::read_to_string(global_dir.join("auth/cloud_auth")).unwrap(),
        "existing"
    );
}

/// 业务流程：skills 迁移只拷贝用户安装的（非 _ 开头目录 + _drafts），不拷贝内置 _builtins
#[test]
fn skills_migration_copies_user_skills_but_not_builtins() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();

    // Legacy: 有 conversations（触发迁移入口条件）
    fs::write(root_path.join("index.json"), "{}").unwrap();
    fs::create_dir_all(root_path.join("global")).unwrap();

    // Legacy skills: 模拟内置 + 用户安装 + 用户草稿
    fs::create_dir_all(root_path.join("skills/_builtins/some-builtin")).unwrap();
    fs::write(
        root_path.join("skills/_builtins/some-builtin/plugin.toml"),
        "",
    )
    .unwrap();
    fs::create_dir_all(root_path.join("skills/_drafts/draft-001")).unwrap();
    fs::write(root_path.join("skills/_drafts/draft-001/SKILL.md"), "draft").unwrap();
    fs::create_dir_all(root_path.join("skills/my-custom-skill")).unwrap();
    fs::write(
        root_path.join("skills/my-custom-skill/plugin.toml"),
        "custom",
    )
    .unwrap();

    let scope_key = "t_1__u_2";
    let user_dir = root_path.join("users").join(scope_key);

    app_lib::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path,
        &user_dir,
        scope_key,
        &root_path.join("global/state.json"),
    )
    .unwrap();

    // 用户自定义 skill 应该被复制
    assert!(user_dir.join("skills/my-custom-skill/plugin.toml").exists());
    // 用户草稿应该被复制
    assert!(user_dir.join("skills/_drafts/draft-001/SKILL.md").exists());
    // 内置 _builtins 不应该被复制
    assert!(!user_dir.join("skills/_builtins").exists());
}

/// 业务流程：完整老用户升级链路
/// 模拟真实场景：legacy config.json 中有 cloud_auth + workspacePath + 会话数据
/// → bootstrap 提取 cloud_auth → config 拆分 → 会话数据迁移到用户目录
#[test]
fn full_upgrade_flow_bootstrap_then_split_then_migrate() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();

    // 模拟老版本数据
    let legacy_config = serde_json::json!({
        "cloud_auth": "encrypted_token_here",
        "workspacePath": "/Users/test/workspace",
        "primaryModel": "deepseek-v3",
        "theme": "dark"
    });
    fs::write(root_path.join("config.json"), legacy_config.to_string()).unwrap();
    fs::create_dir_all(root_path.join("conversations/conv-abc")).unwrap();
    fs::write(
        root_path.join("conversations/conv-abc/conv.json"),
        r#"{"id":"conv-abc","title":"测试"}"#,
    )
    .unwrap();
    fs::write(
        root_path.join("conversations/conv-abc/messages.jsonl"),
        r#"{"role":"user","content":"hello"}"#,
    )
    .unwrap();
    fs::write(
        root_path.join("index.json"),
        r#"{"conversations":[{"id":"conv-abc"}]}"#,
    )
    .unwrap();
    fs::create_dir_all(root_path.join("shared/memory")).unwrap();
    fs::write(root_path.join("shared/memory/memory.jsonl"), "[]").unwrap();
    fs::write(
        root_path.join("mcp_servers.json"),
        r#"[{"name":"test-mcp"}]"#,
    )
    .unwrap();
    fs::create_dir_all(root_path.join("schedules")).unwrap();
    fs::write(
        root_path.join("schedules/sched1.json"),
        r#"{"id":"sched1"}"#,
    )
    .unwrap();

    let global_dir = root_path.join("global");
    fs::create_dir_all(&global_dir).unwrap();

    // Step 1: bootstrap cloud_auth（相当于 lib.rs 中 AuthManager::restore 之前）
    app_lib::storage::migration_user_scope::bootstrap_cloud_auth_if_needed(root_path, &global_dir)
        .unwrap();

    // 验证：cloud_auth 已到位，AuthManager 可以读到
    assert_eq!(
        fs::read_to_string(global_dir.join("auth/cloud_auth")).unwrap(),
        "encrypted_token_here"
    );

    // Step 2: derive scope（假设 AuthManager 成功解密得到 tenant=1, user=2）
    let scope_key = "t_1__u_2";
    let user_dir = root_path.join("users").join(scope_key);

    // Step 3: 迁移用户数据
    app_lib::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path,
        &user_dir,
        scope_key,
        &global_dir.join("state.json"),
    )
    .unwrap();

    // Step 4: 拆分 config
    app_lib::storage::migration_user_scope::migrate_legacy_config_if_needed(
        root_path,
        &user_dir,
        &global_dir,
    )
    .unwrap();

    // ========== 验证完整结果 ==========

    // 用户目录下有会话
    assert!(user_dir.join("conversations/conv-abc/conv.json").exists());
    assert!(user_dir
        .join("conversations/conv-abc/messages.jsonl")
        .exists());
    assert!(user_dir.join("index.json").exists());
    assert!(user_dir.join("shared/memory/memory.jsonl").exists());
    assert!(user_dir.join("mcp_servers.json").exists());
    assert!(user_dir.join("schedules/sched1.json").exists());

    // 用户 config 包含 workspacePath 和模型偏好
    let user_cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(user_dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(user_cfg["workspacePath"], "/Users/test/workspace");
    assert_eq!(user_cfg["primaryModel"], "deepseek-v3");
    assert!(user_cfg.get("cloud_auth").is_none()); // cloud_auth 不在 user config 中

    // global config 包含 theme，不含 cloud_auth 和 workspacePath
    let global_cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(global_dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(global_cfg["theme"], "dark");
    assert!(global_cfg.get("cloud_auth").is_none());
    assert!(global_cfg.get("workspacePath").is_none());

    // legacy 数据原件保留
    assert!(root_path.join("conversations/conv-abc/conv.json").exists());
    assert!(root_path.join("config.json").exists());

    // claim 标记正确
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(global_dir.join("state.json")).unwrap()).unwrap();
    assert_eq!(
        state["migrations"]["legacyRootClaim"]["claimedBy"],
        scope_key
    );

    // ========== 切换账号后 B 看不到 A 的数据 ==========
    let scope_b = "t_3__u_4";
    let user_dir_b = root_path.join("users").join(scope_b);
    app_lib::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path,
        &user_dir_b,
        scope_b,
        &global_dir.join("state.json"),
    )
    .unwrap();

    // B 的目录不应该有 A 的会话
    assert!(!user_dir_b.join("conversations").exists());
    assert!(!user_dir_b.join("index.json").exists());
    assert!(!user_dir_b.join("mcp_servers.json").exists());
}

/// 业务流程：FileManager 在登录后切换 workspace
#[test]
fn file_manager_workspace_updates_on_scope_switch() {
    let fm = app_lib::storage::file_manager::FileManager::new("/tmp/old_workspace");
    assert_eq!(
        fm.workspace_path(),
        std::path::PathBuf::from("/tmp/old_workspace")
    );

    // 模拟登录后更新
    fm.update_workspace_path("/tmp/new_workspace");
    assert_eq!(
        fm.workspace_path(),
        std::path::PathBuf::from("/tmp/new_workspace")
    );

    // 模拟登出后重置
    fm.update_workspace_path("/tmp/default_root");
    assert_eq!(
        fm.workspace_path(),
        std::path::PathBuf::from("/tmp/default_root")
    );
}

/// 业务流程：config 拆分幂等——已拆分过（global/config.json 存在）不再重复拆
#[test]
fn config_split_idempotent_when_global_config_exists() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();

    let legacy_config = serde_json::json!({
        "cloud_auth": "token",
        "workspacePath": "/path/a"
    });
    fs::write(root_path.join("config.json"), legacy_config.to_string()).unwrap();

    let global_dir = root_path.join("global");
    fs::create_dir_all(global_dir.join("auth")).unwrap();
    let user_dir = root_path.join("users/t_1__u_2");
    fs::create_dir_all(&user_dir).unwrap();

    // First split
    app_lib::storage::migration_user_scope::migrate_legacy_config_if_needed(
        root_path,
        &user_dir,
        &global_dir,
    )
    .unwrap();
    assert!(global_dir.join("config.json").exists());
    let first_content = fs::read_to_string(global_dir.join("config.json")).unwrap();

    // Modify legacy config (simulate someone manually editing it)
    let new_legacy = serde_json::json!({
        "cloud_auth": "new_token",
        "workspacePath": "/path/b"
    });
    fs::write(root_path.join("config.json"), new_legacy.to_string()).unwrap();

    // Second split should skip entirely because global/config.json already exists
    app_lib::storage::migration_user_scope::migrate_legacy_config_if_needed(
        root_path,
        &user_dir,
        &global_dir,
    )
    .unwrap();

    // global config should NOT have changed
    assert_eq!(
        fs::read_to_string(global_dir.join("config.json")).unwrap(),
        first_content
    );
    // user config should still have /path/a, not /path/b
    let user_cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(user_dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(user_cfg["workspacePath"], "/path/a");
}
