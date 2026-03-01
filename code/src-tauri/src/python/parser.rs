//! File parser dispatch — route to correct Python script based on file type.
//!
//! Detects the file format and generates a Python script that uses
//! pandas/openpyxl to parse the file and output structured JSON.

use std::path::Path;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::runner::PythonRunner;

/// Supported file formats for parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileFormat {
    Csv,
    Excel,
    Json,
    Parquet,
    Pdf,
    Word,
    Ppt,
    Text,
    Unknown,
}

/// Result of parsing a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    pub format: FileFormat,
    pub column_names: Vec<String>,
    pub row_count: u64,
    pub sample_data: serde_json::Value,
    pub schema_summary: String,
}

/// Detect the file format from its extension.
pub fn detect_format(file_path: &Path) -> FileFormat {
    match file_path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("csv") | Some("tsv") => FileFormat::Csv,
        Some("xlsx") | Some("xls") => FileFormat::Excel,
        Some("json") | Some("jsonl") => FileFormat::Json,
        Some("parquet") => FileFormat::Parquet,
        Some("pdf") => FileFormat::Pdf,
        Some("docx") | Some("doc") => FileFormat::Word,
        Some("pptx") | Some("ppt") => FileFormat::Ppt,
        Some("txt") | Some("log") => FileFormat::Text,
        _ => FileFormat::Unknown,
    }
}

/// Parse a file and return structured metadata + sample data.
///
/// Generates a Python script that reads the file using pandas,
/// extracts column names, row count, and first N rows as sample data,
/// then outputs the result as JSON to stdout.
pub async fn parse_file(runner: &PythonRunner, file_path: &Path) -> Result<ParseResult> {
    let format = detect_format(file_path);
    let file_path_str = file_path.to_string_lossy();

    let read_code = match format {
        FileFormat::Csv => format!(
            r#"df = smart_read_csv(r'{}', nrows=10000)"#,
            file_path_str
        ),
        FileFormat::Excel => format!(
            r#"df = pd.read_excel(r'{}', nrows=10000)"#,
            file_path_str
        ),
        FileFormat::Json => format!(
            r#"
try:
    df = pd.read_json(r'{}')
except ValueError:
    df = pd.read_json(r'{}', lines=True)
"#,
            file_path_str, file_path_str
        ),
        FileFormat::Parquet => format!(
            r#"df = pd.read_parquet(r'{}')"#,
            file_path_str
        ),
        FileFormat::Text => {
            return Ok(ParseResult {
                format: FileFormat::Text,
                column_names: vec![],
                row_count: 0,
                sample_data: serde_json::Value::Null,
                schema_summary: "Plain text file".to_string(),
            });
        }
        FileFormat::Pdf => {
            // PDF parsing uses pdfplumber for tables, falls back to text extraction
            return parse_pdf(runner, file_path).await;
        }
        FileFormat::Word => {
            // Word document parsing uses python-docx
            return parse_word(runner, file_path).await;
        }
        FileFormat::Ppt => {
            // PowerPoint parsing uses python-pptx
            return parse_ppt(runner, file_path).await;
        }
        FileFormat::Unknown => {
            return Err(anyhow!("Unsupported file format: {}", file_path_str));
        }
    };

    let code = format!(
        r#"
import pandas as pd
import json
import sys

def smart_read_csv(path, **kwargs):
    """Read CSV with encoding auto-detection (UTF-8 → GBK → latin-1)."""
    for enc in ['utf-8', 'utf-8-sig', 'gbk', 'gb2312', 'latin-1']:
        try:
            return pd.read_csv(path, encoding=enc, **kwargs)
        except (UnicodeDecodeError, UnicodeError):
            continue
    return pd.read_csv(path, encoding='latin-1', errors='replace', **kwargs)

try:
    {read_code}

    # Gather metadata
    columns = df.columns.tolist()
    dtypes = {{col: str(dtype) for col, dtype in df.dtypes.items()}}
    row_count = len(df)

    # Sample first 5 rows
    sample = df.head(5).to_dict(orient='records')

    # Convert non-serializable types
    for row in sample:
        for key, val in row.items():
            if pd.isna(val):
                row[key] = None
            elif hasattr(val, 'isoformat'):
                row[key] = val.isoformat()

    # Schema summary
    schema_lines = []
    for col in columns:
        schema_lines.append(f"  {{col}}: {{dtypes[col]}}")
    schema_summary = f"{{row_count}} rows, {{len(columns)}} columns:\n" + "\n".join(schema_lines)

    result = {{
        "columnNames": columns,
        "rowCount": row_count,
        "sampleData": sample,
        "schemaSummary": schema_summary,
    }}

    print(json.dumps(result, ensure_ascii=False, default=str))
except Exception as e:
    print(json.dumps({{"error": str(e)}}), file=sys.stderr)
    sys.exit(1)
"#,
        read_code = read_code.trim()
    );

    let exec_result = runner.execute(&code).await?;

    if exec_result.exit_code != 0 {
        return Err(anyhow!(
            "File parsing failed: {}",
            if exec_result.stderr.is_empty() { &exec_result.stdout } else { &exec_result.stderr }
        ));
    }

    // Parse the JSON output
    let output: serde_json::Value = serde_json::from_str(&exec_result.stdout)
        .map_err(|e| anyhow!("Failed to parse Python output: {} (output: {})", e, exec_result.stdout))?;

    if let Some(error) = output.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("Parser error: {}", error));
    }

    Ok(ParseResult {
        format,
        column_names: output.get("columnNames")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        row_count: output.get("rowCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        sample_data: output.get("sampleData").cloned().unwrap_or(serde_json::Value::Null),
        schema_summary: output.get("schemaSummary")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string(),
    })
}

/// Parse a PDF file, extracting tables (via pdfplumber) or plain text.
///
/// Strategy:
/// 1. Try pdfplumber to extract tables → if found, return as structured data
/// 2. Fall back to pdfplumber text extraction → return as Text format
/// 3. If pdfplumber not installed, try PyPDF2/pypdf as last resort
async fn parse_pdf(runner: &PythonRunner, file_path: &Path) -> Result<ParseResult> {
    let file_path_str = file_path.to_string_lossy();

    let code = format!(
        r#"
import json
import sys

file_path = r'{file_path}'

def try_pdfplumber():
    import pdfplumber
    pdf = pdfplumber.open(file_path)
    pages = pdf.pages
    total_pages = len(pages)

    # Try to extract tables from all pages
    all_rows = []
    header = None
    for page in pages:
        tables = page.extract_tables()
        for table in tables:
            for i, row in enumerate(table):
                if header is None and row and any(cell for cell in row):
                    header = [str(c).strip() if c else f"col_{{len(all_rows)+i}}" for c in row]
                elif header is not None:
                    all_rows.append(row)

    if header and all_rows:
        # Has structured table data
        import pandas as pd
        df = pd.DataFrame(all_rows, columns=header)
        # Clean up: strip whitespace, replace empty strings with None
        for col in df.columns:
            if df[col].dtype == object:
                df[col] = df[col].str.strip()
                df[col] = df[col].replace('', None)

        columns = df.columns.tolist()
        dtypes = {{col: str(dtype) for col, dtype in df.dtypes.items()}}
        row_count = len(df)
        sample = df.head(5).to_dict(orient='records')
        for row in sample:
            for key, val in row.items():
                if hasattr(val, 'isoformat'):
                    row[key] = val.isoformat()
                elif val != val:  # NaN check
                    row[key] = None

        schema_lines = [f"  {{col}}: {{dtypes[col]}}" for col in columns]
        schema_summary = f"{{row_count}} rows, {{len(columns)}} columns (extracted from {{total_pages}} PDF pages):\n" + "\n".join(schema_lines)

        print(json.dumps({{
            "type": "table",
            "columnNames": columns,
            "rowCount": row_count,
            "sampleData": sample,
            "schemaSummary": schema_summary,
            "totalPages": total_pages,
        }}, ensure_ascii=False, default=str))
        return True

    # No tables found, extract text
    text_parts = []
    for page in pages:
        text = page.extract_text()
        if text:
            text_parts.append(text.strip())
    pdf.close()

    if text_parts:
        full_text = "\n\n".join(text_parts)
        # Truncate for preview
        preview = full_text[:3000]
        if len(full_text) > 3000:
            preview += "\n... (truncated)"
        print(json.dumps({{
            "type": "text",
            "totalPages": total_pages,
            "textLength": len(full_text),
            "preview": preview,
        }}, ensure_ascii=False))
        return True

    return False

def try_pypdf():
    try:
        from pypdf import PdfReader
    except ImportError:
        from PyPDF2 import PdfReader
    reader = PdfReader(file_path)
    total_pages = len(reader.pages)
    text_parts = []
    for page in reader.pages:
        text = page.extract_text()
        if text:
            text_parts.append(text.strip())

    if text_parts:
        full_text = "\n\n".join(text_parts)
        preview = full_text[:3000]
        if len(full_text) > 3000:
            preview += "\n... (truncated)"
        print(json.dumps({{
            "type": "text",
            "totalPages": total_pages,
            "textLength": len(full_text),
            "preview": preview,
        }}, ensure_ascii=False))
        return True
    return False

try:
    success = False
    try:
        success = try_pdfplumber()
    except ImportError:
        pass

    if not success:
        try:
            success = try_pypdf()
        except ImportError:
            pass

    if not success:
        print(json.dumps({{"error": "no_pdf_library", "message": "请安装 pdfplumber: pip install pdfplumber"}}))
        sys.exit(1)

except Exception as e:
    print(json.dumps({{"error": str(e)}}), file=sys.stderr)
    sys.exit(1)
"#,
        file_path = file_path_str,
    );

    let exec_result = runner.execute(&code).await?;

    if exec_result.exit_code != 0 {
        let err_msg = if exec_result.stderr.contains("no_pdf_library") || exec_result.stdout.contains("no_pdf_library") {
            "PDF 解析需要 pdfplumber 库，请运行: pip install pdfplumber".to_string()
        } else {
            let raw = if exec_result.stderr.is_empty() { &exec_result.stdout } else { &exec_result.stderr };
            format!("PDF 解析失败: {}", raw)
        };
        return Err(anyhow!(err_msg));
    }

    let output: serde_json::Value = serde_json::from_str(&exec_result.stdout)
        .map_err(|e| anyhow!("Failed to parse PDF output: {} (output: {})", e, exec_result.stdout))?;

    if let Some(error) = output.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("PDF 解析失败: {}", error));
    }

    let result_type = output.get("type").and_then(|v| v.as_str()).unwrap_or("text");

    if result_type == "table" {
        // PDF with extracted tables → structured data
        Ok(ParseResult {
            format: FileFormat::Pdf,
            column_names: output.get("columnNames")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            row_count: output.get("rowCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            sample_data: output.get("sampleData").cloned().unwrap_or(serde_json::Value::Null),
            schema_summary: output.get("schemaSummary")
                .and_then(|v| v.as_str())
                .unwrap_or("PDF with tables")
                .to_string(),
        })
    } else {
        // PDF with text only
        let total_pages = output.get("totalPages").and_then(|v| v.as_u64()).unwrap_or(0);
        let text_length = output.get("textLength").and_then(|v| v.as_u64()).unwrap_or(0);
        let preview = output.get("preview").and_then(|v| v.as_str()).unwrap_or("");

        Ok(ParseResult {
            format: FileFormat::Pdf,
            column_names: vec![],
            row_count: 0,
            sample_data: serde_json::json!({ "preview": preview }),
            schema_summary: format!(
                "PDF document, {} pages, ~{} characters of text content",
                total_pages, text_length
            ),
        })
    }
}

/// Parse a Word document (.docx), extracting tables and/or text content.
///
/// Uses python-docx to read the document. If tables are found, returns
/// structured data. Otherwise returns text content.
async fn parse_word(runner: &PythonRunner, file_path: &Path) -> Result<ParseResult> {
    let file_path_str = file_path.to_string_lossy();

    let code = format!(
        r#"
import json
import sys

file_path = r'{file_path}'

try:
    from docx import Document
except ImportError:
    print(json.dumps({{"error": "no_docx_library", "message": "请安装 python-docx: pip install python-docx"}}))
    sys.exit(1)

try:
    doc = Document(file_path)

    # Try to extract tables first
    all_rows = []
    header = None
    for table in doc.tables:
        for i, row in enumerate(table.rows):
            cells = [cell.text.strip() for cell in row.cells]
            if header is None and any(cells):
                header = cells
            elif header is not None:
                all_rows.append(cells)

    if header and all_rows:
        import pandas as pd
        df = pd.DataFrame(all_rows, columns=header)
        for col in df.columns:
            if df[col].dtype == object:
                df[col] = df[col].str.strip()
                df[col] = df[col].replace('', None)

        columns = df.columns.tolist()
        dtypes = {{col: str(dtype) for col, dtype in df.dtypes.items()}}
        row_count = len(df)
        sample = df.head(5).to_dict(orient='records')
        for row in sample:
            for key, val in row.items():
                if val != val:
                    row[key] = None

        schema_lines = [f"  {{col}}: {{dtypes[col]}}" for col in columns]
        schema_summary = f"{{row_count}} rows, {{len(columns)}} columns (extracted from Word document):\n" + "\n".join(schema_lines)

        print(json.dumps({{
            "type": "table",
            "columnNames": columns,
            "rowCount": row_count,
            "sampleData": sample,
            "schemaSummary": schema_summary,
        }}, ensure_ascii=False, default=str))
    else:
        # Extract paragraphs as text
        paragraphs = [p.text for p in doc.paragraphs if p.text.strip()]
        full_text = "\n".join(paragraphs)
        preview = full_text[:3000]
        if len(full_text) > 3000:
            preview += "\n... (truncated)"

        print(json.dumps({{
            "type": "text",
            "paragraphCount": len(paragraphs),
            "textLength": len(full_text),
            "preview": preview,
        }}, ensure_ascii=False))

except Exception as e:
    print(json.dumps({{"error": str(e)}}), file=sys.stderr)
    sys.exit(1)
"#,
        file_path = file_path_str,
    );

    let exec_result = runner.execute(&code).await?;

    if exec_result.exit_code != 0 {
        let err_msg = if exec_result.stderr.contains("no_docx_library") || exec_result.stdout.contains("no_docx_library") {
            "Word 文档解析需要 python-docx 库，请运行: pip install python-docx".to_string()
        } else {
            let raw = if exec_result.stderr.is_empty() {{ &exec_result.stdout }} else {{ &exec_result.stderr }};
            format!("Word 文档解析失败: {}", raw)
        };
        return Err(anyhow!(err_msg));
    }

    let output: serde_json::Value = serde_json::from_str(&exec_result.stdout)
        .map_err(|e| anyhow!("Failed to parse Word output: {} (output: {})", e, exec_result.stdout))?;

    if let Some(error) = output.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("Word 文档解析失败: {}", error));
    }

    let result_type = output.get("type").and_then(|v| v.as_str()).unwrap_or("text");

    if result_type == "table" {
        Ok(ParseResult {
            format: FileFormat::Word,
            column_names: output.get("columnNames")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            row_count: output.get("rowCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            sample_data: output.get("sampleData").cloned().unwrap_or(serde_json::Value::Null),
            schema_summary: output.get("schemaSummary")
                .and_then(|v| v.as_str())
                .unwrap_or("Word document with tables")
                .to_string(),
        })
    } else {
        let paragraphs = output.get("paragraphCount").and_then(|v| v.as_u64()).unwrap_or(0);
        let text_length = output.get("textLength").and_then(|v| v.as_u64()).unwrap_or(0);
        let preview = output.get("preview").and_then(|v| v.as_str()).unwrap_or("");

        Ok(ParseResult {
            format: FileFormat::Word,
            column_names: vec![],
            row_count: 0,
            sample_data: serde_json::json!({ "preview": preview }),
            schema_summary: format!(
                "Word document, {} paragraphs, ~{} characters",
                paragraphs, text_length
            ),
        })
    }
}

/// Parse a PowerPoint file (.pptx), extracting text and tables from slides.
///
/// Uses python-pptx to read the presentation. Extracts text from shapes
/// and tables from table shapes.
async fn parse_ppt(runner: &PythonRunner, file_path: &Path) -> Result<ParseResult> {
    let file_path_str = file_path.to_string_lossy();

    let code = format!(
        r#"
import json
import sys

file_path = r'{file_path}'

try:
    from pptx import Presentation
except ImportError:
    print(json.dumps({{"error": "no_pptx_library", "message": "请安装 python-pptx: pip install python-pptx"}}))
    sys.exit(1)

try:
    prs = Presentation(file_path)
    total_slides = len(prs.slides)

    # Extract tables from all slides
    all_rows = []
    header = None
    for slide in prs.slides:
        for shape in slide.shapes:
            if shape.has_table:
                table = shape.table
                for i, row in enumerate(table.rows):
                    cells = [cell.text.strip() for cell in row.cells]
                    if header is None and any(cells):
                        header = cells
                    elif header is not None:
                        all_rows.append(cells)

    if header and all_rows:
        import pandas as pd
        df = pd.DataFrame(all_rows, columns=header)
        for col in df.columns:
            if df[col].dtype == object:
                df[col] = df[col].str.strip()
                df[col] = df[col].replace('', None)

        columns = df.columns.tolist()
        dtypes = {{col: str(dtype) for col, dtype in df.dtypes.items()}}
        row_count = len(df)
        sample = df.head(5).to_dict(orient='records')
        for row in sample:
            for key, val in row.items():
                if val != val:
                    row[key] = None

        schema_lines = [f"  {{col}}: {{dtypes[col]}}" for col in columns]
        schema_summary = f"{{row_count}} rows, {{len(columns)}} columns (extracted from {{total_slides}} slides):\n" + "\n".join(schema_lines)

        print(json.dumps({{
            "type": "table",
            "columnNames": columns,
            "rowCount": row_count,
            "sampleData": sample,
            "schemaSummary": schema_summary,
            "totalSlides": total_slides,
        }}, ensure_ascii=False, default=str))
    else:
        # Extract text from all slides
        text_parts = []
        for idx, slide in enumerate(prs.slides, 1):
            slide_texts = []
            for shape in slide.shapes:
                if shape.has_text_frame:
                    for para in shape.text_frame.paragraphs:
                        text = para.text.strip()
                        if text:
                            slide_texts.append(text)
            if slide_texts:
                text_parts.append(f"[Slide {{idx}}]\n" + "\n".join(slide_texts))

        full_text = "\n\n".join(text_parts)
        preview = full_text[:3000]
        if len(full_text) > 3000:
            preview += "\n... (truncated)"

        print(json.dumps({{
            "type": "text",
            "totalSlides": total_slides,
            "textLength": len(full_text),
            "preview": preview,
        }}, ensure_ascii=False))

except Exception as e:
    print(json.dumps({{"error": str(e)}}), file=sys.stderr)
    sys.exit(1)
"#,
        file_path = file_path_str,
    );

    let exec_result = runner.execute(&code).await?;

    if exec_result.exit_code != 0 {
        let err_msg = if exec_result.stderr.contains("no_pptx_library") || exec_result.stdout.contains("no_pptx_library") {
            "PPT 解析需要 python-pptx 库，请运行: pip install python-pptx".to_string()
        } else {
            let raw = if exec_result.stderr.is_empty() {{ &exec_result.stdout }} else {{ &exec_result.stderr }};
            format!("PPT 解析失败: {}", raw)
        };
        return Err(anyhow!(err_msg));
    }

    let output: serde_json::Value = serde_json::from_str(&exec_result.stdout)
        .map_err(|e| anyhow!("Failed to parse PPT output: {} (output: {})", e, exec_result.stdout))?;

    if let Some(error) = output.get("error").and_then(|v| v.as_str()) {
        return Err(anyhow!("PPT 解析失败: {}", error));
    }

    let result_type = output.get("type").and_then(|v| v.as_str()).unwrap_or("text");

    if result_type == "table" {
        Ok(ParseResult {
            format: FileFormat::Ppt,
            column_names: output.get("columnNames")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            row_count: output.get("rowCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            sample_data: output.get("sampleData").cloned().unwrap_or(serde_json::Value::Null),
            schema_summary: output.get("schemaSummary")
                .and_then(|v| v.as_str())
                .unwrap_or("PPT with tables")
                .to_string(),
        })
    } else {
        let total_slides = output.get("totalSlides").and_then(|v| v.as_u64()).unwrap_or(0);
        let text_length = output.get("textLength").and_then(|v| v.as_u64()).unwrap_or(0);
        let preview = output.get("preview").and_then(|v| v.as_str()).unwrap_or("");

        Ok(ParseResult {
            format: FileFormat::Ppt,
            column_names: vec![],
            row_count: 0,
            sample_data: serde_json::json!({ "preview": preview }),
            schema_summary: format!(
                "PowerPoint presentation, {} slides, ~{} characters",
                total_slides, text_length
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_csv() {
        assert_eq!(detect_format(Path::new("data.csv")), FileFormat::Csv);
        assert_eq!(detect_format(Path::new("DATA.CSV")), FileFormat::Csv);
    }

    #[test]
    fn test_detect_excel() {
        assert_eq!(detect_format(Path::new("data.xlsx")), FileFormat::Excel);
        assert_eq!(detect_format(Path::new("data.xls")), FileFormat::Excel);
    }

    #[test]
    fn test_detect_json() {
        assert_eq!(detect_format(Path::new("data.json")), FileFormat::Json);
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(detect_format(Path::new("data.zip")), FileFormat::Unknown);
    }

    #[test]
    fn test_parse_result_serialization() {
        let result = ParseResult {
            format: FileFormat::Csv,
            column_names: vec!["name".to_string(), "salary".to_string()],
            row_count: 100,
            sample_data: serde_json::json!([]),
            schema_summary: "100 rows, 2 columns".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"format\":\"csv\""));
        assert!(json.contains("\"rowCount\":100"));
    }
}
