//! execute_python handler.

use anyhow::Result;
use log::{error, info, warn};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::llm::orchestrator;
use crate::plugin::context::PluginContext;
use crate::python::runner::PythonRunner;
use crate::python::sandbox::SandboxConfig;

use super::util::py_escape;
use super::{optional_str, require_str};

/// 2. execute_python — run arbitrary Python code.
pub(crate) async fn handle_execute_python(ctx: &PluginContext, args: &Value) -> Result<String> {
    let code = require_str(args, "code")?;
    let purpose = optional_str(args, "purpose").unwrap_or("code execution");

    info!("[TOOL:execute_python] purpose='{}' code_len={} workspace={:?}",
        purpose, code.len(), ctx.workspace_path);
    info!("[TOOL:execute_python] code:\n{}", code);

    // Validate user code early (before assembling system preamble/epilogue).
    // System-injected code bypasses validation via execute_raw().
    let sandbox = SandboxConfig::for_workspace(&ctx.workspace_path);
    sandbox.validate_code(code).map_err(|e| anyhow::anyhow!("Sandbox violation: {}", e))?;

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
    let step_state = orchestrator::get_step_state(&ctx.storage, &ctx.conversation_id);
    let is_analysis = step_state.is_some();
    if is_analysis {
        let snap_dir = format!("analysis/{}", ctx.conversation_id);
        let current_step = step_state.map(|s| s.step).unwrap_or(0);

        // Write ANALYSIS_UTILS to a module file so Python doesn't
        // re-compile ~50KB of utility code on every execute_python call.
        // Only written once per session — the binary doesn't change mid-run.
        let utils_path = ctx.workspace_path.join("temp/_analysis_utils.py");
        if !utils_path.exists() {
            if let Some(parent) = utils_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            if let Err(e) = std::fs::write(&utils_path, crate::python::analysis_utils::ANALYSIS_UTILS) {
                warn!("[TOOL:execute_python] Failed to write _analysis_utils.py: {}", e);
            }
        }

        // Analysis preamble: three-layer snapshot + _df_raw + _CURRENT_STEP + utils (loaded via exec)
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

# Restore user-created variables from previous execute_python calls
# (e.g. col_map, results, _df_final — anything except _df/_dfs which have dedicated snapshots)
_uv_path = os.path.join(_ANALYSIS_DIR, '_user_vars.pkl')
if os.path.exists(_uv_path):
    try:
        for _k, _v in _pkl.load(open(_uv_path, 'rb')).items():
            globals()[_k] = _v
        del _k, _v
    except Exception:
        pass

# _df_raw: read-only reference to original data (always available)
if os.path.exists(_orig_path):
    _df_raw = _pkl.load(open(_orig_path, 'rb'))
else:
    _df_raw = _df.copy() if '_df' in dir() and isinstance(_df, pd.DataFrame) else None

# Load analysis utility functions from module file
_au_path = os.path.join(os.getcwd(), 'temp', '_analysis_utils.py')
if os.path.exists(_au_path):
    with open(_au_path, encoding='utf-8') as _au_f:
        exec(_au_f.read())

# Snapshot system variable names BEFORE user code runs (used by epilogue to detect user vars)
_SYS_VARS_SNAPSHOT = set(globals().keys())
"#,
            conv_id = py_escape(&ctx.conversation_id),
            step = current_step,
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
    # Auto-save user-created variables (DataFrame, dict, list, etc.)
    # so they persist across execute_python calls in the same conversation.
    # Uses runtime snapshot taken before user code ran — no manual list to maintain.
    _user_vars = {{}}
    _CHEAP_TYPES = (int, float, str, bool, bytes, type(None))
    for _vname, _vval in list(globals().items()):
        if _vname.startswith('__'):
            continue  # skip dunder vars (__name__, __builtins__, etc.)
        if _vname in _SYS_VARS_SNAPSHOT:
            continue  # skip system/snapshot vars (captured before user code)
        if callable(_vval) or isinstance(_vval, type) or type(_vval).__name__ == 'module':
            continue  # skip functions, classes, modules
        try:
            if not isinstance(_vval, _CHEAP_TYPES):
                _pkl.dumps(_vval)  # probe only non-trivial types
            _user_vars[_vname] = _vval
        except Exception:
            pass
    if _user_vars:
        _pkl.dump(_user_vars, open(os.path.join(_snap_dir, '_user_vars.pkl.tmp'), 'wb'))
        os.replace(os.path.join(_snap_dir, '_user_vars.pkl.tmp'),
                   os.path.join(_snap_dir, '_user_vars.pkl'))
except Exception as _e:
    import sys as _sys
    print(f"[WARN] DataFrame snapshot save failed: {{_e}}", file=_sys.stderr)
"#,
            snap_dir = py_escape(&snap_dir)
        );
        info!("[TOOL:execute_python] Appending DataFrame auto-save epilogue (analysis mode, step={})", current_step);
        final_code.push_str(&epilogue);
    }

    let result = if is_analysis {
        // Analysis mode: use persistent session (warm process, no cold-start overhead).
        // The session reuses a long-running Python REPL, eliminating process spawn,
        // pandas/numpy import, and _analysis_utils.py compilation on every call.
        let timeout = std::time::Duration::from_secs(600);
        let session_result = ctx.session_manager
            .execute(&ctx.conversation_id, &final_code, timeout, &sandbox)
            .await?;
        session_result.result
    } else {
        // Daily mode: use one-shot PythonRunner (no persistent state needed)
        let runner = PythonRunner::with_config(ctx.workspace_path.clone(), sandbox, ctx.app_handle.as_ref());
        runner.execute_raw(&final_code).await?
    };

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
                        // Inject error into output so LLM knows the file was NOT created
                        clean_stdout.push_str(&format!(
                            "\n[System: File '{}' was NOT created. Do NOT tell the user the file was exported.]\n",
                            rel_path
                        ));
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
            } else {
                warn!("[TOOL:execute_python] Failed to parse __GENERATED_FILE__ JSON (possibly truncated): {:?}",
                    &json_str[..json_str.len().min(200)]);
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

    // For fixable code errors, instruct LLM to silently retry without apologizing
    if result.exit_code != 0 && is_fixable_code_error(&result.stderr) {
        output.push_str("\n[System: Code error you generated. Fix and retry silently. Do NOT mention this error to the user.]\n");
    }

    Ok(output)
}

/// Check if a Python error is a fixable code error that the LLM should silently retry.
fn is_fixable_code_error(stderr: &str) -> bool {
    const FIXABLE_ERRORS: &[&str] = &[
        "SyntaxError:", "IndentationError:", "NameError:", "TypeError:",
        "AttributeError:", "KeyError:", "ValueError:", "UnboundLocalError:",
    ];
    FIXABLE_ERRORS.iter().any(|p| stderr.contains(p))
}
