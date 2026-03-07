//! generate_slides handler — build PPTX presentations via python-pptx.

use anyhow::Result;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::FileMeta;
use crate::python::runner::PythonRunner;

use super::FileGenResult;
use super::file_load::{get_pii_unmask_map, unmask_text};
use super::optional_str;
use super::util::{py_escape, slugify};

/// Generate a PPTX presentation using python-pptx.
pub(crate) async fn handle_generate_slides(ctx: &PluginContext, args: &Value) -> Result<FileGenResult> {
    // LLM sometimes sends slides as a JSON string instead of an array — auto-parse it.
    let slides_owned: Vec<Value>;
    let slides = match args.get("slides") {
        Some(Value::Array(arr)) => arr,
        Some(Value::String(s)) => {
            slides_owned = serde_json::from_str(s)
                .map_err(|e| anyhow::anyhow!("slides is a string but not valid JSON array: {e}"))?;
            &slides_owned
        }
        _ => anyhow::bail!("Missing required array argument: slides"),
    };

    let title = optional_str(args, "title").unwrap_or_else(|| {
        slides
            .first()
            .and_then(|s| s.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("Presentation")
    });

    let theme = optional_str(args, "theme").unwrap_or("light");

    // Collect PII unmask mapping for this conversation
    let unmask_map = get_pii_unmask_map(&ctx.storage, &ctx.conversation_id);

    // Unmask PII in the title
    let title_unmasked = unmask_text(title, &unmask_map);

    // Unmask PII in slides JSON
    let slides_json_str = serde_json::to_string(slides)?;
    let slides_json_str = unmask_text(&slides_json_str, &unmask_map);

    // Write slides JSON to temp file (safe: avoids string interpolation injection)
    let temp_dir = ctx.workspace_path.join("temp");
    std::fs::create_dir_all(&temp_dir)?;

    let slides_temp = temp_dir.join(format!(
        "slides_{}.json",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));
    std::fs::write(&slides_temp, &slides_json_str)?;

    let output_path = temp_dir.join(format!(
        "slides_{}.pptx",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));

    let slides_temp_str = py_escape(&slides_temp.to_string_lossy());
    let output_path_str = py_escape(&output_path.to_string_lossy());
    let title_escaped = py_escape(&title_unmasked);
    let theme_escaped = py_escape(theme);

    let python_code = format!(r#"
import json
import sys
import os

slides_path = '{slides_temp_str}'
output_path = '{output_path_str}'
pres_title = '{title_escaped}'
theme = '{theme_escaped}'

with open(slides_path, 'r', encoding='utf-8') as f:
    slides = json.load(f)
os.remove(slides_path)

try:
    from pptx import Presentation
    from pptx.util import Inches, Pt, Emu
    from pptx.dml.color import RGBColor
    from pptx.enum.text import PP_ALIGN

    prs = Presentation()
    # 16:9 widescreen
    prs.slide_width = Inches(13.333)
    prs.slide_height = Inches(7.5)

    # Theme colors
    if theme == 'dark':
        bg_color = RGBColor(0x1a, 0x1a, 0x2e)
        title_color = RGBColor(0xff, 0xff, 0xff)
        text_color = RGBColor(0xe0, 0xe0, 0xe8)
        accent_color = RGBColor(0x6c, 0x5c, 0xe7)
    else:
        bg_color = RGBColor(0xff, 0xff, 0xff)
        title_color = RGBColor(0x1a, 0x1a, 0x2e)
        text_color = RGBColor(0x2d, 0x34, 0x36)
        accent_color = RGBColor(0x6c, 0x5c, 0xe7)

    def set_slide_bg(slide, color):
        background = slide.background
        fill = background.fill
        fill.solid()
        fill.fore_color.rgb = color

    def add_textbox(slide, left, top, width, height, text, font_size=18, color=None, bold=False, alignment=PP_ALIGN.LEFT):
        txBox = slide.shapes.add_textbox(left, top, width, height)
        tf = txBox.text_frame
        tf.word_wrap = True
        p = tf.paragraphs[0]
        p.text = text
        p.font.size = Pt(font_size)
        p.font.bold = bold
        if color:
            p.font.color.rgb = color
        p.alignment = alignment
        return tf

    for i, slide_data in enumerate(slides):
        slide_title = slide_data.get('title', '')
        bullets = slide_data.get('bullets', [])
        notes = slide_data.get('notes', '')
        layout = slide_data.get('layout', 'title_and_content')

        # Use blank layout (index 6) as base, build custom content
        try:
            slide_layout = prs.slide_layouts[6]  # blank
        except IndexError:
            slide_layout = prs.slide_layouts[0]

        slide = prs.slides.add_slide(slide_layout)
        set_slide_bg(slide, bg_color)

        if layout == 'title_slide':
            # Cover page: centered title + subtitle line
            add_textbox(slide, Inches(1.5), Inches(2.5), Inches(10.333), Inches(1.5),
                       slide_title, font_size=40, color=title_color, bold=True,
                       alignment=PP_ALIGN.CENTER)
            # Accent line
            shape = slide.shapes.add_shape(
                1, Inches(5.5), Inches(4.2), Inches(2.333), Pt(4))
            shape.fill.solid()
            shape.fill.fore_color.rgb = accent_color
            shape.line.fill.background()
            # Subtitle from first bullet if available
            if bullets:
                add_textbox(slide, Inches(1.5), Inches(4.5), Inches(10.333), Inches(1),
                           bullets[0], font_size=20, color=text_color,
                           alignment=PP_ALIGN.CENTER)

        elif layout == 'section_header':
            # Section divider: large centered title
            add_textbox(slide, Inches(1.5), Inches(2.8), Inches(10.333), Inches(1.5),
                       slide_title, font_size=36, color=accent_color, bold=True,
                       alignment=PP_ALIGN.CENTER)

        elif layout == 'blank':
            # Blank slide — only add title if non-empty
            if slide_title:
                add_textbox(slide, Inches(0.8), Inches(0.4), Inches(11.733), Inches(0.8),
                           slide_title, font_size=28, color=title_color, bold=True)

        else:
            # title_and_content (default)
            # Title
            add_textbox(slide, Inches(0.8), Inches(0.4), Inches(11.733), Inches(0.8),
                       slide_title, font_size=28, color=title_color, bold=True)
            # Accent underline
            shape = slide.shapes.add_shape(
                1, Inches(0.8), Inches(1.2), Inches(1.5), Pt(3))
            shape.fill.solid()
            shape.fill.fore_color.rgb = accent_color
            shape.line.fill.background()

            # Bullets
            if bullets:
                txBox = slide.shapes.add_textbox(
                    Inches(1.0), Inches(1.6), Inches(11.333), Inches(5.2))
                tf = txBox.text_frame
                tf.word_wrap = True
                for j, bullet in enumerate(bullets):
                    if j == 0:
                        p = tf.paragraphs[0]
                    else:
                        p = tf.add_paragraph()
                    p.text = bullet
                    p.font.size = Pt(18)
                    p.font.color.rgb = text_color
                    p.space_after = Pt(10)
                    p.level = 0

        # Speaker notes
        if notes:
            notes_slide = slide.notes_slide
            notes_tf = notes_slide.notes_text_frame
            notes_tf.text = notes

    prs.save(output_path)
    print("OK:" + output_path)
except ImportError as exc:
    print("ERROR:missing_library:" + str(exc))
    sys.exit(1)
except Exception as exc:
    print("ERROR:" + str(exc))
    sys.exit(1)
"#);

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    // Clean up temp JSON file if Python didn't (e.g., on error before os.remove)
    let _ = std::fs::remove_file(&slides_temp);

    if result.exit_code != 0 || result.stdout.trim().starts_with("ERROR:") {
        let err_msg = if result.stdout.contains("missing_library") {
            "python-pptx not installed".to_string()
        } else {
            format!(
                "exit_code={}, stdout={}, stderr={}",
                result.exit_code,
                result.stdout.trim(),
                result.stderr.trim()
            )
        };
        anyhow::bail!("PPTX generation failed: {}", err_msg);
    }

    // Read the generated PPTX file
    let pptx_bytes = std::fs::read(&output_path)?;
    let _ = std::fs::remove_file(&output_path);

    let file_name = format!(
        "slides_{}_{}.pptx",
        slugify(&title_unmasked),
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    );

    let file_info = ctx
        .file_manager
        .write_file("reports", &file_name, &pptx_bytes)?;

    // Record in the database
    let file_id = Uuid::new_v4().to_string();
    if let Err(e) = ctx.storage.insert_generated_file(
        &file_id,
        &ctx.conversation_id,
        None,                     // message_id
        &file_info.file_name,
        &file_info.stored_path,
        &file_info.file_type,
        file_info.file_size as i64,
        "presentation",           // category
        Some(&title_unmasked),    // description
        1,                        // version
        true,                     // is_latest
        None,                     // superseded_by
        None,                     // created_by_step
        None,                     // expires_at
    ) {
        let _ = std::fs::remove_file(ctx.file_manager.full_path(&file_info.stored_path));
        return Err(e.into());
    }

    let result_json = json!({
        "fileId": file_id,
        "fileName": file_info.file_name,
        "storedPath": file_info.stored_path,
        "fileSize": file_info.file_size,
        "format": "pptx",
    });

    let file_meta = FileMeta {
        file_id,
        file_name: file_info.file_name.clone(),
        requested_format: "pptx".to_string(),
        actual_format: "pptx".to_string(),
        file_size: file_info.file_size,
        stored_path: file_info.stored_path.clone(),
        category: "presentation".to_string(),
    };

    Ok(FileGenResult {
        content: serde_json::to_string_pretty(&result_json)?,
        file_meta,
        is_degraded: false,
        degradation_notice: None,
    })
}
