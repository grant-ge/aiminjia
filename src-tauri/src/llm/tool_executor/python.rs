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

    // P0: Auto-compact large Python output (tables, describe, value_counts)
    let pre_compact_len = clean_stdout.len();
    let clean_stdout = compact_python_output(&clean_stdout);
    let compact_saved = pre_compact_len.saturating_sub(clean_stdout.len());

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

    // M2: Python execution quality
    let has_error = result.exit_code != 0;
    let timed_out = result.stderr.contains("timed out") || result.stderr.contains("TimeoutError");
    info!(
        "[METRICS:python] conv={} exit_code={} duration_ms={} stdout_chars={} stderr_chars={} compact_saved={}chars | has_error={} timeout={}",
        ctx.conversation_id, result.exit_code, result.execution_time_ms,
        clean_stdout.len(), result.stderr.len(), compact_saved,
        has_error, timed_out,
    );
    crate::telemetry::record("python", &ctx.workspace_path, &[
        ("conv", &ctx.conversation_id),
        ("exit_code", &result.exit_code.to_string()),
        ("duration_ms", &result.execution_time_ms.to_string()),
        ("stdout_chars", &clean_stdout.len().to_string()),
        ("stderr_chars", &result.stderr.len().to_string()),
        ("compact_saved", &compact_saved.to_string()),
        ("has_error", &has_error.to_string()),
        ("timeout", &timed_out.to_string()),
        ("model", &ctx.model),
    ]);

    Ok(output)
}

// ---------------------------------------------------------------------------
// P0: Single-output compaction — compress large pandas output to save context
// ---------------------------------------------------------------------------

/// Threshold (chars) above which stdout is auto-compacted.
const SUMMARY_THRESHOLD_CHARS: usize = 4000;

/// Compact large Python stdout to reduce LLM context usage.
///
/// When the total output exceeds [`SUMMARY_THRESHOLD_CHARS`], the function
/// detects and compresses common pandas output patterns:
///
/// - **DataFrame tables** (aligned columns): keep header + first 3 rows + last row
/// - **`describe()` output**: keep count/mean/std/min/max, fold percentiles
/// - **`value_counts()`**: keep top 5 + bottom 1 + total count
/// - **Plain text** (print statements): preserved verbatim
///
/// Non-table text (print statements, JSON, etc.) is always kept.
pub(crate) fn compact_python_output(stdout: &str) -> String {
    if stdout.len() <= SUMMARY_THRESHOLD_CHARS {
        return stdout.to_string();
    }

    let mut result = String::new();
    let lines: Vec<&str> = stdout.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Try to detect a block starting at line i
        if let Some((compacted, consumed)) = try_compact_block(&lines, i) {
            result.push_str(&compacted);
            result.push('\n');
            i += consumed;
        } else {
            result.push_str(lines[i]);
            result.push('\n');
            i += 1;
        }
    }

    if result.len() < stdout.len() {
        info!(
            "[compact_python_output] Compacted {} → {} chars (saved {})",
            stdout.len(),
            result.len(),
            stdout.len() - result.len()
        );
    }
    result
}

/// Try to detect and compact a pandas output block starting at `start`.
/// Returns `Some((compacted_text, lines_consumed))` or `None` if not a table.
fn try_compact_block(lines: &[&str], start: usize) -> Option<(String, usize)> {
    if start >= lines.len() {
        return None;
    }

    // Detect describe() output: first non-empty line is all column headers,
    // second line starts with a stat name like "count", "mean", etc.
    if let Some(r) = try_compact_describe(lines, start) {
        return Some(r);
    }

    // Detect value_counts() output: lines like "value    count" with optional dtype footer
    if let Some(r) = try_compact_value_counts(lines, start) {
        return Some(r);
    }

    // Detect DataFrame table: aligned columns with separator or index
    if let Some(r) = try_compact_dataframe(lines, start) {
        return Some(r);
    }

    None
}

/// Compact a `describe()` output block.
/// Pattern: header row + stat rows (count, mean, std, min, 25%, 50%, 75%, max).
fn try_compact_describe(lines: &[&str], start: usize) -> Option<(String, usize)> {
    const DESCRIBE_STATS: &[&str] = &[
        "count", "mean", "std", "min", "25%", "50%", "75%", "max",
        "unique", "top", "freq",
    ];
    const KEEP_STATS: &[&str] = &["count", "mean", "std", "min", "max", "unique", "top", "freq"];
    const FOLD_STATS: &[&str] = &["25%", "50%", "75%"];

    // Need at least header + a few stat lines
    if start + 3 >= lines.len() {
        return None;
    }

    // Check: line after header should start with a known stat name
    let header_line = lines[start];
    let first_stat_line_idx = start + 1;
    let first_stat_line = lines[first_stat_line_idx].trim_start();

    let is_describe = DESCRIBE_STATS.iter().any(|s| first_stat_line.starts_with(s));
    if !is_describe {
        return None;
    }

    // Collect all stat lines
    let mut end = first_stat_line_idx;
    while end < lines.len() {
        let trimmed = lines[end].trim_start();
        let is_stat = DESCRIBE_STATS.iter().any(|s| trimmed.starts_with(s));
        if !is_stat && !trimmed.is_empty() && end > first_stat_line_idx {
            break;
        }
        if trimmed.is_empty() && end > first_stat_line_idx {
            break;
        }
        end += 1;
    }

    let consumed = end - start;
    if consumed < 4 {
        return None; // Too few lines to be a describe block
    }

    let mut out = String::new();
    out.push_str(header_line);
    out.push('\n');

    let mut folded_parts: Vec<&str> = Vec::new();
    for &line in &lines[first_stat_line_idx..end] {
        let trimmed = line.trim_start();
        if FOLD_STATS.iter().any(|s| trimmed.starts_with(s)) {
            folded_parts.push(trimmed.split_whitespace().next().unwrap_or(""));
        } else if KEEP_STATS.iter().any(|s| trimmed.starts_with(s)) {
            out.push_str(line);
            out.push('\n');
        } else if !trimmed.is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !folded_parts.is_empty() {
        out.push_str(&format!("[percentiles ({}) omitted]\n", folded_parts.join(",")));
    }

    Some((out, consumed))
}

/// Compact a `value_counts()` output block.
/// Pattern: value + count pairs, possibly a "Name:" / "dtype:" footer.
fn try_compact_value_counts(lines: &[&str], start: usize) -> Option<(String, usize)> {
    // value_counts typically looks like:
    //   Category A    45
    //   Category B    32
    //   ...
    //   Name: col, dtype: int64
    // OR just:
    //   Category A    45
    //   Category B    32
    //   ...
    //   Name: col, Length: N, dtype: int64

    // Heuristic: at least 8 lines of "label  number" pattern to be worth compacting
    let mut data_lines: Vec<usize> = Vec::new();
    let mut footer_end = start;
    let mut has_name_footer = false;

    for idx in start..lines.len() {
        let line = lines[idx].trim();
        if line.is_empty() {
            footer_end = idx + 1;
            break;
        }
        if line.starts_with("Name:") || line.starts_with("dtype:") || line.starts_with("Length:") {
            has_name_footer = true;
            footer_end = idx + 1;
            // Consume any trailing blank line
            if footer_end < lines.len() && lines[footer_end].trim().is_empty() {
                footer_end += 1;
            }
            break;
        }
        // Check if this looks like a value_count row: text followed by whitespace and a number
        let parts: Vec<&str> = line.rsplitn(2, char::is_whitespace).collect();
        if parts.len() == 2 && parts[0].trim().parse::<f64>().is_ok() {
            data_lines.push(idx);
        } else {
            // Not a value_counts pattern
            if data_lines.is_empty() {
                return None;
            }
            footer_end = idx;
            break;
        }
        footer_end = idx + 1;
    }

    // Need at least 8 data lines to be worth compacting
    if data_lines.len() < 8 {
        return None;
    }

    let consumed = footer_end - start;

    // Keep top 5 + bottom 1
    let mut out = String::new();
    for &idx in data_lines.iter().take(5) {
        out.push_str(lines[idx]);
        out.push('\n');
    }
    out.push_str(&format!("[...{} more values]\n", data_lines.len() - 6));
    if let Some(&last_idx) = data_lines.last() {
        out.push_str(lines[last_idx]);
        out.push('\n');
    }
    if has_name_footer {
        // Include the footer line
        let footer_idx = footer_end - if lines.get(footer_end - 1).map_or(false, |l| l.trim().is_empty()) { 2 } else { 1 };
        if footer_idx < lines.len() {
            out.push_str(lines[footer_idx]);
            out.push('\n');
        }
    }
    out.push_str(&format!("[total: {} unique values]\n", data_lines.len()));

    Some((out, consumed))
}

/// Compact a DataFrame table output.
/// Pattern: column header row + data rows (aligned with spaces).
///
/// Heuristics:
/// - A "header" line has 2+ space-separated tokens
/// - Subsequent lines are "data rows" if they have a similar column structure
/// - If total data rows > 6, keep first 3 + last 1 + summary
fn try_compact_dataframe(lines: &[&str], start: usize) -> Option<(String, usize)> {
    if start >= lines.len() {
        return None;
    }

    let header_line = lines[start];
    let header_tokens: Vec<&str> = header_line.split_whitespace().collect();
    if header_tokens.len() < 3 {
        return None;
    }

    // Count consecutive data rows after the header
    let mut data_start = start + 1;
    let mut data_end = data_start;

    // Skip optional separator line (e.g., "---" or "===")
    if data_start < lines.len() {
        let sep = lines[data_start].trim();
        if sep.chars().all(|c| c == '-' || c == '=' || c == ' ' || c == '+') && sep.len() > 3 {
            data_start += 1;
            data_end = data_start;
        }
    }

    // Data rows: non-empty lines with content
    while data_end < lines.len() {
        let line = lines[data_end].trim();
        if line.is_empty() {
            break;
        }
        // Footer detection: "[N rows x M columns]"
        if line.starts_with('[') && line.contains("rows") {
            data_end += 1;
            break;
        }
        data_end += 1;
    }

    let total_data_rows = data_end - data_start;
    let consumed = data_end - start;

    // Only compact if there are enough rows to make it worthwhile
    if total_data_rows <= 6 {
        return None;
    }

    let mut out = String::new();
    // Header
    out.push_str(header_line);
    out.push('\n');
    // Separator if present
    if data_start > start + 1 {
        out.push_str(lines[start + 1]);
        out.push('\n');
    }
    // First 3 data rows
    for i in data_start..(data_start + 3).min(data_end) {
        out.push_str(lines[i]);
        out.push('\n');
    }
    // Omission marker
    out.push_str(&format!("[...{} more rows hidden from display, full data still available in _df]\n", total_data_rows.saturating_sub(4)));
    // Last data row
    if data_end > data_start {
        let last_data = data_end - 1;
        // Check if last line is a footer
        let last_trimmed = lines[last_data].trim();
        if last_trimmed.starts_with('[') && last_trimmed.contains("rows") {
            // The footer line — include it; use second-to-last as data
            if last_data > data_start {
                out.push_str(lines[last_data - 1]);
                out.push('\n');
            }
            out.push_str(lines[last_data]);
            out.push('\n');
        } else {
            out.push_str(lines[last_data]);
            out.push('\n');
        }
    }

    Some((out, consumed))
}

/// Check if a Python error is a fixable code error that the LLM should silently retry.
fn is_fixable_code_error(stderr: &str) -> bool {
    const FIXABLE_ERRORS: &[&str] = &[
        "SyntaxError:", "IndentationError:", "NameError:", "TypeError:",
        "AttributeError:", "KeyError:", "ValueError:", "UnboundLocalError:",
    ];
    FIXABLE_ERRORS.iter().any(|p| stderr.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_below_threshold_is_noop() {
        let short = "hello\nworld\n";
        assert_eq!(compact_python_output(short), short);
    }

    #[test]
    fn compact_describe_folds_percentiles() {
        // Build a describe() block that exceeds the threshold
        let mut input = String::new();
        // Header
        input.push_str("         salary    bonus    tenure\n");
        input.push_str("count   1000.00  1000.00   1000.00\n");
        input.push_str("mean   50000.00  5000.00     5.50\n");
        input.push_str("std    15000.00  2000.00     3.20\n");
        input.push_str("min    20000.00   500.00     0.10\n");
        input.push_str("25%    38000.00  3500.00     2.80\n");
        input.push_str("50%    48000.00  4800.00     5.00\n");
        input.push_str("75%    60000.00  6200.00     8.00\n");
        input.push_str("max    95000.00 12000.00    15.00\n");
        // Pad to exceed threshold
        input.push_str(&"x".repeat(SUMMARY_THRESHOLD_CHARS));

        let result = compact_python_output(&input);
        assert!(result.contains("count"));
        assert!(result.contains("mean"));
        assert!(result.contains("max"));
        assert!(result.contains("percentiles"));
        // Percentile rows should be folded
        assert!(!result.contains("\n50%"));
    }

    #[test]
    fn compact_value_counts_keeps_top5_bottom1() {
        let mut input = String::new();
        for i in 0..20 {
            input.push_str(&format!("Category_{}    {}\n", i, 100 - i * 5));
        }
        input.push_str("Name: department, dtype: int64\n");
        // Pad to exceed threshold
        input.push_str(&"z".repeat(SUMMARY_THRESHOLD_CHARS));

        let result = compact_python_output(&input);
        // Should keep first 5 categories
        assert!(result.contains("Category_0"));
        assert!(result.contains("Category_4"));
        // Should have omission marker
        assert!(result.contains("more values"));
        // Should keep last value
        assert!(result.contains("Category_19"));
    }

    #[test]
    fn compact_dataframe_table() {
        let mut input = String::new();
        input.push_str("   name    salary  department  level\n");
        for i in 0..20 {
            input.push_str(&format!("{}  Alice{}  50000      Engineering  L{}\n", i, i, i));
        }
        // Pad to exceed threshold
        input.push_str(&"p".repeat(SUMMARY_THRESHOLD_CHARS));

        let result = compact_python_output(&input);
        assert!(result.contains("name"));
        // First 3 data rows kept
        assert!(result.contains("Alice0"));
        assert!(result.contains("Alice2"));
        // Should have omission marker
        assert!(result.contains("more rows"));
    }

    #[test]
    fn compact_preserves_plain_text() {
        let mut input = String::new();
        input.push_str("Data loaded successfully.\n");
        input.push_str("Found 1000 rows and 15 columns.\n");
        input.push_str("Processing complete.\n");
        // Pad to exceed threshold
        input.push_str(&"t".repeat(SUMMARY_THRESHOLD_CHARS));

        let result = compact_python_output(&input);
        assert!(result.contains("Data loaded successfully."));
        assert!(result.contains("Found 1000 rows"));
        assert!(result.contains("Processing complete."));
    }

    #[test]
    fn compact_mixed_output() {
        let mut input = String::new();
        input.push_str("Loading data...\n");
        input.push_str("         salary    bonus\n");
        input.push_str("count   1000.00  1000.00\n");
        input.push_str("mean   50000.00  5000.00\n");
        input.push_str("std    15000.00  2000.00\n");
        input.push_str("min    20000.00   500.00\n");
        input.push_str("25%    38000.00  3500.00\n");
        input.push_str("50%    48000.00  4800.00\n");
        input.push_str("75%    60000.00  6200.00\n");
        input.push_str("max    95000.00 12000.00\n");
        input.push_str("\nDone.\n");
        // Pad to exceed threshold
        input.push_str(&"q".repeat(SUMMARY_THRESHOLD_CHARS));

        let result = compact_python_output(&input);
        assert!(result.contains("Loading data..."));
        assert!(result.contains("Done."));
        assert!(result.contains("percentiles"));
    }
}
