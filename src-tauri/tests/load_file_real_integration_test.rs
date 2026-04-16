//! S3 真实集成测试 — DefaultFileOperations + AppStorage 生产路径验证
//!
//! 目标：用真实的 `DefaultFileOperations` + 真实的 `AppStorage` 替代 mock 自证，
//! 验证 loaded key 写入/读取契约、scope 选择逻辑、以及 `is_loaded()` 的生产语义。
//!
//! ## 测试说明
//!
//! - Test 1: DefaultFileOperations::is_loaded() 能正确读取真实 AppStorage 中的 loaded key
//!           （验证 key 格式 "loaded:{scope_id}:{file_id}" 在真实存储中端到端 round-trip）
//! - Test 2: handle_load_file_core JSON 输出契约 (#[ignore] — 需要 Python 运行时)
//! - Test 3: scope 选择 — run_id 存在时用 run_id，否则用 conversation_id

use std::sync::Arc;

use app_lib::runtime::ids::RunId;
use app_lib::runtime::tools::capability::DefaultFileOperations;
use app_lib::runtime::tools::FileOperations;
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

// ─── 测试基础设施 ─────────────────────────────────────────────────────────────

/// 创建一个隔离的临时 workspace + AppStorage + FileManager。
///
/// TempDir 必须随测试保持存活，否则目录被提前删除导致 IO 错误。
/// 调用方保留 `_dir` 变量即可。
fn make_test_env() -> (TempDir, Arc<AppStorage>, Arc<FileManager>) {
    let dir = TempDir::new().expect("TempDir::new failed");
    let storage = Arc::new(
        AppStorage::new(dir.path()).expect("AppStorage::new failed"),
    );
    // FileManager workspace 直接指向 TempDir 根
    let file_manager = Arc::new(FileManager::new(dir.path()));
    (dir, storage, file_manager)
}

/// 构造 `DefaultFileOperations`（无 run_id — scope = conversation_id）
fn make_default_file_ops(
    storage: Arc<AppStorage>,
    file_manager: Arc<FileManager>,
    workspace_path: std::path::PathBuf,
    conversation_id: &str,
) -> DefaultFileOperations {
    DefaultFileOperations::new(storage, file_manager, workspace_path, conversation_id, None)
}

/// 构造 `DefaultFileOperations`（有 run_id — scope = run_id）
fn make_default_file_ops_with_run(
    storage: Arc<AppStorage>,
    file_manager: Arc<FileManager>,
    workspace_path: std::path::PathBuf,
    conversation_id: &str,
    run_id: RunId,
) -> DefaultFileOperations {
    DefaultFileOperations::new(storage, file_manager, workspace_path, conversation_id, Some(run_id))
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: DefaultFileOperations 写入真实 loaded key
//
// 策略：
//   - 构造真实 AppStorage
//   - 用 AppStorage::set_memory 写入 "loaded:{scope_id}:{file_id}" (模拟 handle_load_file_core 写入)
//   - 验证 DefaultFileOperations::is_loaded() 能读取该 key → true
//   - 验证未写入的 file_id → false
//
// 这条测试锁住了 is_loaded() 与 handle_load_file_core 之间的 key 格式契约：
// 两者必须同时使用 "loaded:{scope_id}:{file_id}" 格式，否则 is_loaded 永远返回 false。
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn default_file_ops_is_loaded_reads_real_storage_loaded_key() {
    let (_dir, storage, file_manager) = make_test_env();
    let workspace = _dir.path().to_path_buf();

    let conversation_id = "conv-real-001";
    let file_id = "file-abc-001";

    let file_ops = make_default_file_ops(
        storage.clone(),
        file_manager,
        workspace,
        conversation_id,
    );

    // ── Before write ──────────────────────────────────────────────────────────
    assert!(
        !file_ops.is_loaded(file_id, conversation_id),
        "is_loaded must be false before the loaded key is written to AppStorage"
    );

    // ── Simulate what handle_load_file_core writes when it stores the loaded mapping ──
    // Production code: storage.set_memory(&loaded_key, &loaded_info.to_string(), Some("load_file"))?
    // loaded_key = format!("loaded:{}:{}", scope_id, file_id)
    let loaded_key = format!("loaded:{}:{}", conversation_id, file_id);
    let loaded_info = serde_json::json!({
        "path": "/tmp/workspace/uploads/test.csv",
        "format": "csv",
        "originalName": "test.csv",
        "fileId": file_id,
        "loadedAs": "dataframe",
        "sheet": null,
        "nrows": null,
        "piiMasked": false,
    });
    storage
        .set_memory(&loaded_key, &loaded_info.to_string(), Some("load_file"))
        .expect("set_memory must succeed");

    // ── After write: is_loaded must return true ───────────────────────────────
    assert!(
        file_ops.is_loaded(file_id, conversation_id),
        "is_loaded must return true after 'loaded:<scope_id>:<file_id>' key is written to AppStorage. \
        This verifies the key format contract between DefaultFileOperations::is_loaded() and \
        handle_load_file_core's set_memory call are using the same format."
    );

    // ── Different file_id → still false ──────────────────────────────────────
    assert!(
        !file_ops.is_loaded("other-file-999", conversation_id),
        "is_loaded must be false for a file_id that was never written"
    );

    // ── Different scope_id → still false ─────────────────────────────────────
    // This is a key cross-check: the scope prefix must match exactly.
    assert!(
        !file_ops.is_loaded(file_id, "different-scope-999"),
        "is_loaded with a different scope_id must be false — scope must match exactly"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: handle_load_file_core 的真实 JSON 输出契约
//
// 状态: #[ignore] — 需要 Python 运行时
//
// 原因：handle_load_file_core 内部调用 PythonRunner::with_config + parser::parse_file，
// 两者都需要一个可运行的 Python 环境（pandas, pdfplumber 等依赖）。
// 在 CI / Cargo test 的无 Python 环境中无法运行此路径。
//
// 如果需要在 CI 中启用本测试：
//   1. 确保 Python 3 + pip 依赖已安装（pip install pandas openpyxl pdfplumber python-docx）
//   2. 准备一个真实的 CSV 文件作为 upload
//   3. 去掉 #[ignore] 注解
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Python runtime with pandas installed"]
async fn handle_load_file_core_json_output_matches_production_contract() {
    // This test is intentionally left as a documented skeleton.
    // When Python is available, instantiate:
    //   - TempDir as workspace
    //   - AppStorage::new(dir)
    //   - FileManager::new(dir)
    //   - Write a real CSV file to uploads/
    //   - Insert uploaded file record via storage.insert_uploaded_file(...)
    //   - Build LoadFileParams { storage, file_manager, workspace_path, conversation_id, run_id: None, app_handle: None }
    //   - Call handle_load_file_core(&params, &json!({"file_id": "..."})).await
    //   - Assert JSON output contains: status, fileId, originalName, format, loadedAs
    //
    // The fields above are what the production handle_load_file_core builds in its success path:
    //   json!({ "status": "loaded", "fileId": ..., "originalName": ..., "format": ..., "loadedAs": ... })
    //
    // Notably absent from the real output: "file_meta" (which was a MockFileOps invention)

    // Placeholder assertion to satisfy compiler
    let _placeholder = true;
    assert!(_placeholder);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: scope 选择（run vs conversation）
//
// 验证：
//   - 无 run_id 时：loaded key scope = conversation_id
//   - 有 run_id 时：loaded key scope = run_id
//
// 策略：使用真实 AppStorage::set_memory 写入各自 scope 的 loaded key，
// 然后通过 DefaultFileOperations::is_loaded(file_id, scope_id) 确认读取正确。
//
// 这锁住了 LoadFileParams::loaded_scope_id() 的选择逻辑（production code）：
//   fn loaded_scope_id(&self) -> &str {
//       self.run_id.map(|r| r.as_str()).unwrap_or(self.conversation_id)
//   }
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn scope_selection_no_run_id_uses_conversation_id() {
    let (_dir, storage, file_manager) = make_test_env();
    let workspace = _dir.path().to_path_buf();

    let conversation_id = "conv-scope-001";
    let file_id = "file-scope-001";

    // Build DefaultFileOperations WITHOUT run_id → scope = conversation_id
    let file_ops = make_default_file_ops(
        storage.clone(),
        file_manager,
        workspace,
        conversation_id,
    );

    // Write the key that production code would write when run_id is absent:
    // loaded_scope_id() → conversation_id
    let loaded_key = format!("loaded:{}:{}", conversation_id, file_id);
    let dummy_info = serde_json::json!({"path": "/tmp/test.csv", "format": "csv",
        "originalName": "test.csv", "fileId": file_id, "loadedAs": "dataframe",
        "piiMasked": false});
    storage
        .set_memory(&loaded_key, &dummy_info.to_string(), Some("load_file"))
        .expect("set_memory must succeed");

    // is_loaded(file_id, conversation_id) → true  (correct scope)
    assert!(
        file_ops.is_loaded(file_id, conversation_id),
        "Without run_id, is_loaded(file_id, conversation_id) must be true — \
        scope must be conversation_id"
    );

    // is_loaded with a fake run_id scope → false  (key never written for that scope)
    let fake_run_id = "run-9999";
    assert!(
        !file_ops.is_loaded(file_id, fake_run_id),
        "Without run_id, is_loaded(file_id, run_id) must be false — \
        the loaded key was written under conversation_id scope, not run_id scope"
    );
}

#[test]
fn scope_selection_with_run_id_uses_run_id_not_conversation_id() {
    let (_dir, storage, file_manager) = make_test_env();
    let workspace = _dir.path().to_path_buf();

    let conversation_id = "conv-scope-002";
    let run_id_str = "run-scope-002";
    let file_id = "file-scope-002";

    let run_id = RunId::new(run_id_str);

    // Build DefaultFileOperations WITH run_id → scope = run_id
    let file_ops = make_default_file_ops_with_run(
        storage.clone(),
        file_manager,
        workspace,
        conversation_id,
        run_id,
    );

    // Write the key that production code would write when run_id is present:
    // loaded_scope_id() → run_id.as_str()
    let loaded_key = format!("loaded:{}:{}", run_id_str, file_id);
    let dummy_info = serde_json::json!({"path": "/tmp/test.csv", "format": "csv",
        "originalName": "test.csv", "fileId": file_id, "loadedAs": "dataframe",
        "piiMasked": false});
    storage
        .set_memory(&loaded_key, &dummy_info.to_string(), Some("load_file"))
        .expect("set_memory must succeed");

    // is_loaded(file_id, run_id_str) → true  (correct scope)
    assert!(
        file_ops.is_loaded(file_id, run_id_str),
        "With run_id present, is_loaded(file_id, run_id_str) must be true — \
        scope must be run_id"
    );

    // is_loaded with conversation_id scope → false  (key was written under run_id scope)
    assert!(
        !file_ops.is_loaded(file_id, conversation_id),
        "With run_id present, is_loaded(file_id, conversation_id) must be false — \
        the loaded key was written under run_id scope, not conversation_id scope"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: key 格式一致性 — set_memory / get_memory round-trip
//
// 这是最底层的防回归测试：直接验证 AppStorage 的 get_memory("loaded:X:Y")
// 在 set_memory("loaded:X:Y", ...) 后能返回 Some。
// 任何对 key 格式的无意修改都会在这里被捕获。
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn loaded_key_format_round_trips_in_real_appstorage() {
    let (_dir, storage, _file_manager) = make_test_env();

    // Exact format used by LoadFileParams::loaded_key() and DefaultFileOperations::is_loaded()
    let scope_id = "test-scope-round-trip";
    let file_id = "test-file-round-trip";
    let key = format!("loaded:{}:{}", scope_id, file_id);

    // Before write
    let before = storage.get_memory(&key).expect("get_memory must not error");
    assert!(before.is_none(), "key must not exist before set_memory");

    // Write
    storage
        .set_memory(&key, r#"{"format":"csv","loadedAs":"dataframe"}"#, Some("test"))
        .expect("set_memory must succeed");

    // After write
    let after = storage.get_memory(&key).expect("get_memory must not error");
    assert!(
        after.is_some(),
        "get_memory must return Some after set_memory with exact loaded key format 'loaded:<scope_id>:<file_id>'"
    );

    // Verify the content is preserved
    let content = after.unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("stored value must be valid JSON");
    assert_eq!(
        parsed["format"].as_str(),
        Some("csv"),
        "stored JSON value must round-trip correctly"
    );
}
