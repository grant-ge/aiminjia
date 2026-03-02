//! load_file handler — load an uploaded file into a variable for execute_python.

use anyhow::{anyhow, Result};
use log::info;
use serde_json::{json, Value};

use crate::llm::orchestrator;
use crate::plugin::context::PluginContext;
use crate::python::parser;
use crate::python::runner::PythonRunner;

use super::{optional_str, require_str};

/// 3. load_file — load an uploaded file into a variable for execute_python.
///
/// Resolves file_id → absolute path, parses the file to get metadata,
/// and stores the mapping in DB so execute_python can auto-inject `_df`/`_text`.
/// The LLM never sees or handles file paths — all path resolution is system-managed.
pub(crate) async fn handle_load_file(ctx: &PluginContext, args: &Value) -> Result<String> {
    let file_id = require_str(args, "file_id")?;
    let sheet = optional_str(args, "sheet");
    let nrows = args.get("nrows").and_then(|v| v.as_u64());

    info!("[TOOL:load_file] file_id='{}' conversation_id='{}' sheet={:?} nrows={:?}",
        file_id, ctx.conversation_id, sheet, nrows);

    // 0. Cache check: if this file was already loaded (same file_id in this conversation),
    //    skip re-parsing and return the cached metadata directly.
    let loaded_key = format!("loaded:{}:{}", ctx.conversation_id, file_id);
    if let Ok(Some(cached)) = ctx.storage.get_memory(&loaded_key) {
        if let Ok(cached_info) = serde_json::from_str::<Value>(&cached) {
            info!("[TOOL:load_file] Cache hit for file_id='{}', skipping re-parse", file_id);
            let output = json!({
                "status": "loaded",
                "originalName": cached_info.get("originalName").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "format": cached_info.get("format").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "loadedAs": cached_info.get("loadedAs").and_then(|v| v.as_str()).unwrap_or("dataframe"),
                "cached": true,
                "usage": if cached_info.get("loadedAs").and_then(|v| v.as_str()) == Some("text") {
                    "文本已加载到变量 _text，可在 execute_python 中直接使用 _text 获取内容"
                } else {
                    "数据已加载到变量 _df，可在 execute_python 中直接使用 _df 进行分析"
                },
            });
            return Ok(serde_json::to_string_pretty(&output)?);
        }
    }

    // 1. Resolve file_id → stored_path → absolute path
    let file_record = ctx
        .storage
        .get_uploaded_file_for_conversation(file_id, &ctx.conversation_id)?
        .ok_or_else(|| anyhow!(
            "Uploaded file not found or does not belong to this conversation: {}",
            file_id
        ))?;

    let stored_path = file_record
        .get("storedPath")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("File record missing storedPath"))?;

    let original_name = file_record
        .get("originalName")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let full_path = ctx.file_manager.full_path(stored_path);

    if !full_path.exists() {
        return Err(anyhow!("File does not exist on disk: {}", full_path.display()));
    }

    // 2. Detect format
    let format = parser::detect_format(&full_path);
    let format_str = serde_json::to_value(&format)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    // 3. Determine loaded variable type based on format
    let loaded_as = match format {
        parser::FileFormat::Csv
        | parser::FileFormat::Excel
        | parser::FileFormat::Json
        | parser::FileFormat::Parquet => "dataframe",

        parser::FileFormat::Text => "text",

        // Mixed types: try table extraction first, fall back to text
        parser::FileFormat::Pdf
        | parser::FileFormat::Word
        | parser::FileFormat::Ppt
        | parser::FileFormat::Html => "auto", // determined after parsing

        parser::FileFormat::Unknown => {
            return Err(anyhow!("Unsupported file format: {}", original_name));
        }
    };

    // 4. Parse the file to get metadata (reuses existing parser)
    let mut parse_sandbox = crate::python::sandbox::SandboxConfig::for_workspace(&ctx.workspace_path);
    parse_sandbox.timeout_seconds = 60;
    let runner = PythonRunner::with_config(
        ctx.workspace_path.clone(), parse_sandbox, ctx.app_handle.as_ref(),
    );
    let parse_result = parser::parse_file(&runner, &full_path).await?;

    // Determine actual loaded_as based on parse result
    let actual_loaded_as = if loaded_as == "auto" {
        if !parse_result.column_names.is_empty() {
            "dataframe"
        } else {
            "text"
        }
    } else {
        loaded_as
    };

    // 5. Store the loaded file mapping in DB for execute_python to read
    //    fileId is stored so the preamble builder can use it as the dict key
    //    (avoids collision when two files have the same original name).
    let loaded_key = format!("loaded:{}:{}", ctx.conversation_id, file_id);
    let loaded_info = json!({
        "path": full_path.to_string_lossy(),
        "format": format_str,
        "originalName": original_name,
        "fileId": file_id,
        "loadedAs": actual_loaded_as,
        "sheet": sheet,
        "nrows": nrows,
    });
    ctx.storage.set_memory(&loaded_key, &loaded_info.to_string(), Some("load_file"))?;

    // When user explicitly loads a (new) file during analysis, clear ALL pkl snapshots
    // (_original, _step_df, _step{N}_df, _step_dfs) so fresh data takes priority.
    if orchestrator::get_step_state(&ctx.storage, &ctx.conversation_id).is_some() {
        let snap_dir = ctx.workspace_path.join("analysis").join(&ctx.conversation_id);
        if snap_dir.exists() {
            for entry in std::fs::read_dir(&snap_dir).into_iter().flatten() {
                if let Ok(e) = entry {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.ends_with(".pkl") || name.ends_with(".pkl.tmp") {
                        if let Err(err) = std::fs::remove_file(e.path()) {
                            log::warn!("[TOOL:load_file] Failed to remove snapshot {}: {}", name, err);
                        } else {
                            info!("[TOOL:load_file] Cleared snapshot {} (new file loaded)", name);
                        }
                    }
                }
            }
        }
    }

    info!("[TOOL:load_file] Stored loaded file mapping: {} -> {} ({})",
        file_id, full_path.display(), actual_loaded_as);

    // 6. Return metadata to LLM (no file path exposed)
    let output = json!({
        "status": "loaded",
        "originalName": original_name,
        "format": format_str,
        "loadedAs": actual_loaded_as,
        "columns": parse_result.column_names,
        "rowCount": parse_result.row_count,
        "schemaSummary": parse_result.schema_summary,
        "sampleData": parse_result.sample_data,
        "usage": if actual_loaded_as == "dataframe" {
            "数据已加载到变量 _df，可在 execute_python 中直接使用 _df 进行分析"
        } else {
            "文本已加载到变量 _text，可在 execute_python 中直接使用 _text 获取内容"
        },
    });

    Ok(serde_json::to_string_pretty(&output)?)
}

/// Build Python preamble code that auto-loads files previously loaded via `load_file`.
///
/// Reads `loaded:{conversation_id}:*` from DB and generates Python code that
/// loads each file into `_df` / `_text` variables before user code runs.
/// If multiple files are loaded, also creates `_dfs` dict keyed by file_id
/// (not original_name, to avoid collision when two files share the same name).
pub(crate) fn build_loaded_files_preamble(
    db: &std::sync::Arc<crate::storage::file_store::AppStorage>,
    conversation_id: &str,
    workspace_path: &std::path::Path,
) -> String {
    let prefix = format!("loaded:{}:", conversation_id);
    let loaded_files = match db.get_memories_by_prefix(&prefix) {
        Ok(files) if !files.is_empty() => files,
        _ => return String::new(),
    };

    // Resolve workspace root for path containment checks
    let workspace_canonical = workspace_path.canonicalize().ok();

    let mut preamble = String::from("\n# ── Auto-loaded files (via load_file) ──\n");
    // (file_id, original_name, path, sheet, nrows)
    let mut df_loads: Vec<(String, String, String, Option<String>, Option<u64>)> = Vec::new();
    // (file_id, original_name, path)
    let mut text_loads: Vec<(String, String, String)> = Vec::new();

    for (_key, value) in &loaded_files {
        let info: Value = match serde_json::from_str(value) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let path = info.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let original_name = info.get("originalName").and_then(|v| v.as_str()).unwrap_or("unknown");
        // Use file_id as dict key to avoid collision when files share the same name.
        // Fall back to original_name for backward compatibility with entries that lack fileId.
        let file_id = info.get("fileId").and_then(|v| v.as_str()).unwrap_or(original_name);
        let loaded_as = info.get("loadedAs").and_then(|v| v.as_str()).unwrap_or("text");
        let sheet = info.get("sheet").and_then(|v| v.as_str()).map(|s| s.to_string());
        let nrows = info.get("nrows").and_then(|v| v.as_u64());

        if path.is_empty() {
            continue;
        }

        // Path containment check: reject paths outside the workspace root
        if let Some(ref ws) = workspace_canonical {
            let file_path = std::path::PathBuf::from(path);
            match file_path.canonicalize() {
                Ok(canonical) if canonical.starts_with(ws) => {} // OK
                Ok(canonical) => {
                    log::warn!(
                        "[preamble] Path {:?} escapes workspace {:?}, skipping",
                        canonical, ws
                    );
                    continue;
                }
                Err(e) => {
                    log::warn!(
                        "[preamble] Cannot resolve path {:?}: {}, skipping",
                        file_path, e
                    );
                    continue;
                }
            }
        }

        match loaded_as {
            "dataframe" => {
                df_loads.push((file_id.to_string(), original_name.to_string(), path.to_string(), sheet, nrows));
            }
            "text" => {
                text_loads.push((file_id.to_string(), original_name.to_string(), path.to_string()));
            }
            _ => {}
        }
    }

    if df_loads.is_empty() && text_loads.is_empty() {
        return String::new();
    }

    // Generate DataFrame loading code
    // Use normal '' strings (not r'') so that py_escape is semantically correct.
    // Dict keys use file_id (UUID) to avoid collision when files share the same name.
    use super::util::{py_comment_safe, py_escape};

    if df_loads.len() == 1 {
        let (_file_id, original_name, path, sheet, nrows) = &df_loads[0];
        let mut load_call = format!("_df = _smart_read_data('{}'", py_escape(path));
        if let Some(s) = sheet {
            load_call.push_str(&format!(", sheet_name='{}'", py_escape(s)));
        }
        if let Some(n) = nrows {
            load_call.push_str(&format!(", nrows={}", n));
        }
        load_call.push(')');
        preamble.push_str(&format!("# Loaded: {}\n{}\n", py_comment_safe(original_name), load_call));
    } else if df_loads.len() > 1 {
        preamble.push_str("_dfs = {}\n");
        for (file_id, original_name, path, sheet, nrows) in &df_loads {
            let safe_key = py_escape(file_id);
            let mut load_call = format!("_dfs['{}'] = _smart_read_data('{}'", safe_key, py_escape(path));
            if let Some(s) = sheet {
                load_call.push_str(&format!(", sheet_name='{}'", py_escape(s)));
            }
            if let Some(n) = nrows {
                load_call.push_str(&format!(", nrows={}", n));
            }
            load_call.push(')');
            preamble.push_str(&format!("# {}\n{}\n", py_comment_safe(original_name), load_call));
        }
        // _df points to the last loaded DataFrame
        if let Some((file_id, _, _, _, _)) = df_loads.last() {
            preamble.push_str(&format!("_df = _dfs['{}']\n", py_escape(file_id)));
        }
    }

    // Generate text loading code
    if text_loads.len() == 1 {
        let (_file_id, original_name, path) = &text_loads[0];
        preamble.push_str(&format!(
            "# Loaded text: {}\nwith open('{}', 'r', encoding='utf-8') as _f:\n    _text = _f.read()\n",
            py_comment_safe(original_name), py_escape(path)
        ));
    } else if text_loads.len() > 1 {
        preamble.push_str("_texts = {}\n");
        for (file_id, original_name, path) in &text_loads {
            preamble.push_str(&format!(
                "# {}\nwith open('{}', 'r', encoding='utf-8') as _f:\n    _texts['{}'] = _f.read()\n",
                py_comment_safe(original_name), py_escape(path), py_escape(file_id)
            ));
        }
        if let Some((file_id, _, _)) = text_loads.last() {
            preamble.push_str(&format!("_text = _texts['{}']\n", py_escape(file_id)));
        }
    }

    // Snapshot restore is now handled in python.rs analysis preamble
    // (three-layer snapshot system with _original, _step_df, _step{N}_df).

    preamble.push('\n');
    preamble
}
