//! load_file handler — load an uploaded file into a variable for execute_python.
//!
//! Includes PII masking for uploaded files: tabular data (CSV/Excel/JSON/Parquet)
//! is masked via a Python script; text data (PDF/Word) is masked via Rust masking.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use log::{info, warn};
use serde_json::{json, Value};

use crate::llm::masking::{MaskingContext, MaskingLevel};
use crate::llm::orchestrator;
use crate::plugin::context::PluginContext;
use crate::python::parser;
use crate::python::runner::PythonRunner;

use super::{optional_str, require_str};

// ---------------------------------------------------------------------------
// Embedded Python script for tabular PII masking
// ---------------------------------------------------------------------------

/// Python script that detects PII columns in tabular data and replaces values
/// with placeholder tokens. Outputs JSON result to stdout.
///
/// Injected parameters: INPUT_PATH, OUTPUT_PATH, FILE_FORMAT.
const PII_MASK_SCRIPT: &str = r#"
import pandas as pd
import json
import sys
import re
import os

INPUT_PATH = '{input_path}'
OUTPUT_PATH = '{output_path}'
FILE_FORMAT = '{file_format}'

# ── Column name patterns for PII detection ──
COLUMN_PATTERNS = {
    'person': [
        r'(?i)(姓名|名字|员工姓名|联系人|负责人|name|employee.?name|full.?name|'
        r'first.?name|last.?name|人员|经办人|制表人|审批人|申请人|收件人)',
    ],
    'id_card': [
        r'(?i)(身份证|证件号|id.?card|identity|身份证号|证件号码|id.?number)',
    ],
    'phone': [
        r'(?i)(手机|电话|联系方式|phone|mobile|tel|联系电话|手机号)',
    ],
    'email': [
        r'(?i)(邮箱|邮件|email|e.?mail|电子邮箱)',
    ],
    'bank_card': [
        r'(?i)(银行卡|卡号|bank.?card|银行账号|工资卡|账号|bank.?account)',
    ],
    'company': [
        r'(?i)(公司|企业|单位|employer|company|机构|组织|供应商|客户名称)',
    ],
}

# ── Value validation patterns ──
VALUE_VALIDATORS = {
    'id_card': re.compile(r'^[1-9]\d{5}(19|20)\d{2}(0[1-9]|1[0-2])(0[1-9]|[12]\d|3[01])\d{3}[\dXx]$|^\d{15}$'),
    'phone': re.compile(r'^1[3-9]\d{9}$'),
    'email': re.compile(r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$'),
    'bank_card': re.compile(r'^[3-6]\d{15,18}$'),
}

def smart_read(path, fmt):
    """Read file based on format."""
    if fmt == 'csv':
        for enc in ['utf-8', 'utf-8-sig', 'gbk', 'gb2312', 'latin-1']:
            try:
                return pd.read_csv(path, encoding=enc)
            except (UnicodeDecodeError, UnicodeError):
                continue
        return pd.read_csv(path, encoding='latin-1', errors='replace')
    elif fmt == 'excel':
        return pd.read_excel(path)
    elif fmt == 'json':
        try:
            return pd.read_json(path)
        except ValueError:
            return pd.read_json(path, lines=True)
    elif fmt == 'parquet':
        return pd.read_parquet(path)
    else:
        raise ValueError(f"Unsupported format: {fmt}")

def detect_pii_columns(df):
    """Detect PII columns by matching column names + validating sample values."""
    detected = {}

    for col in df.columns:
        col_str = str(col).strip()
        for category, patterns in COLUMN_PATTERNS.items():
            matched = False
            for pat in patterns:
                if re.search(pat, col_str):
                    matched = True
                    break
            if not matched:
                continue

            # For categories with value validators, sample first 10 non-null rows
            # and require >= 30% match rate to confirm
            if category in VALUE_VALIDATORS:
                validator = VALUE_VALIDATORS[category]
                sample = df[col].dropna().head(10).astype(str)
                if len(sample) == 0:
                    continue
                match_count = sum(1 for v in sample if validator.match(v.strip()))
                if match_count / len(sample) < 0.3:
                    continue

            detected[col_str] = category
            break  # one category per column

    return detected

def mask_column(series, category, mapping, reverse_mapping, counters):
    """Replace unique values in a series with placeholders."""
    tag = category.upper()
    def get_placeholder(val):
        val_str = str(val).strip()
        if val_str in ('', 'nan', 'None', 'NaT'):
            return val
        key = f"{tag}:{val_str}"
        if key in mapping:
            return mapping[key]
        counters[tag] = counters.get(tag, 0) + 1
        placeholder = f"[{tag}_{counters[tag]}]"
        mapping[key] = placeholder
        reverse_mapping[placeholder] = val_str
        return placeholder
    return series.map(get_placeholder)

try:
    df = smart_read(INPUT_PATH, FILE_FORMAT)

    detected = detect_pii_columns(df)
    if not detected:
        # No PII columns found
        print(json.dumps({"masked": False}))
        sys.exit(0)

    mapping = {}
    reverse_mapping = {}
    counters = {}
    masked_columns = {}

    for col, category in detected.items():
        if col in df.columns:
            df[col] = mask_column(df[col], category, mapping, reverse_mapping, counters)
            masked_columns[col] = category

    # Write masked CSV
    os.makedirs(os.path.dirname(OUTPUT_PATH), exist_ok=True)
    df.to_csv(OUTPUT_PATH, index=False, encoding='utf-8-sig')

    result = {
        "masked": True,
        "maskedColumns": masked_columns,
        "mapping": reverse_mapping,
        "maskedPath": OUTPUT_PATH,
        "originalRowCount": len(df),
    }
    print(json.dumps(result, ensure_ascii=False, default=str))

except Exception as e:
    print(json.dumps({"error": str(e)}), file=sys.stderr)
    sys.exit(1)
"#;

// ---------------------------------------------------------------------------
// Data structures for PII masking results
// ---------------------------------------------------------------------------

/// Result from tabular file PII masking.
struct FileMaskResult {
    /// Path to the masked CSV file.
    masked_path: PathBuf,
    /// Mapping from placeholder to original value, e.g. {"[PERSON_1]": "张三"}.
    mapping: HashMap<String, String>,
    /// Which columns were masked, e.g. {"姓名": "person", "身份证号": "id_card"}.
    masked_columns: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// PII masking functions
// ---------------------------------------------------------------------------

/// Run the Python PII masking script on a tabular file.
///
/// Returns `Some(FileMaskResult)` if PII was detected and masked,
/// `None` if no PII found or if the script fails (fail-open).
async fn mask_tabular_file(
    runner: &PythonRunner,
    original_path: &Path,
    format: &parser::FileFormat,
    workspace_path: &Path,
) -> Option<FileMaskResult> {
    let format_str = match format {
        parser::FileFormat::Csv => "csv",
        parser::FileFormat::Excel => "excel",
        parser::FileFormat::Json => "json",
        parser::FileFormat::Parquet => "parquet",
        _ => return None,
    };

    // Build output path: {workspace}/uploads/masked/{stem}_{uuid_short}_masked.csv
    let stem = original_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let uuid_short = uuid::Uuid::new_v4().to_string();
    let uuid_short = uuid_short.split('-').next().unwrap_or("x");
    let masked_dir = workspace_path.join("uploads").join("masked");
    let output_path = masked_dir.join(format!("{}_{}_masked.csv", stem, uuid_short));

    let escaped_input = crate::llm::tool_executor::py_escape(&original_path.to_string_lossy());
    let escaped_output = crate::llm::tool_executor::py_escape(&output_path.to_string_lossy());

    let script = PII_MASK_SCRIPT
        .replace("{input_path}", &escaped_input)
        .replace("{output_path}", &escaped_output)
        .replace("{file_format}", format_str);

    let exec_result = match runner.execute(&script).await {
        Ok(r) => r,
        Err(e) => {
            warn!("[PII] Tabular masking script failed to execute: {}", e);
            return None;
        }
    };

    if exec_result.exit_code != 0 {
        warn!(
            "[PII] Tabular masking script exited with code {}: {}",
            exec_result.exit_code,
            if exec_result.stderr.is_empty() { &exec_result.stdout } else { &exec_result.stderr }
        );
        return None;
    }

    let output: Value = match serde_json::from_str(&exec_result.stdout) {
        Ok(v) => v,
        Err(e) => {
            warn!("[PII] Failed to parse masking script output: {} (stdout: {})", e, exec_result.stdout);
            return None;
        }
    };

    if output.get("masked").and_then(|v| v.as_bool()) != Some(true) {
        info!("[PII] No PII columns detected in {}", original_path.display());
        return None;
    }

    let mapping: HashMap<String, String> = output
        .get("mapping")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let masked_columns: HashMap<String, String> = output
        .get("maskedColumns")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    info!(
        "[PII] Masked {} columns in {}: {:?}",
        masked_columns.len(),
        original_path.display(),
        masked_columns.keys().collect::<Vec<_>>()
    );

    Some(FileMaskResult {
        masked_path: output_path,
        mapping,
        masked_columns,
    })
}

/// Mask text content (PDF/Word) using Rust masking engine.
///
/// Returns `Some((masked_text, mapping))` if PII was detected,
/// `None` if masking produced identical text (no PII).
fn mask_text_content(text: &str) -> Option<(String, HashMap<String, String>)> {
    let mut masking_ctx = MaskingContext::new(MaskingLevel::Strict);
    let masked = masking_ctx.mask_text(text);

    if masked == text {
        return None;
    }

    // Build placeholder → original mapping from the masking context
    let mapping: HashMap<String, String> = masking_ctx
        .mask_map()
        .iter()
        .map(|(original, placeholder)| (placeholder.clone(), original.clone()))
        .collect();

    Some((masked, mapping))
}

/// Extract full text from a PDF/Word file via Python.
async fn extract_full_text(
    runner: &PythonRunner,
    file_path: &Path,
    format: &parser::FileFormat,
) -> Result<String> {
    let escaped_path = crate::llm::tool_executor::py_escape(&file_path.to_string_lossy());

    let script = match format {
        parser::FileFormat::Pdf => format!(
            r#"
import sys
try:
    import pdfplumber
    text_parts = []
    with pdfplumber.open('{path}') as pdf:
        for page in pdf.pages:
            t = page.extract_text()
            if t:
                text_parts.append(t)
    print('\n'.join(text_parts))
except Exception as e:
    print(str(e), file=sys.stderr)
    sys.exit(1)
"#,
            path = escaped_path
        ),
        parser::FileFormat::Word => format!(
            r#"
import sys
try:
    from docx import Document
    doc = Document('{path}')
    text_parts = [p.text for p in doc.paragraphs if p.text.strip()]
    print('\n'.join(text_parts))
except Exception as e:
    print(str(e), file=sys.stderr)
    sys.exit(1)
"#,
            path = escaped_path
        ),
        _ => return Err(anyhow!("extract_full_text: unsupported format")),
    };

    let exec_result = runner.execute(&script).await?;
    if exec_result.exit_code != 0 {
        return Err(anyhow!(
            "Text extraction failed: {}",
            if exec_result.stderr.is_empty() { &exec_result.stdout } else { &exec_result.stderr }
        ));
    }

    Ok(exec_result.stdout)
}

/// Store PII mapping for a file in the memory DB.
fn store_pii_mapping(
    storage: &std::sync::Arc<crate::storage::file_store::AppStorage>,
    conversation_id: &str,
    file_id: &str,
    mapping: &HashMap<String, String>,
) -> Result<()> {
    if mapping.is_empty() {
        return Ok(());
    }
    let key = format!("pii_mapping:{}:{}", conversation_id, file_id);
    let value = serde_json::to_string(mapping)?;
    storage.set_memory(&key, &value, Some("pii_mask"))?;
    info!("[PII] Stored mapping for {}:{} ({} entries)", conversation_id, file_id, mapping.len());
    Ok(())
}

/// Retrieve aggregated PII unmask mapping for a conversation.
///
/// Collects all `pii_mapping:{conversation_id}:*` entries and returns
/// a combined `placeholder → original` HashMap.
pub(crate) fn get_pii_unmask_map(
    storage: &std::sync::Arc<crate::storage::file_store::AppStorage>,
    conversation_id: &str,
) -> HashMap<String, String> {
    let prefix = format!("pii_mapping:{}:", conversation_id);
    let entries = match storage.get_memories_by_prefix(&prefix) {
        Ok(e) => e,
        Err(e) => {
            warn!("[PII] Failed to query pii_mapping: {}", e);
            return HashMap::new();
        }
    };

    let mut combined = HashMap::new();
    for (_key, value) in entries {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&value) {
            combined.extend(map);
        }
    }

    combined
}

/// Apply PII unmask mapping to a string: replace all placeholders with original values.
pub(crate) fn unmask_text(text: &str, unmask_map: &HashMap<String, String>) -> String {
    if unmask_map.is_empty() {
        return text.to_string();
    }

    let mut result = text.to_string();
    // Replace longest placeholders first to avoid partial matches
    let mut pairs: Vec<(&String, &String)> = unmask_map.iter().collect();
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (placeholder, original) in pairs {
        result = result.replace(placeholder.as_str(), original.as_str());
    }

    result
}

// ---------------------------------------------------------------------------
// Main handler
// ---------------------------------------------------------------------------

/// 3. load_file — load an uploaded file into a variable for execute_python.
///
/// Resolves file_id → absolute path, parses the file to get metadata,
/// applies PII masking, and stores the mapping in DB so execute_python
/// can auto-inject `_df`/`_text` using the masked version.
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
            let mut output = json!({
                "status": "loaded",
                "fileId": file_id,
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
            // Include PII masking info if present in cache
            if let Some(cols) = cached_info.get("piiMaskedColumns") {
                output["piiMaskedColumns"] = cols.clone();
                output["piiNotice"] = json!("以下列已自动脱敏，分析基于脱敏数据进行");
            }
            if cached_info.get("piiMasked").and_then(|v| v.as_bool()) == Some(true) {
                output["piiMasked"] = json!(true);
            }
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
    let mut parse_result = parser::parse_file(&runner, &full_path).await?;

    // Determine actual loaded_as based on parse result
    // Word/PPT must always be "text" — _smart_read_data (used in preamble) only
    // supports Excel/CSV/JSON/Parquet, not .docx/.pptx binary formats.
    let actual_loaded_as = if matches!(format, parser::FileFormat::Word | parser::FileFormat::Ppt) {
        "text"
    } else if loaded_as == "auto" {
        if !parse_result.column_names.is_empty() {
            "dataframe"
        } else {
            "text"
        }
    } else {
        loaded_as
    };

    // 4.5 PII masking — apply before storing the loaded file mapping
    let mut pii_masked = false;
    let mut pii_masked_columns: Option<HashMap<String, String>> = None;
    let mut effective_path = full_path.clone();

    if actual_loaded_as == "dataframe" {
        // Tabular data: use Python masking script
        if let Some(mask_result) = mask_tabular_file(&runner, &full_path, &format, &ctx.workspace_path).await {
            // Re-parse the masked file to get updated sample_data
            match parser::parse_file(&runner, &mask_result.masked_path).await {
                Ok(masked_parse) => {
                    parse_result = masked_parse;
                    effective_path = mask_result.masked_path;
                    pii_masked = true;
                    pii_masked_columns = Some(mask_result.masked_columns);
                    // Store PII mapping for later unmask
                    if let Err(e) = store_pii_mapping(&ctx.storage, &ctx.conversation_id, file_id, &mask_result.mapping) {
                        warn!("[PII] Failed to store mapping: {}", e);
                    }
                }
                Err(e) => {
                    warn!("[PII] Failed to re-parse masked file, using original: {}", e);
                    // Clean up the masked file
                    let _ = std::fs::remove_file(&mask_result.masked_path);
                }
            }
        }
    } else if actual_loaded_as == "text" {
        // Text data (PDF/Word): extract full text, mask with Rust, save masked file
        let is_extractable = matches!(format, parser::FileFormat::Pdf | parser::FileFormat::Word);
        if is_extractable {
            match extract_full_text(&runner, &full_path, &format).await {
                Ok(full_text) => {
                    if let Some((masked_text, mapping)) = mask_text_content(&full_text) {
                        // Save masked text to file
                        let stem = full_path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
                        let masked_dir = ctx.workspace_path.join("uploads").join("masked");
                        let _ = std::fs::create_dir_all(&masked_dir);
                        let masked_path = masked_dir.join(format!("{}_masked.txt", stem));

                        match std::fs::write(&masked_path, &masked_text) {
                            Ok(()) => {
                                effective_path = masked_path;
                                pii_masked = true;
                                // Update sample_data preview with masked text
                                let preview = if masked_text.len() > 2000 {
                                    format!("{}...", &masked_text[..2000])
                                } else {
                                    masked_text
                                };
                                parse_result.sample_data = json!({"preview": preview});
                                // Store mapping
                                if let Err(e) = store_pii_mapping(&ctx.storage, &ctx.conversation_id, file_id, &mapping) {
                                    warn!("[PII] Failed to store text mapping: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("[PII] Failed to write masked text file: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("[PII] Failed to extract full text for masking: {}", e);
                }
            }
        }
    }

    // 5. Store the loaded file mapping in DB for execute_python to read
    //    fileId is stored so the preamble builder can use it as the dict key
    //    (avoids collision when two files have the same original name).
    //    path points to the masked file if PII masking was applied.
    let loaded_key = format!("loaded:{}:{}", ctx.conversation_id, file_id);
    let mut loaded_info = json!({
        "path": effective_path.to_string_lossy(),
        "format": format_str,
        "originalName": original_name,
        "fileId": file_id,
        "loadedAs": actual_loaded_as,
        "sheet": sheet,
        "nrows": nrows,
        "piiMasked": pii_masked,
    });
    if let Some(ref cols) = pii_masked_columns {
        loaded_info["piiMaskedColumns"] = json!(cols);
    }
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
                            warn!("[TOOL:load_file] Failed to remove snapshot {}: {}", name, err);
                        } else {
                            info!("[TOOL:load_file] Cleared snapshot {} (new file loaded)", name);
                        }
                    }
                }
            }
        }
    }

    info!("[TOOL:load_file] Stored loaded file mapping: {} -> {} ({}{})",
        file_id, effective_path.display(), actual_loaded_as,
        if pii_masked { ", PII masked" } else { "" });

    // 6. Return metadata to LLM (no file path exposed)
    let row_count_note = if actual_loaded_as == "dataframe" {
        format!("共 {} 行数据，全部已加载到 _df 中（sampleData 仅展示前几行样本，分析请使用 _df 全量数据）", parse_result.row_count)
    } else {
        "文本已完整加载到 _text 中".to_string()
    };

    let mut output = json!({
        "status": "loaded",
        "fileId": file_id,
        "originalName": original_name,
        "format": format_str,
        "loadedAs": actual_loaded_as,
        "columns": parse_result.column_names,
        "rowCount": parse_result.row_count,
        "schemaSummary": parse_result.schema_summary,
        "sampleData": parse_result.sample_data,
        "dataNote": row_count_note,
        "usage": if actual_loaded_as == "dataframe" {
            "数据已加载到变量 _df，可在 execute_python 中直接使用 _df 进行分析。_df 包含完整数据，请基于全量数据分析，聊天中展示摘要，完整明细用 _export_detail 导出"
        } else {
            "文本已加载到变量 _text，可在 execute_python 中直接使用 _text 获取内容"
        },
    });

    if pii_masked {
        output["piiMasked"] = json!(true);
        if let Some(ref cols) = pii_masked_columns {
            output["piiMaskedColumns"] = json!(cols);
            output["piiNotice"] = json!("以下列已自动脱敏，分析基于脱敏数据进行");
        }
    }

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

/// Build the analysis preamble that injects `_ANALYSIS_DIR`, `_CONV_ID`,
/// snapshot restoration, and analysis utility functions into a Python session.
///
/// This is the same setup that `handle_execute_python` performs before user code,
/// extracted as a shared helper so the precompute path in `chat.rs` can reuse it.
///
/// Also ensures `_analysis_utils.py` is written to disk before returning.
pub(crate) fn build_analysis_preamble(
    conversation_id: &str,
    step: u32,
    workspace_path: &std::path::Path,
) -> String {
    use super::util::py_escape;

    // Write ANALYSIS_UTILS to module file. Always overwrite to ensure
    // the on-disk version matches the compiled binary (handles upgrades).
    let utils_path = workspace_path.join("temp/_analysis_utils.py");
    {
        if let Some(parent) = utils_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Err(e) = std::fs::write(&utils_path, crate::python::analysis_utils::ANALYSIS_UTILS) {
            log::warn!("[build_analysis_preamble] Failed to write _analysis_utils.py: {}", e);
        }
    }

    format!(
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
"#,
        conv_id = py_escape(conversation_id),
        step = step,
    )
}
