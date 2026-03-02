//! execute_python handler.

use anyhow::Result;
use log::{error, info, warn};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::llm::orchestrator;
use crate::plugin::context::PluginContext;
use crate::python::runner::PythonRunner;

use super::util::py_escape;
use super::{optional_str, require_str};

/// 2. execute_python — run arbitrary Python code.
pub(crate) async fn handle_execute_python(ctx: &PluginContext, args: &Value) -> Result<String> {
    let code = require_str(args, "code")?;
    let purpose = optional_str(args, "purpose").unwrap_or("code execution");

    info!("[TOOL:execute_python] purpose='{}' code_len={} workspace={:?}",
        purpose, code.len(), ctx.workspace_path);
    info!("[TOOL:execute_python] code:\n{}", code);

    // Auto-load uploaded files that haven't been loaded via load_file yet.
    // This ensures _df/_text variables are available even if the LLM skips load_file.
    {
        let uploaded_files = ctx.storage.get_uploaded_files_for_conversation(&ctx.conversation_id)
            .unwrap_or_default();
        for file in &uploaded_files {
            let file_id = file.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if file_id.is_empty() { continue; }
            let loaded_key = format!("loaded:{}:{}", ctx.conversation_id, file_id);
            if ctx.storage.get_memory(&loaded_key).ok().flatten().is_none() {
                info!("[TOOL:execute_python] Auto-loading file '{}' for conversation {}",
                    file_id, ctx.conversation_id);
                let load_args = json!({"file_id": file_id});
                if let Err(e) = super::file_load::handle_load_file(ctx, &load_args).await {
                    warn!("[TOOL:execute_python] Auto-load failed for '{}': {}", file_id, e);
                }
            }
        }
    }

    // Auto-inject loaded files preamble (from load_file tool)
    let loaded_preamble = super::file_load::build_loaded_files_preamble(&ctx.storage, &ctx.conversation_id, &ctx.workspace_path);
    let mut final_code = if loaded_preamble.is_empty() {
        code.to_string()
    } else {
        info!("[TOOL:execute_python] Injecting loaded files preamble ({} bytes)",
            loaded_preamble.len());
        format!("{}\n{}", loaded_preamble, code)
    };

    // In analysis mode:
    // 1. Inject three-layer snapshot system + _df_raw + _CURRENT_STEP
    // 2. Inject pre-written analysis utility functions
    // 3. Append DataFrame auto-save epilogue (working + step snapshots)
    let is_analysis = orchestrator::get_step_state(&ctx.storage, &ctx.conversation_id).is_some();
    if is_analysis {
        let snap_dir = format!("analysis/{}", ctx.conversation_id);
        let current_step = orchestrator::get_step_state(&ctx.storage, &ctx.conversation_id)
            .map(|s| s.step)
            .unwrap_or(0);

        // Analysis preamble: three-layer snapshot + _df_raw + _CURRENT_STEP + utils
        let analysis_preamble = format!(
            r#"
_CONV_ID = '{conv_id}'
_ANALYSIS_DIR = os.path.join(os.getcwd(), 'analysis', _CONV_ID)
os.makedirs(_ANALYSIS_DIR, exist_ok=True)
_CURRENT_STEP = {step}

# Layer 1: Save original data (first time only, never modified)
import pickle as _pkl
_orig_path = os.path.join(_ANALYSIS_DIR, '_original.pkl')
if '_df' in dir() and isinstance(_df, pd.DataFrame) and not os.path.exists(_orig_path):
    _pkl.dump(_df.copy(), open(_orig_path + '.tmp', 'wb'))
    os.replace(_orig_path + '.tmp', _orig_path)

# Layer 3: Restore working snapshot (overrides file-loaded _df)
_snap_path = os.path.join(_ANALYSIS_DIR, '_step_df.pkl')
if os.path.exists(_snap_path):
    _df = _pkl.load(open(_snap_path, 'rb'))

# Restore _dfs snapshot if exists
_snap_dfs_path = os.path.join(_ANALYSIS_DIR, '_step_dfs.pkl')
if os.path.exists(_snap_dfs_path):
    _dfs = _pkl.load(open(_snap_dfs_path, 'rb'))

# _df_raw: read-only reference to original data (always available)
if os.path.exists(_orig_path):
    _df_raw = _pkl.load(open(_orig_path, 'rb'))
else:
    _df_raw = _df.copy() if '_df' in dir() and isinstance(_df, pd.DataFrame) else None

{utils}
"#,
            conv_id = py_escape(&ctx.conversation_id),
            step = current_step,
            utils = crate::python::analysis_utils::ANALYSIS_UTILS,
        );
        info!("[TOOL:execute_python] Injecting analysis preamble ({} bytes, step={}) for conversation {}",
            analysis_preamble.len(), current_step, ctx.conversation_id);
        final_code = format!("{}{}", analysis_preamble, final_code);

        // Epilogue: save working snapshot + step snapshot
        let epilogue = format!(
            r#"

# ── System: DataFrame auto-save (three-layer snapshots) ──
try:
    import pickle as _pkl
    _snap_dir = os.path.join(os.getcwd(), '{snap_dir}')
    os.makedirs(_snap_dir, exist_ok=True)
    if '_df' in dir() and isinstance(_df, pd.DataFrame):
        # Layer 3: Working snapshot (persists across execute_python calls)
        _pkl.dump(_df, open(os.path.join(_snap_dir, '_step_df.pkl.tmp'), 'wb'))
        os.replace(os.path.join(_snap_dir, '_step_df.pkl.tmp'),
                   os.path.join(_snap_dir, '_step_df.pkl'))
        # Layer 2: Step snapshot (for per-step rollback)
        _step_snap = os.path.join(_snap_dir, f'_step{{_CURRENT_STEP}}_df.pkl')
        _pkl.dump(_df, open(_step_snap + '.tmp', 'wb'))
        os.replace(_step_snap + '.tmp', _step_snap)
    if '_dfs' in dir() and isinstance(_dfs, dict):
        _pkl.dump(_dfs, open(os.path.join(_snap_dir, '_step_dfs.pkl.tmp'), 'wb'))
        os.replace(os.path.join(_snap_dir, '_step_dfs.pkl.tmp'),
                   os.path.join(_snap_dir, '_step_dfs.pkl'))
except Exception:
    pass
"#,
            snap_dir = py_escape(&snap_dir)
        );
        info!("[TOOL:execute_python] Appending DataFrame auto-save epilogue (analysis mode, step={})", current_step);
        final_code.push_str(&epilogue);
    }

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&final_code).await?;  // Timeout now returns Err, propagated by ?

    info!("[TOOL:execute_python] exit_code={} time={}ms stdout_len={} stderr_len={}",
        result.exit_code, result.execution_time_ms,
        result.stdout.len(), result.stderr.len());

    // Auto-register files created by _export_detail (detected via __GENERATED_FILE__ markers)
    let mut generated_files_info = Vec::new();
    let mut clean_stdout = String::new();
    for line in result.stdout.lines() {
        if let Some(json_str) = line.strip_prefix("__GENERATED_FILE__:") {
            if let Ok(file_meta) = serde_json::from_str::<Value>(json_str) {
                let rel_path = file_meta.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let filename = file_meta.get("filename").and_then(|v| v.as_str()).unwrap_or("");
                let title = file_meta.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let fmt = file_meta.get("format").and_then(|v| v.as_str()).unwrap_or("excel");

                let full_path = ctx.workspace_path.join(rel_path);
                // Path traversal guard: resolve symlinks/.. and reject paths outside workspace
                let canonical = match full_path.canonicalize() {
                    Ok(p) => p,
                    Err(_) => {
                        warn!("[TOOL:execute_python] Skipping __GENERATED_FILE__: path does not exist: {:?}", full_path);
                        continue;
                    }
                };
                let workspace_canonical = ctx.workspace_path.canonicalize().unwrap_or_else(|_| ctx.workspace_path.clone());
                if !canonical.starts_with(&workspace_canonical) {
                    error!("[TOOL:execute_python] Path traversal blocked in __GENERATED_FILE__: {:?} escapes workspace {:?}", canonical, workspace_canonical);
                    continue;
                }
                let file_size = std::fs::metadata(&canonical).map(|m| m.len() as i64).unwrap_or(0);
                let file_id = Uuid::new_v4().to_string();

                if let Err(e) = ctx.storage.insert_generated_file(
                    &file_id,
                    &ctx.conversation_id,
                    None,
                    filename,
                    rel_path,
                    fmt,
                    file_size,
                    "data",
                    Some(title),
                    1, true, None, None, None,
                ) {
                    error!("Failed to register generated file '{}': {}", filename, e);
                } else {
                    info!("[TOOL:execute_python] auto-registered file: {} ({})", filename, file_id);
                    generated_files_info.push(json!({
                        "fileId": file_id,
                        "fileName": filename,
                        "storedPath": rel_path,
                        "fileSize": file_size,
                    }));
                }
            }
            // Don't include the marker line in output
        } else {
            clean_stdout.push_str(line);
            clean_stdout.push('\n');
        }
    }

    let mut output = String::new();
    output.push_str(&format!("[Purpose: {}]\n", purpose));
    output.push_str(&format!("Exit code: {}\n", result.exit_code));
    output.push_str(&format!(
        "Execution time: {}ms\n",
        result.execution_time_ms
    ));

    if !clean_stdout.is_empty() {
        output.push_str(&format!("\n--- stdout ---\n{}\n", clean_stdout.trim_end()));
    }
    if !result.stderr.is_empty() {
        output.push_str(&format!("\n--- stderr ---\n{}\n", result.stderr));
    }

    // Append generated file info so the LLM knows about registered files
    if !generated_files_info.is_empty() {
        output.push_str("\n--- generated_files ---\n");
        for fi in &generated_files_info {
            output.push_str(&format!(
                "File registered: {} (fileId: {}, size: {} bytes)\n",
                fi["fileName"].as_str().unwrap_or(""),
                fi["fileId"].as_str().unwrap_or(""),
                fi["fileSize"].as_i64().unwrap_or(0),
            ));
        }
    }

    Ok(output)
}
