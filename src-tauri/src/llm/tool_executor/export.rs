//! export_data handler.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::FileMeta;
use crate::python::runner::PythonRunner;

use super::FileGenResult;
use super::file_load::{get_pii_unmask_map, unmask_text};
use super::require_str;
use super::util::{py_escape, indent_python};

/// 9. export_data — write data to CSV, Excel, or JSON.
///
/// Two input modes:
/// - `source_file`: path to an existing file to convert to another format
/// - `data`: pre-computed JSON records to export
pub(crate) async fn handle_export_data(ctx: &PluginContext, args: &Value) -> Result<FileGenResult> {
    let format = require_str(args, "format")?;
    let filename = require_str(args, "filename")?;

    // Ensure the filename has the correct extension.
    let filename = ensure_extension(filename, format);

    // Determine input mode: source_file takes priority over data.
    let source_file = args.get("source_file").and_then(|v| v.as_str());

    // Track temp file for cleanup (only created in JSON data mode).
    let mut data_temp_path: Option<std::path::PathBuf> = None;

    let python_code = if let Some(src_path) = source_file {
        // Mode 1: Read from existing file and convert to target format.
        build_export_python_from_file(src_path, format, &filename)?
    } else {
        // Mode 2: Export JSON data records (deprecated — prefer execute_python + source_file).
        let data_val = args
            .get("data")
            .ok_or_else(|| anyhow!(
                "参数 'source_file' 和 'data' 都未提供。请选择以下方式之一：\n\
                 1. 推荐：在 execute_python 中使用 _export_detail(_df, '{}', format='{}') 直接导出\n\
                 2. 提供 source_file: 已有文件的路径（如 'exports/xxx.xlsx'）\n\
                 3. 提供 data: 实际的 JSON 记录数组",
                filename, format
            ))?;

        log::warn!("[export_data] Using deprecated 'data' parameter. Recommend using execute_python with _export_detail() or source_file instead.");

        // Validate: reject null data
        if data_val.is_null() {
            return Err(anyhow!(
                "参数 'data' 为 null。请改用以下方式之一：\n\
                 1. 在 execute_python 中使用 _export_detail(_df, '{}', format='{}') 直接导出\n\
                 2. 使用 source_file 参数指定已有文件路径进行格式转换\n\
                 3. 传入实际的 JSON 记录数组，如 [{{\"name\":\"A\",\"value\":1}}]",
                filename, format
            ));
        }

        // Validate: reject string data (LLM passes variable names like "_df")
        if let Some(data_str) = data_val.as_str() {
            return Err(anyhow!(
                "参数 'data' 不能是字符串 '{}'。请改用以下方式之一：\n\
                 1. 在 execute_python 中使用 _export_detail(_df, '{}', format='{}') 直接导出\n\
                 2. 使用 source_file 参数指定已有文件路径（如 'exports/xxx.csv'）进行格式转换",
                data_str, filename, format
            ));
        }

        // Validate: reject empty object {}
        if data_val.is_object() {
            let obj = data_val.as_object().unwrap();
            if obj.is_empty() {
                return Err(anyhow!(
                    "参数 'data' 为空对象 {{}}。请改用以下方式之一：\n\
                     1. 在 execute_python 中使用 _export_detail(_df, '{}', format='{}') 直接导出\n\
                     2. 使用 source_file 参数指定已有文件路径进行格式转换\n\
                     3. 传入实际的 JSON 记录数组，如 [{{\"name\":\"A\",\"value\":1}}]",
                    filename, format
                ));
            }
        }

        // Write data JSON to temp file (avoids triple-quote injection in Python source)
        let temp_dir = ctx.workspace_path.join("temp");
        std::fs::create_dir_all(&temp_dir)?;
        let data_temp = temp_dir.join(format!(
            "export_data_{}.json",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        ));
        let data_json = serde_json::to_string(data_val)?;
        std::fs::write(&data_temp, &data_json)?;

        let code = build_export_python_from_json(&data_temp.to_string_lossy(), format, &filename)?;
        data_temp_path = Some(data_temp);
        code
    };

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    // Clean up temp file if Python didn't
    if let Some(ref temp_path) = data_temp_path {
        let _ = std::fs::remove_file(temp_path);
    }

    if result.exit_code != 0 {
        return Err(anyhow!(
            "Export failed:\n{}",
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        ));
    }

    // Record the file in the database.
    let stored_path = format!("exports/{}", filename);
    let full_path = ctx.workspace_path.join(&stored_path);
    if !full_path.exists() {
        return Err(anyhow!("Export failed: file '{}' was not created", stored_path));
    }

    // Unmask PII placeholders in text-based export formats (CSV, JSON)
    let unmask_map = get_pii_unmask_map(&ctx.storage, &ctx.conversation_id);
    if !unmask_map.is_empty() {
        let is_text_format = matches!(format, "csv" | "json" | "tsv");
        if is_text_format {
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let unmasked = unmask_text(&content, &unmask_map);
                if unmasked != content {
                    let _ = std::fs::write(&full_path, &unmasked);
                    log::info!("[export_data] Unmasked PII placeholders in {}", filename);
                }
            }
        }
    }

    let file_size = std::fs::metadata(&full_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let file_id = Uuid::new_v4().to_string();
    if let Err(e) = ctx.storage.insert_generated_file(
        &file_id,
        &ctx.conversation_id,
        None,
        &filename,
        &stored_path,
        format,
        file_size as i64,
        "data",
        None,
        1,
        true,
        None,
        None,
        None,
    ) {
        let _ = std::fs::remove_file(&full_path);
        return Err(e.into());
    }

    let content = serde_json::to_string_pretty(&json!({
        "fileId": file_id,
        "fileName": filename,
        "storedPath": stored_path,
        "fileSize": file_size,
        "format": format,
    }))?;

    let file_meta = FileMeta {
        file_id,
        file_name: filename.to_string(),
        requested_format: format.to_string(),
        actual_format: format.to_string(),
        file_size,
        stored_path,
        category: "data".to_string(),
    };

    Ok(FileGenResult {
        content,
        file_meta,
        is_degraded: false,
        degradation_notice: None,
    })
}

/// Build Python code to read from an existing source file and export to target format.
fn build_export_python_from_file(source_path: &str, format: &str, filename: &str) -> Result<String> {
    let escaped_source = py_escape(source_path);
    let escaped_filename = py_escape(filename);

    let write_code = build_write_code(format)?;
    let write_code = indent_python(&write_code, 4);

    Ok(format!(
        r#"
import pandas as pd
import os
import sys

try:
    source = '{source}'
    # Resolve relative paths from workspace root
    if not os.path.isabs(source):
        source = os.path.abspath(source)

    if not os.path.exists(source):
        print(f"Error: source file '{{source}}' not found.", file=sys.stderr)
        sys.exit(1)

    # Read source file using auto-detection
    df = _smart_read_data(source)
    print(f"Read {{len(df)}} rows from {{source}}")

    # Ensure exports directory exists
    os.makedirs('exports', exist_ok=True)
    output_path = os.path.join('exports', '{filename}')

{write_code}
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#,
        source = escaped_source,
        filename = escaped_filename,
        write_code = write_code,
    ))
}

/// Build Python code to export JSON data records to target format.
fn build_export_python_from_json(data_file_path: &str, format: &str, filename: &str) -> Result<String> {
    let escaped_data_path = py_escape(data_file_path);
    let escaped_filename = py_escape(filename);

    let write_code = build_write_code(format)?;
    let write_code = indent_python(&write_code, 4);

    Ok(format!(
        r#"
import pandas as pd
import json
import os
import sys

try:
    with open('{data_path}', 'r', encoding='utf-8') as _f:
        data = json.load(_f)
    os.remove('{data_path}')

    # Validate data type — reject strings and other non-structured types
    if isinstance(data, str):
        print(f"Error: 'data' must be a list of record objects or a dict, not a string. Received: {{data[:100]}}... "
              f"请改用 execute_python 中的 _export_detail(_df, '{filename}', format='{format}') 直接导出。", file=sys.stderr)
        sys.exit(1)

    # Handle various data shapes
    if isinstance(data, list):
        if len(data) == 0:
            print("Error: 'data' is an empty list — nothing to export.", file=sys.stderr)
            sys.exit(1)
        if isinstance(data[0], str):
            print(f"Error: 'data' must be a list of record objects (dicts), not a list of strings. "
              f"请改用 execute_python 中的 _export_detail(_df, '{filename}', format='{format}') 直接导出。", file=sys.stderr)
            sys.exit(1)
        df = pd.DataFrame(data)
    elif isinstance(data, dict):
        if 'columns' in data and 'rows' in data:
            df = pd.DataFrame(data['rows'], columns=data['columns'])
        elif 'records' in data and isinstance(data['records'], list):
            df = pd.DataFrame(data['records'])
        else:
            # Reject arbitrary dicts — they are almost always LLM errors
            # (e.g. {{"to_dict": "records"}} instead of actual data).
            # Valid formats: {{"columns": [...], "rows": [...]}} or {{"records": [...]}}
            print(f"Error: 'data' dict must have either {{'columns', 'rows'}} or {{'records'}} keys. "
                  f"Got keys: {{list(data.keys())}}. "
                  f"请改用 execute_python 中的 _export_detail(_df, '{filename}', format='{format}') 直接导出。",
                  file=sys.stderr)
            sys.exit(1)
    else:
        print(f"Error: 'data' must be a list of records or a dict with 'columns'+'rows'/'records' keys. Got {{type(data).__name__}}.", file=sys.stderr)
        sys.exit(1)

    # Ensure exports directory exists
    os.makedirs('exports', exist_ok=True)
    output_path = os.path.join('exports', '{filename}')

{write_code}
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#,
        data_path = escaped_data_path,
        filename = escaped_filename,
        format = format,
        write_code = write_code,
    ))
}

/// Build the format-specific write code block (shared between both input modes).
fn build_write_code(format: &str) -> Result<String> {
    match format {
        "csv" => Ok(
            r#"df.to_csv(output_path, index=False, encoding='utf-8-sig')
print(f"Exported {len(df)} rows to {output_path}")"#.to_string(),
        ),
        "excel" => Ok(
            r#"df.to_excel(output_path, index=False, engine='openpyxl')
print(f"Exported {len(df)} rows to {output_path}")"#.to_string(),
        ),
        "json" => Ok(
            r#"df.to_json(output_path, orient='records', force_ascii=False, indent=2)
print(f"Exported {len(df)} rows to {output_path}")"#.to_string(),
        ),
        other => Err(anyhow!(
            "Unsupported export format: {}. Supported: csv, excel, json",
            other
        )),
    }
}

/// Ensure a filename has the correct extension for the given format.
fn ensure_extension(filename: &str, format: &str) -> String {
    let expected_ext = match format {
        "csv" => "csv",
        "excel" => "xlsx",
        "json" => "json",
        _ => format,
    };

    if filename.ends_with(&format!(".{}", expected_ext)) {
        filename.to_string()
    } else {
        // Strip any existing extension and add the correct one.
        let base = filename
            .rsplit_once('.')
            .map(|(base, _)| base)
            .unwrap_or(filename);
        format!("{}.{}", base, expected_ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_export_python_from_json tests ──────────────────

    #[test]
    fn test_build_export_python_json_csv() {
        let code = build_export_python_from_json("/tmp/data.json", "csv", "output.csv").unwrap();
        assert!(code.contains("to_csv"));
        assert!(code.contains("output.csv"));
        assert!(code.contains("exports"));
        // Verify temp-file protocol: reads data from file, not inline JSON
        assert!(code.contains("json.load("));
        assert!(code.contains("/tmp/data.json"));
    }

    /// Verify that generated export code has correct Python indentation.
    /// The write_code block must be indented inside the `try:` block —
    /// otherwise Python raises `SyntaxError: expected 'except' or 'finally'`.
    #[test]
    fn test_build_export_python_json_indentation() {
        let code = build_export_python_from_json("/tmp/data.json", "csv", "test.csv").unwrap();

        // The to_csv call and the print() after it must both be indented
        // inside the try: block (4 spaces).
        for line in code.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("df.to_csv") || trimmed.starts_with("print(f\"Exported") {
                assert!(
                    line.starts_with("    "),
                    "Line should be indented inside try block: '{}'", line
                );
            }
        }

        // The except clause must exist at column 0
        assert!(code.contains("\nexcept Exception as e:"),
            "except clause should exist at column 0");
    }

    #[test]
    fn test_build_export_python_json_excel() {
        let code = build_export_python_from_json("/tmp/data.json", "excel", "data.xlsx").unwrap();
        assert!(code.contains("to_excel"));
        assert!(code.contains("openpyxl"));
    }

    #[test]
    fn test_build_export_python_json_json() {
        let code = build_export_python_from_json("/tmp/data.json", "json", "out.json").unwrap();
        assert!(code.contains("to_json"));
        assert!(code.contains("orient='records'"));
    }

    #[test]
    fn test_build_export_python_json_unsupported() {
        let result = build_export_python_from_json("/tmp/data.json", "parquet", "out.parquet");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_export_python_json_validates_string_data() {
        let code = build_export_python_from_json("/tmp/data.json", "csv", "out.csv").unwrap();
        // Generated code should reject string data at runtime
        assert!(code.contains("isinstance(data, str)"), "Should check for string data type");
        assert!(code.contains("isinstance(data[0], str)"), "Should check for list of strings");
    }

    // ── build_export_python_from_file tests ──────────────────

    #[test]
    fn test_build_export_from_file_csv() {
        let code = build_export_python_from_file("exports/step1_data.xlsx", "csv", "output.csv").unwrap();
        assert!(code.contains("_smart_read_data"));
        assert!(code.contains("exports/step1_data.xlsx"));
        assert!(code.contains("to_csv"));
        assert!(code.contains("output.csv"));
    }

    #[test]
    fn test_build_export_from_file_excel() {
        let code = build_export_python_from_file("exports/data.csv", "excel", "data.xlsx").unwrap();
        assert!(code.contains("_smart_read_data"));
        assert!(code.contains("to_excel"));
        assert!(code.contains("openpyxl"));
    }

    #[test]
    fn test_build_export_from_file_json() {
        let code = build_export_python_from_file("exports/data.xlsx", "json", "out.json").unwrap();
        assert!(code.contains("_smart_read_data"));
        assert!(code.contains("to_json"));
        assert!(code.contains("orient='records'"));
    }

    #[test]
    fn test_build_export_from_file_indentation() {
        let code = build_export_python_from_file("exports/data.xlsx", "csv", "test.csv").unwrap();

        for line in code.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("df.to_csv") || trimmed.starts_with("print(f\"Exported") {
                assert!(
                    line.starts_with("    "),
                    "Line should be indented inside try block: '{}'", line
                );
            }
        }

        assert!(code.contains("\nexcept Exception as e:"),
            "except clause should exist at column 0");
    }

    #[test]
    fn test_build_export_from_file_unsupported() {
        let result = build_export_python_from_file("data.xlsx", "parquet", "out.parquet");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_export_from_file_checks_existence() {
        let code = build_export_python_from_file("data.xlsx", "csv", "out.csv").unwrap();
        assert!(code.contains("os.path.exists"), "Should check if source file exists");
    }

    // ── build_write_code tests ──────────────────────────────

    #[test]
    fn test_build_write_code_csv() {
        let code = build_write_code("csv").unwrap();
        assert!(code.contains("to_csv"));
        assert!(code.contains("utf-8-sig"));
    }

    #[test]
    fn test_build_write_code_excel() {
        let code = build_write_code("excel").unwrap();
        assert!(code.contains("to_excel"));
        assert!(code.contains("openpyxl"));
    }

    #[test]
    fn test_build_write_code_json() {
        let code = build_write_code("json").unwrap();
        assert!(code.contains("to_json"));
        assert!(code.contains("orient='records'"));
    }

    #[test]
    fn test_build_write_code_unsupported() {
        assert!(build_write_code("parquet").is_err());
        assert!(build_write_code("xml").is_err());
    }

    // ── ensure_extension tests ───────────────────

    #[test]
    fn test_ensure_extension_already_correct() {
        assert_eq!(ensure_extension("data.csv", "csv"), "data.csv");
        assert_eq!(ensure_extension("report.xlsx", "excel"), "report.xlsx");
    }

    #[test]
    fn test_ensure_extension_wrong_ext() {
        assert_eq!(ensure_extension("data.txt", "csv"), "data.csv");
        assert_eq!(ensure_extension("report.csv", "excel"), "report.xlsx");
    }

    #[test]
    fn test_ensure_extension_no_ext() {
        assert_eq!(ensure_extension("data", "csv"), "data.csv");
        assert_eq!(ensure_extension("report", "json"), "report.json");
    }
}
