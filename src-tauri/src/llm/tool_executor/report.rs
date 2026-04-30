//! generate_report handler and HTML/PDF/DOCX report construction.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::plugin::context::PluginContext;
use crate::python::runner::PythonRunner;
use crate::runtime::store::AuthorizedWorkspaceRef;
use crate::runtime::tools::builtin::report_capability::{
    build_file_gen_result, DefaultReportCapability, ReportCapability, ReportGenOutput,
};

use super::file_load::unmask_text;
use super::optional_str;
use super::util::py_escape;
use super::FileGenResult;

pub(crate) struct ReportCoreParams<'a> {
    pub workspace_path: &'a Path,
    pub authorized_workspace: Option<&'a AuthorizedWorkspaceRef>,
    pub conversation_id: &'a str,
}

/// 4. generate_report — build a structured HTML/PDF/DOCX/Markdown report.
pub(crate) async fn handle_generate_report(
    ctx: &PluginContext,
    args: &Value,
) -> Result<FileGenResult> {
    let resolver = ctx
        .runtime_resolver
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("managed runtime resolver is required for Python tools"))?;
    let deps = resolver.workspace_dependencies()?;
    let python_binary = deps.python;
    let python_home = None;
    let capability = DefaultReportCapability {
        storage: ctx.storage.clone(),
        file_manager: ctx.file_manager.clone(),
        auth_manager: ctx.auth_manager.clone(),
        workspace_path: ctx.workspace_path.clone(),
        python_binary,
        python_home,
    };
    let params = ReportCoreParams {
        workspace_path: &ctx.workspace_path,
        authorized_workspace: ctx.authorized_workspace.as_ref(),
        conversation_id: &ctx.conversation_id,
    };
    handle_generate_report_core(&params, args, &capability).await
}

pub(crate) async fn handle_generate_report_core(
    params: &ReportCoreParams<'_>,
    args: &Value,
    capability: &dyn ReportCapability,
) -> Result<FileGenResult> {
    let sections_value = resolve_report_sections(params, args)?;
    let sections = sections_value.as_slice();
    let format = optional_str(args, "format").unwrap_or("html");
    let title = optional_str(args, "title").unwrap_or_else(|| {
        sections
            .first()
            .and_then(|s| s.get("heading"))
            .and_then(|v| v.as_str())
            .unwrap_or("分析报告")
    });

    let unmask_map = capability.get_pii_unmask_map(params.conversation_id);
    let product_name = capability.get_product_name().await;
    let generated = capability
        .generate_report_bytes(
            params.workspace_path,
            title,
            sections,
            format,
            &unmask_map,
            product_name.as_deref(),
        )
        .await?;
    let persisted = capability
        .persist_file(
            params.conversation_id,
            &generated.bytes,
            &generated.extension,
            title,
            &generated.actual_format,
        )
        .await?;

    let mut result = json!({
        "fileId": persisted.file_id,
        "fileName": persisted.file_name,
        "storedPath": persisted.stored_path,
        "fileSize": persisted.file_size,
        "format": generated.actual_format,
    });

    let degradation_notice = generated.degradation_notice.clone();
    let content = if let Some(notice) = degradation_notice.clone() {
        result["notice"] = json!(&notice);
        format!("{}\n{}", notice, serde_json::to_string_pretty(&result)?)
    } else {
        serde_json::to_string_pretty(&result)?
    };

    Ok(build_file_gen_result(
        content,
        persisted,
        format,
        &generated.actual_format,
        generated.is_degraded,
        degradation_notice,
    ))
}

fn resolve_report_sections(params: &ReportCoreParams<'_>, args: &Value) -> Result<Vec<Value>> {
    if let Some(source_path) = args.get("source").and_then(|v| v.as_str()) {
        let full_path = if Path::new(source_path).is_absolute() {
            PathBuf::from(source_path)
        } else {
            params.workspace_path.join(source_path)
        };
        let canonical = full_path.canonicalize().map_err(|e| {
            anyhow::anyhow!(
                "Failed to read source file '{}': {}. Use execute_python to generate the JSON file first.",
                source_path,
                e
            )
        })?;
        let workspace_canonical = params
            .workspace_path
            .canonicalize()
            .unwrap_or_else(|_| params.workspace_path.to_path_buf());
        let in_workspace = canonical.starts_with(&workspace_canonical);
        let in_authorized = params
            .authorized_workspace
            .map(|aw| canonical.starts_with(&aw.root_path))
            .unwrap_or(false);
        if !in_workspace && !in_authorized {
            anyhow::bail!(
                "Source file path '{}' is outside the workspace directory and outside the authorized workspace. \
                 Only files within the workspace or the authorized local directory are allowed.",
                source_path
            );
        }
        let content = std::fs::read_to_string(&canonical).map_err(|e| {
            anyhow::anyhow!(
                "Failed to read source file '{}': {}. Use execute_python to generate the JSON file first.",
                source_path,
                e
            )
        })?;
        let sections = serde_json::from_str::<Vec<Value>>(&content).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse sections from '{}': {}. The file must contain a JSON array of section objects.",
                source_path,
                e
            )
        })?;
        if sections.is_empty() {
            anyhow::bail!(
                "Source file '{}' contains an empty sections array.",
                source_path
            );
        }
        log::info!(
            "[generate_report] Loaded {} sections from source file: {}",
            sections.len(),
            source_path
        );
        Ok(sections)
    } else if let Some(arr) = args.get("sections").and_then(|v| v.as_array()) {
        if arr.is_empty() {
            anyhow::bail!("'sections' array is empty. At least one section is required.");
        }
        Ok(arr.clone())
    } else if let Some(text) = args.get("sections").and_then(|v| v.as_str()) {
        let sections = serde_json::from_str::<Vec<Value>>(text).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse 'sections' string as JSON array: {}. \
                 The 'sections' parameter must be a valid JSON array of section objects.",
                e
            )
        })?;
        if sections.is_empty() {
            anyhow::bail!("'sections' array is empty. At least one section is required.");
        }
        Ok(sections)
    } else {
        anyhow::bail!(
            "Missing report data. Provide either:\n\
             1. 'sections': array of section objects (preferred)\n\
             2. 'source': path to a JSON file with sections array"
        );
    }
}

pub(crate) async fn generate_report_bytes_core(
    workspace_path: &Path,
    title: &str,
    sections: &[Value],
    format: &str,
    unmask_map: &HashMap<String, String>,
    product_name: Option<&str>,
    python_runtime: Option<(&PathBuf, Option<&PathBuf>)>,
) -> Result<ReportGenOutput> {
    let html_content = build_html_report(title, sections);
    let html_content = match product_name {
        Some(name) => html_content.replace("AI小家", name),
        None => html_content,
    };
    let html_content = unmask_text(&html_content, unmask_map);

    match format {
        "markdown" => {
            let markdown = unmask_text(&build_markdown_report(title, sections), unmask_map);
            Ok(ReportGenOutput {
                bytes: markdown.into_bytes(),
                extension: "md".to_string(),
                actual_format: "markdown".to_string(),
                is_degraded: false,
                degradation_notice: None,
            })
        }
        "pdf" => match convert_sections_to_pdf_with_runtime(
            workspace_path,
            title,
            sections,
            unmask_map,
            python_runtime,
        )
        .await
        {
            Ok(bytes) => Ok(ReportGenOutput {
                bytes,
                extension: "pdf".to_string(),
                actual_format: "pdf".to_string(),
                is_degraded: false,
                degradation_notice: None,
            }),
            Err(e) => {
                log::warn!(
                    "[generate_report] PDF conversion failed: {}. Falling back to HTML.",
                    e
                );
                Ok(ReportGenOutput {
                    bytes: html_content.clone().into_bytes(),
                    extension: "html".to_string(),
                    actual_format: "html_fallback_from_pdf".to_string(),
                    is_degraded: true,
                    degradation_notice: Some(format!(
                        "⚠️ PDF 转换失败，已保存为 HTML 格式。请告知用户实际生成的是 HTML 而非 PDF，可在浏览器中打开后通过 Ctrl/Cmd+P 打印为 PDF。"
                    )),
                })
            }
        },
        "docx" => {
            match convert_html_to_docx_with_runtime(workspace_path, &html_content, python_runtime)
                .await
            {
                Ok(bytes) => Ok(ReportGenOutput {
                    bytes,
                    extension: "docx".to_string(),
                    actual_format: "docx".to_string(),
                    is_degraded: false,
                    degradation_notice: None,
                }),
                Err(e) => {
                    log::warn!(
                        "[generate_report] DOCX conversion failed: {}. Falling back to HTML.",
                        e
                    );
                    Ok(ReportGenOutput {
                    bytes: html_content.into_bytes(),
                    extension: "html".to_string(),
                    actual_format: "html_fallback_from_docx".to_string(),
                    is_degraded: true,
                    degradation_notice: Some(format!(
                        "⚠️ DOCX 转换失败，已保存为 HTML 格式。请告知用户实际生成的是 HTML 而非 DOCX，可在浏览器中打开后通过 Ctrl/Cmd+P 打印为 PDF。"
                    )),
                })
                }
            }
        }
        _ => Ok(ReportGenOutput {
            bytes: html_content.into_bytes(),
            extension: "html".to_string(),
            actual_format: "html".to_string(),
            is_degraded: false,
            degradation_notice: None,
        }),
    }
}

/// Convert structured report sections to PDF using Python reportlab.
///
/// Instead of converting HTML→PDF (which requires C-dependent libraries like
/// weasyprint/pycairo), this builds the PDF directly from the structured JSON
/// data using reportlab (pure Python wheels, no C dependencies).
async fn convert_sections_to_pdf_with_runtime(
    workspace_path: &Path,
    title: &str,
    sections: &[Value],
    unmask_map: &HashMap<String, String>,
    python_runtime: Option<(&PathBuf, Option<&PathBuf>)>,
) -> Result<Vec<u8>> {
    let (python_binary, python_home) =
        python_runtime.ok_or_else(|| anyhow::anyhow!("python runtime unavailable"))?;
    let runner = PythonRunner::with_runtime(
        workspace_path.to_path_buf(),
        crate::python::sandbox::SandboxConfig::for_workspace(&workspace_path.to_path_buf()),
        python_binary.to_path_buf(),
        python_home.cloned(),
    );

    let temp_dir = workspace_path.join("temp");
    std::fs::create_dir_all(&temp_dir)?;

    // Write sections JSON to temp file (avoids string interpolation issues)
    let json_temp = temp_dir.join(format!(
        "sections_{}.json",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));
    let mut report_data = serde_json::json!({
        "title": title,
        "sections": sections,
    });
    // Unmask PII in the JSON payload
    let json_str = serde_json::to_string(&report_data)?;
    let json_str = unmask_text(&json_str, unmask_map);
    report_data = serde_json::from_str(&json_str)?;
    std::fs::write(&json_temp, serde_json::to_string(&report_data)?)?;

    let output_path = temp_dir.join(format!(
        "report_{}.pdf",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));

    let json_temp_str = py_escape(&json_temp.to_string_lossy());
    let output_path_str = py_escape(&output_path.to_string_lossy());

    let python_code = format!(
        r#"
import sys, os, json

json_path = '{json_temp_str}'
output_path = '{output_path_str}'

with open(json_path, 'r', encoding='utf-8') as f:
    data = json.load(f)
os.remove(json_path)

try:
    from reportlab.lib.pagesizes import A4
    from reportlab.lib.units import mm, cm
    from reportlab.lib.styles import getSampleStyleSheet, ParagraphStyle
    from reportlab.lib.colors import HexColor
    from reportlab.lib.enums import TA_LEFT, TA_CENTER
    from reportlab.platypus import (
        SimpleDocTemplate, Paragraph, Spacer, Table, TableStyle,
        HRFlowable, KeepTogether,
    )
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.ttfonts import TTFont
except ImportError as exc:
    print("ERROR:reportlab not installed: " + str(exc))
    sys.exit(1)

# ── Register CJK font ──
# Try platform fonts for Chinese support
font_registered = False
font_name = 'Helvetica'
bold_font = 'Helvetica-Bold'

cjk_candidates = [
    # macOS
    '/System/Library/Fonts/PingFang.ttc',
    '/System/Library/Fonts/STHeiti Light.ttc',
    '/Library/Fonts/Arial Unicode.ttf',
    # Linux
    '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf',
    # Windows
    'C:/Windows/Fonts/msyh.ttc',
    'C:/Windows/Fonts/simsun.ttc',
]

for fp in cjk_candidates:
    if os.path.exists(fp):
        try:
            pdfmetrics.registerFont(TTFont('CJK', fp, subfontIndex=0))
            font_name = 'CJK'
            bold_font = 'CJK'
            font_registered = True
            break
        except Exception:
            continue

# ── Styles ──
styles = getSampleStyleSheet()

style_title = ParagraphStyle(
    'ReportTitle', parent=styles['Title'],
    fontName=bold_font, fontSize=22, leading=28,
    textColor=HexColor('#1a1a2e'), spaceAfter=6,
)
style_meta = ParagraphStyle(
    'ReportMeta', parent=styles['Normal'],
    fontName=font_name, fontSize=9, leading=12,
    textColor=HexColor('#8e8ea0'), spaceAfter=16,
)
style_h2 = ParagraphStyle(
    'ReportH2', parent=styles['Heading2'],
    fontName=bold_font, fontSize=15, leading=20,
    textColor=HexColor('#1a1a2e'), spaceBefore=16, spaceAfter=8,
    borderWidth=0, borderPadding=0,
)
style_body = ParagraphStyle(
    'ReportBody', parent=styles['Normal'],
    fontName=font_name, fontSize=10, leading=16,
    textColor=HexColor('#333333'), spaceAfter=8,
)
style_bullet = ParagraphStyle(
    'ReportBullet', parent=style_body,
    bulletFontName=font_name, bulletFontSize=10,
    leftIndent=18, bulletIndent=6,
)
style_callout = ParagraphStyle(
    'ReportCallout', parent=style_body,
    fontName=font_name, fontSize=10, leading=16,
    textColor=HexColor('#4a4a6a'), backColor=HexColor('#f8f7ff'),
    borderWidth=0, leftIndent=12, spaceAfter=12, spaceBefore=8,
    borderPadding=(8, 12, 8, 12),
)
style_footer = ParagraphStyle(
    'ReportFooter', parent=styles['Normal'],
    fontName=font_name, fontSize=8, leading=10,
    textColor=HexColor('#8e8ea0'), alignment=TA_CENTER,
)

# ── Build document ──
doc = SimpleDocTemplate(
    output_path, pagesize=A4,
    leftMargin=2*cm, rightMargin=2*cm,
    topMargin=2*cm, bottomMargin=2*cm,
)

story = []
title_text = data.get('title', 'Report')
story.append(Paragraph(title_text, style_title))

from datetime import datetime
ts = datetime.now().strftime('%Y-%m-%d %H:%M')
story.append(Paragraph(f'Generated: {{ts}} &nbsp;|&nbsp; AIjia', style_meta))
story.append(HRFlowable(width='100%', thickness=2, color=HexColor('#6c5ce7'), spaceAfter=16))

page_width = A4[0] - 4*cm  # usable width

for section in data.get('sections', []):
    elements = []
    heading = section.get('heading', '')
    if heading:
        elements.append(Paragraph(heading, style_h2))
        elements.append(HRFlowable(width='100%', thickness=0.5, color=HexColor('#e8e8f0'), spaceAfter=8))

    # Content paragraphs
    content = section.get('content', '')
    if content and content.strip():
        for line in content.split('\n'):
            line = line.strip()
            if not line:
                continue
            if line.startswith('### '):
                ps = ParagraphStyle('H4', parent=style_body, fontName=bold_font, fontSize=11, spaceBefore=8)
                elements.append(Paragraph(line[4:], ps))
            elif line.startswith('## ') or line.startswith('# '):
                prefix_len = 3 if line.startswith('## ') else 2
                ps = ParagraphStyle('H3', parent=style_body, fontName=bold_font, fontSize=12, spaceBefore=10)
                elements.append(Paragraph(line[prefix_len:], ps))
            elif line.startswith('- ') or line.startswith('* '):
                elements.append(Paragraph(line[2:], style_bullet, bulletText='•'))
            else:
                # Convert **bold** to <b>bold</b> for reportlab
                import re
                line = re.sub('\\*\\*(.+?)\\*\\*', '<b>\\1</b>', line)
                elements.append(Paragraph(line, style_body))

    # Metrics
    metrics = section.get('metrics')
    if metrics and isinstance(metrics, list):
        for m in metrics:
            label = m.get('label', '')
            value = m.get('value', '')
            subtitle = m.get('subtitle', '')
            line_text = f'<b>{{label}}</b>: <font size=14>{{value}}</font>'
            if subtitle:
                line_text += f' <font size=8 color=grey>({{subtitle}})</font>'
            elements.append(Paragraph(line_text, style_body))

    # Table
    table_data = section.get('table')
    if table_data:
        columns = table_data.get('columns', [])
        rows = table_data.get('rows', [])
        if columns and rows:
            # Column labels
            col_info = []
            for col in columns:
                if isinstance(col, str):
                    col_info.append((col, col))
                else:
                    col_info.append((col.get('label', ''), col.get('key', '')))

            header_row = [Paragraph(f'<b>{{label}}</b>', style_body) for label, _ in col_info]
            tdata = [header_row]

            for row in rows:
                if isinstance(row, list):
                    cells = [Paragraph(str(row[i]) if i < len(row) else '', style_body) for i in range(len(col_info))]
                else:
                    cells = [Paragraph(str(row.get(key, '')), style_body) for _, key in col_info]
                tdata.append(cells)

            n_cols = len(col_info)
            col_width = page_width / n_cols
            t = Table(tdata, colWidths=[col_width]*n_cols, repeatRows=1)
            t.setStyle(TableStyle([
                ('BACKGROUND', (0, 0), (-1, 0), HexColor('#f5f5fa')),
                ('TEXTCOLOR', (0, 0), (-1, 0), HexColor('#444444')),
                ('GRID', (0, 0), (-1, -1), 0.5, HexColor('#e0e0e8')),
                ('ROWBACKGROUNDS', (0, 1), (-1, -1), [HexColor('#ffffff'), HexColor('#fafafa')]),
                ('VALIGN', (0, 0), (-1, -1), 'TOP'),
                ('TOPPADDING', (0, 0), (-1, -1), 4),
                ('BOTTOMPADDING', (0, 0), (-1, -1), 4),
            ]))
            elements.append(Spacer(1, 6))
            elements.append(t)
            elements.append(Spacer(1, 6))

    # Items (bullet list)
    items = section.get('items')
    if items and isinstance(items, list):
        for item in items:
            if isinstance(item, str):
                import re
                item = re.sub('\\*\\*(.+?)\\*\\*', '<b>\\1</b>', item)
                elements.append(Paragraph(item, style_bullet, bulletText='•'))

    # Highlight callout
    highlight = section.get('highlight')
    if highlight and isinstance(highlight, str):
        import re
        highlight = re.sub('\\*\\*(.+?)\\*\\*', '<b>\\1</b>', highlight)
        elements.append(Paragraph(highlight, style_callout))

    # Embedded chart placeholder (PDF can't render interactive HTML chart)
    chart_path = section.get('chart')
    if chart_path and isinstance(chart_path, str) and chart_path.strip():
        note = f'<i>📊 见随附交互图表：{{chart_path}}</i>'
        elements.append(Spacer(1, 4))
        elements.append(Paragraph(note, style_body))

    # Try to keep each section together
    try:
        story.append(KeepTogether(elements))
    except Exception:
        story.extend(elements)

    story.append(Spacer(1, 8))

# Footer
story.append(Spacer(1, 24))
story.append(HRFlowable(width='100%', thickness=0.5, color=HexColor('#e8e8f0'), spaceBefore=8))
story.append(Paragraph(f'Report generated by AIjia — {{ts}}', style_footer))

doc.build(story)
print("OK:" + output_path)
"#
    );

    let result = runner.execute(&python_code).await?;

    // Clean up JSON temp file if Python didn't
    let _ = std::fs::remove_file(&json_temp);

    if result.exit_code != 0 || result.stdout.trim().starts_with("ERROR:") {
        let err_msg = if result.stdout.contains("reportlab not installed") {
            "reportlab not installed".to_string()
        } else {
            format!(
                "exit_code={}, stdout={}, stderr={}",
                result.exit_code,
                result.stdout.trim(),
                result.stderr.trim()
            )
        };
        anyhow::bail!("PDF conversion failed: {}", err_msg);
    }

    let pdf_bytes = std::fs::read(&output_path)?;
    let _ = std::fs::remove_file(&output_path);

    Ok(pdf_bytes)
}

/// Convert HTML content to DOCX using Python htmldocx.
///
/// Uses temp-file protocol: writes HTML to a temp file, Python reads it.
/// This avoids string interpolation injection via triple-quote boundary breaking.
async fn convert_html_to_docx_with_runtime(
    workspace_path: &Path,
    html: &str,
    python_runtime: Option<(&PathBuf, Option<&PathBuf>)>,
) -> Result<Vec<u8>> {
    let (python_binary, python_home) =
        python_runtime.ok_or_else(|| anyhow::anyhow!("python runtime unavailable"))?;
    let runner = PythonRunner::with_runtime(
        workspace_path.to_path_buf(),
        crate::python::sandbox::SandboxConfig::for_workspace(&workspace_path.to_path_buf()),
        python_binary.to_path_buf(),
        python_home.cloned(),
    );

    let temp_dir = workspace_path.join("temp");
    std::fs::create_dir_all(&temp_dir)?;

    // Write HTML to temp file (safe: no string interpolation)
    let html_temp = temp_dir.join(format!(
        "html_{}.tmp",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));
    std::fs::write(&html_temp, html)?;

    let output_path = temp_dir.join(format!(
        "report_{}.docx",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));

    let html_temp_str = py_escape(&html_temp.to_string_lossy());
    let output_path_str = py_escape(&output_path.to_string_lossy());

    let python_code = format!(
        r#"
import sys
import os

html_path = '{html_temp_str}'
output_path = '{output_path_str}'

with open(html_path, 'r', encoding='utf-8') as f:
    html_content = f.read()
os.remove(html_path)

try:
    from htmldocx import HtmlToDocx
    from docx import Document

    doc = Document()
    parser = HtmlToDocx()
    parser.add_html_to_document(html_content, doc)
    doc.save(output_path)
    print("OK:" + output_path)
except ImportError as exc:
    print("ERROR:missing_library:" + str(exc))
    sys.exit(1)
except Exception as exc:
    print("ERROR:" + str(exc))
    sys.exit(1)
"#
    );

    let result = runner.execute(&python_code).await?;

    // Clean up HTML temp file if Python didn't
    let _ = std::fs::remove_file(&html_temp);

    if result.exit_code != 0 || result.stdout.trim().starts_with("ERROR:") {
        let err_msg = format!(
            "exit_code={}, stdout={}, stderr={}",
            result.exit_code,
            result.stdout.trim(),
            result.stderr.trim()
        );
        anyhow::bail!("DOCX conversion failed: {}", err_msg);
    }

    let docx_bytes = std::fs::read(&output_path)?;
    let _ = std::fs::remove_file(&output_path);

    Ok(docx_bytes)
}

/// Build a complete standalone HTML report from a title and sections array.
fn build_html_report(title: &str, sections: &[Value]) -> String {
    let mut body = String::new();

    for section in sections {
        let heading = section
            .get("heading")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled Section");

        body.push_str(&format!(
            "    <section>\n      <h2>{}</h2>\n",
            html_escape(heading),
        ));

        // Text content (with markdown → HTML conversion)
        if let Some(content) = section.get("content").and_then(|v| v.as_str()) {
            if !content.trim().is_empty() {
                body.push_str("      <div class=\"content\">");
                body.push_str(&report_markdown_to_html(content));
                body.push_str("</div>\n");
            }
        }

        // Metric cards
        if let Some(metrics) = section.get("metrics").and_then(|v| v.as_array()) {
            body.push_str("      <div class=\"metric-grid\">\n");
            for m in metrics {
                let label = m.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let value = m.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let subtitle = m.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
                let state = m.get("state").and_then(|v| v.as_str()).unwrap_or("neutral");
                let state_class = match state {
                    "good" => "metric-good",
                    "warn" => "metric-warn",
                    "bad" => "metric-bad",
                    _ => "",
                };
                body.push_str(&format!(
                    "        <div class=\"metric-card {}\"><div class=\"metric-label\">{}</div><div class=\"metric-value\">{}</div><div class=\"metric-sub\">{}</div></div>\n",
                    state_class, html_escape(label), html_escape(value), html_escape(subtitle),
                ));
            }
            body.push_str("      </div>\n");
        }

        // Structured table
        if let Some(table) = section.get("table") {
            render_report_table(&mut body, table);
        }

        // Bullet list
        if let Some(items) = section.get("items").and_then(|v| v.as_array()) {
            body.push_str("      <ul class=\"item-list\">\n");
            for item in items {
                if let Some(text) = item.as_str() {
                    body.push_str(&format!(
                        "        <li>{}</li>\n",
                        report_inline_md(&html_escape(text))
                    ));
                }
            }
            body.push_str("      </ul>\n");
        }

        // Highlight callout
        if let Some(highlight) = section.get("highlight").and_then(|v| v.as_str()) {
            body.push_str(&format!(
                "      <div class=\"callout\">{}</div>\n",
                report_inline_md(&html_escape(highlight)),
            ));
        }

        // Embedded chart (iframe to a generate_chart output, e.g. "charts/chart_xxx.html")
        if let Some(chart_path) = section.get("chart").and_then(|v| v.as_str()) {
            let trimmed = chart_path.trim();
            // Whitelist: only allow relative path under charts/, no traversal, must end with .html
            let safe = !trimmed.is_empty()
                && !trimmed.contains("..")
                && !trimmed.starts_with('/')
                && !trimmed.contains(':')
                && trimmed.ends_with(".html")
                && (trimmed.starts_with("charts/") || !trimmed.contains('/'));
            if safe {
                let src = if trimmed.contains('/') {
                    format!("../{}", trimmed)
                } else {
                    format!("../charts/{}", trimmed)
                };
                body.push_str(&format!(
                    "      <div class=\"chart-embed\"><iframe src=\"{}\" loading=\"lazy\" style=\"width:100%;height:560px;border:1px solid #e5e7eb;border-radius:8px;background:#fff\"></iframe></div>\n",
                    html_escape(&src),
                ));
            }
        }

        body.push_str("    </section>\n");
    }

    let now = chrono::Local::now();

    let formatted = format!(
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<style>
  @page {{ margin: 2cm; }}
  @media print {{
    body {{ padding: 0; }}
    .no-print {{ display: none; }}
    section {{ break-inside: avoid; }}
  }}
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif;
    font-size: 14px;
    line-height: 1.7;
    color: #1a1a2e;
    background: #fff;
    padding: 48px 40px;
    max-width: 960px;
    margin: 0 auto;
  }}
  /* ── Header ── */
  .report-header {{
    border-bottom: 3px solid #6c5ce7;
    padding-bottom: 24px;
    margin-bottom: 36px;
  }}
  .report-header h1 {{
    font-size: 26px;
    font-weight: 700;
    color: #1a1a2e;
    margin-bottom: 8px;
  }}
  .report-header .meta {{
    font-size: 12px;
    color: #8e8ea0;
  }}
  /* ── Sections ── */
  section {{
    margin-bottom: 32px;
    page-break-inside: avoid;
  }}
  h2 {{
    font-size: 18px;
    font-weight: 700;
    color: #1a1a2e;
    padding-bottom: 8px;
    border-bottom: 1px solid #e8e8f0;
    margin-bottom: 16px;
  }}
  .content {{
    line-height: 1.7;
  }}
  .content p {{ margin-bottom: 10px; }}
  .content h3 {{ font-size: 15px; font-weight: 600; margin: 16px 0 8px; color: #2d3436; }}
  .content h4 {{ font-size: 14px; font-weight: 600; margin: 12px 0 6px; color: #2d3436; }}
  .content ul, .content ol {{ margin: 8px 0 12px 20px; }}
  .content li {{ margin-bottom: 4px; }}
  .content strong {{ color: #1a1a2e; }}
  .content code {{ background: #f0f0f5; padding: 2px 6px; border-radius: 3px; font-size: 13px; }}
  /* ── Metric Cards ── */
  .metric-grid {{
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
    margin: 16px 0;
  }}
  .metric-card {{
    flex: 1;
    min-width: 160px;
    padding: 16px 20px;
    border-radius: 10px;
    border: 1px solid #e0e0e8;
    background: #fafafa;
  }}
  .metric-label {{ font-size: 12px; color: #8e8ea0; font-weight: 500; }}
  .metric-value {{ font-size: 24px; font-weight: 700; margin: 6px 0 4px; color: #1a1a2e; }}
  .metric-sub {{ font-size: 11px; color: #8e8ea0; }}
  .metric-good {{ border-color: #00b894; background: #f0faf7; }}
  .metric-good .metric-value {{ color: #00b894; }}
  .metric-warn {{ border-color: #fdcb6e; background: #fffef5; }}
  .metric-warn .metric-value {{ color: #e17055; }}
  .metric-bad {{ border-color: #ff7675; background: #fff5f5; }}
  .metric-bad .metric-value {{ color: #d63031; }}
  /* ── Tables ── */
  table {{
    width: 100%;
    border-collapse: collapse;
    margin: 14px 0;
    font-size: 13px;
  }}
  table th, table td {{
    border: 1px solid #e0e0e8;
    padding: 8px 12px;
    text-align: left;
  }}
  table th {{
    background: #f5f5fa;
    font-weight: 600;
    color: #444;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }}
  table tr:nth-child(even) {{ background: #fafafa; }}
  /* ── Callout ── */
  .callout {{
    border-left: 4px solid #6c5ce7;
    padding: 14px 18px;
    margin: 16px 0;
    background: #f8f7ff;
    border-radius: 0 8px 8px 0;
    font-size: 14px;
    line-height: 1.6;
  }}
  /* ── Item List ── */
  .item-list {{
    margin: 12px 0 16px 20px;
    line-height: 1.7;
  }}
  .item-list li {{ margin-bottom: 6px; }}
  /* ── Footer ── */
  .report-footer {{
    border-top: 1px solid #e8e8f0;
    padding-top: 16px;
    margin-top: 48px;
    font-size: 11px;
    color: #8e8ea0;
    text-align: center;
  }}
</style>
</head>
<body>
<div class="report-header">
  <h1>{title}</h1>
  <div class="meta">Generated: {timestamp} &nbsp;|&nbsp; {{PRODUCT_NAME}} — 组织专家，工作助手</div>
</div>
{body}
<div class="report-footer">
  本报告由 {{PRODUCT_NAME}}（组织专家，工作助手）自动生成 — {timestamp}
</div>
</body>
</html>"##,
        title = html_escape(title),
        body = body,
        timestamp = now.format("%Y-%m-%d %H:%M"),
    );
    // Replace product name placeholder (custom branding handled by caller)
    formatted.replace("{{PRODUCT_NAME}}", "AI小家")
}

/// Render a structured table for report HTML.
fn render_report_table(html: &mut String, table: &Value) {
    let title = table.get("title").and_then(|v| v.as_str());
    if let Some(t) = title {
        html.push_str(&format!(
            "      <div style=\"font-weight:600;font-size:13px;margin:12px 0 6px\">{}</div>\n",
            html_escape(t)
        ));
    }

    // Support both { columns: [str], rows: [[str]] } and { columns: [{label, key}], rows: [{key: val}] }
    let columns = match table.get("columns").and_then(|v| v.as_array()) {
        Some(cols) => cols,
        None => return,
    };
    let rows = match table.get("rows").and_then(|v| v.as_array()) {
        Some(rows) => rows,
        None => return,
    };

    html.push_str("      <table><thead><tr>\n");

    // Determine column labels and keys
    let col_info: Vec<(String, String)> = columns
        .iter()
        .map(|col| {
            if let Some(label) = col.as_str() {
                (label.to_string(), label.to_string())
            } else {
                let label = col
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let key = col
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                (label, key)
            }
        })
        .collect();

    for (label, _) in &col_info {
        html.push_str(&format!("        <th>{}</th>\n", html_escape(label)));
    }
    html.push_str("      </tr></thead><tbody>\n");

    for row in rows {
        html.push_str("      <tr>");
        if let Some(row_arr) = row.as_array() {
            // Row is an array of values
            for (i, _) in col_info.iter().enumerate() {
                let cell = row_arr
                    .get(i)
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                html.push_str(&format!("<td>{}</td>", html_escape(&cell)));
            }
        } else {
            // Row is an object with keys
            for (_, key) in &col_info {
                let cell = row
                    .get(key.as_str())
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                html.push_str(&format!("<td>{}</td>", html_escape(&cell)));
            }
        }
        html.push_str("</tr>\n");
    }
    html.push_str("      </tbody></table>\n");
}

/// Simple markdown → HTML for report content blocks.
fn report_markdown_to_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    let mut in_list = false;
    let mut in_table = false;
    let mut table_header_done = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Table rows (| col | col |)
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            if trimmed
                .chars()
                .all(|c| c == '|' || c == '-' || c == ' ' || c == ':')
            {
                // Separator line — skip but mark header done
                table_header_done = true;
                continue;
            }
            if !in_table {
                if in_list {
                    result.push_str("</ul>");
                    in_list = false;
                }
                result.push_str("<table>");
                in_table = true;
                table_header_done = false;
            }
            let cells: Vec<&str> = trimmed
                .split('|')
                .filter(|s| !s.trim().is_empty())
                .collect();
            let tag = if !table_header_done { "th" } else { "td" };
            result.push_str("<tr>");
            for cell in &cells {
                result.push_str(&format!(
                    "<{}>{}</{}>",
                    tag,
                    report_inline_md(&html_escape(cell.trim())),
                    tag
                ));
            }
            result.push_str("</tr>");
            continue;
        }

        // Close table if we were in one
        if in_table {
            result.push_str("</table>");
            in_table = false;
            table_header_done = false;
        }

        // Unordered list items
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if !in_list {
                result.push_str("<ul>");
                in_list = true;
            }
            result.push_str(&format!(
                "<li>{}</li>",
                report_inline_md(&html_escape(&trimmed[2..]))
            ));
            continue;
        }

        // Ordered list items
        if let Some(rest) = trimmed
            .strip_prefix(|c: char| c.is_ascii_digit())
            .and_then(|s| s.strip_prefix(". "))
        {
            if !in_list {
                result.push_str("<ol>");
                in_list = true;
            }
            result.push_str(&format!(
                "<li>{}</li>",
                report_inline_md(&html_escape(rest))
            ));
            continue;
        }

        if in_list {
            if trimmed.is_empty() {
                result.push_str("</ul>");
                in_list = false;
            }
        }

        // Headers
        if trimmed.starts_with("### ") {
            result.push_str(&format!(
                "<h4>{}</h4>",
                report_inline_md(&html_escape(&trimmed[4..]))
            ));
        } else if trimmed.starts_with("## ") {
            result.push_str(&format!(
                "<h3>{}</h3>",
                report_inline_md(&html_escape(&trimmed[3..]))
            ));
        } else if trimmed.starts_with("# ") {
            result.push_str(&format!(
                "<h3>{}</h3>",
                report_inline_md(&html_escape(&trimmed[2..]))
            ));
        } else if trimmed.is_empty() {
            // Skip excessive blank lines
        } else {
            result.push_str(&format!(
                "<p>{}</p>",
                report_inline_md(&html_escape(trimmed))
            ));
        }
    }

    if in_list {
        result.push_str("</ul>");
    }
    if in_table {
        result.push_str("</table>");
    }
    result
}

/// Convert inline markdown (bold, code) to HTML for reports.
fn report_inline_md(text: &str) -> String {
    let mut result = text.to_string();
    // Bold: **text**
    while let Some(start) = result.find("**") {
        if let Some(end) = result[start + 2..].find("**") {
            let inner = result[start + 2..start + 2 + end].to_string();
            result = format!(
                "{}<strong>{}</strong>{}",
                &result[..start],
                inner,
                &result[start + 2 + end + 2..]
            );
        } else {
            break;
        }
    }
    // Inline code: `text`
    while let Some(start) = result.find('`') {
        if let Some(end) = result[start + 1..].find('`') {
            let inner = result[start + 1..start + 1 + end].to_string();
            result = format!(
                "{}<code>{}</code>{}",
                &result[..start],
                inner,
                &result[start + 1 + end + 1..]
            );
        } else {
            break;
        }
    }
    result
}

/// Build a Markdown report from a title and sections array.
fn build_markdown_report(title: &str, sections: &[Value]) -> String {
    let mut output = format!("# {}\n\n", title);
    for section in sections {
        let heading = section
            .get("heading")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled Section");
        let content = section
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        output.push_str(&format!("## {}\n\n{}\n\n", heading, content));
        if let Some(chart_path) = section.get("chart").and_then(|v| v.as_str()) {
            let trimmed = chart_path.trim();
            if !trimmed.is_empty() && !trimmed.contains("..") {
                output.push_str(&format!("> 📊 [查看交互图表]({})\n\n", trimmed));
            }
        }
    }
    output
}

/// Minimal HTML escaping.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_html_report() {
        let sections = vec![
            json!({"heading": "Summary", "content": "Good results."}),
            json!({"heading": "Details", "content": "Line1\nLine2"}),
        ];
        let html = build_html_report("Test Report", &sections);
        assert!(html.contains("<title>Test Report</title>"));
        assert!(html.contains("<h2>Summary</h2>"));
        assert!(html.contains("Good results."));
        // Multi-line content is split into separate <p> tags
        assert!(html.contains("<p>Line1</p>"));
        assert!(html.contains("<p>Line2</p>"));
    }

    #[test]
    fn test_build_html_report_escapes_html() {
        let sections = vec![json!({"heading": "<script>alert(1)</script>", "content": "a & b"})];
        let html = build_html_report("Test", &sections);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("a &amp; b"));
    }

    #[test]
    fn test_build_markdown_report() {
        let sections = vec![json!({"heading": "Intro", "content": "Hello"})];
        let md = build_markdown_report("My Report", &sections);
        assert!(md.starts_with("# My Report\n"));
        assert!(md.contains("## Intro\n"));
        assert!(md.contains("Hello"));
    }
}
