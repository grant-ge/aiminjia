use std::sync::Arc;

use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

#[derive(Clone)]
pub struct TauriExportCommandAdapter {
    db: Arc<AppStorage>,
    file_mgr: Arc<FileManager>,
}

impl TauriExportCommandAdapter {
    pub fn new(db: Arc<AppStorage>, file_mgr: Arc<FileManager>) -> Self {
        Self { db, file_mgr }
    }

    pub async fn export_conversation(
        &self,
        conversation_id: String,
        format: String,
    ) -> Result<serde_json::Value, String> {
        // Delegate to the existing command implementation — business logic stays in commands/export.rs
        // This adapter is a thin forwarding shim that owns the dependencies.
        use crate::commands::export as export_impl;

        log::info!(
            "export_conversation: conversation_id={}, format={}",
            conversation_id,
            format
        );

        // 1. Get conversation title
        let conversations = self.db.get_conversations().map_err(|e| e.to_string())?;
        let conv = conversations
            .iter()
            .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(&conversation_id))
            .ok_or_else(|| format!("Conversation {} not found", conversation_id))?;
        let title = conv
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Conversation Export");
        let created_at = conv.get("createdAt").and_then(|v| v.as_str()).unwrap_or("");

        // 2. Get all messages
        let messages = self
            .db
            .get_messages(&conversation_id)
            .map_err(|e| e.to_string())?;
        if messages.is_empty() {
            return Err("No messages to export".to_string());
        }

        // 3. Render HTML
        let html = export_impl::render_conversation_html(title, &messages, created_at);

        // 4. Prepare workspace paths
        let workspace = self.file_mgr.workspace_path().to_path_buf();
        let temp_dir = workspace.join("temp");
        let exports_dir = workspace.join("exports");
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temp dir: {}", e))?;
        std::fs::create_dir_all(&exports_dir)
            .map_err(|e| format!("Failed to create exports dir: {}", e))?;

        let file_id = uuid::Uuid::new_v4().to_string();
        let safe_title: String = title
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
            .take(30)
            .collect();
        let safe_title = if safe_title.is_empty() {
            "export".to_string()
        } else {
            safe_title
        };
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();

        let html_temp_path = temp_dir.join(format!("export_{}.html", file_id));
        std::fs::write(&html_temp_path, &html)
            .map_err(|e| format!("Failed to write temp HTML: {}", e))?;

        let (output_ext, _output_mime) = match format.as_str() {
            "pdf" => ("pdf", "pdf"),
            "html" => ("html", "html"),
            _ => return Err(format!("Unsupported export format: {}", format)),
        };

        let output_filename = format!("{}_{}.{}", safe_title, timestamp, output_ext);
        let output_path = exports_dir.join(&output_filename);
        let stored_path = format!("exports/{}", output_filename);

        if format == "html" {
            std::fs::write(&output_path, &html)
                .map_err(|e| format!("Failed to write HTML export: {}", e))?;

            let file_size = std::fs::metadata(&output_path)
                .map(|m| m.len() as i64)
                .unwrap_or(0);

            self.db
                .insert_generated_file(
                    &file_id,
                    &conversation_id,
                    None,
                    &output_filename,
                    &stored_path,
                    "html",
                    file_size,
                    "report",
                    Some(&format!("Conversation export ({})", format)),
                    1,
                    true,
                    None,
                    None,
                    None,
                )
                .map_err(|e| format!("Failed to record export file: {}", e))?;

            log::info!(
                "export_conversation OK (HTML direct): file_id={}, path={}",
                file_id,
                stored_path
            );

            return Ok(serde_json::json!({
                "fileId": file_id,
                "fileName": output_filename,
                "storedPath": stored_path,
                "fileSize": file_size,
            }));
        }

        // For PDF: write HTML to temp and run Python conversion
        let python_code = format!(
            r#"
import json, sys

html_path = {html_path}
pdf_path = {pdf_path}

try:
    from weasyprint import HTML
    HTML(filename=html_path).write_pdf(pdf_path)
    print(json.dumps({{"status": "success", "path": pdf_path}}))
except ImportError:
    import shutil
    html_out = pdf_path.replace('.pdf', '.html')
    shutil.copy(html_path, html_out)
    print(json.dumps({{"status": "fallback_html", "path": html_out, "error": "weasyprint not installed, exported as HTML instead. Install with: pip install weasyprint"}}))
except Exception as e:
    print(json.dumps({{"status": "error", "error": str(e)}}))
"#,
            html_path =
                serde_json::to_string(&html_temp_path.to_string_lossy().to_string()).unwrap(),
            pdf_path =
                serde_json::to_string(&output_path.to_string_lossy().to_string()).unwrap(),
        );

        let script_path = temp_dir.join(format!("export_script_{}.py", file_id));
        std::fs::write(&script_path, &python_code)
            .map_err(|e| format!("Failed to write export script: {}", e))?;

        let py_output = tokio::process::Command::new("python3")
            .arg("-u")
            .arg(&script_path)
            .env("PYTHONIOENCODING", "utf-8")
            .output()
            .await
            .map_err(|e| format!("Failed to run python3: {}", e))?;

        let _ = std::fs::remove_file(&script_path);

        let stdout = String::from_utf8_lossy(&py_output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&py_output.stderr).to_string();

        if !py_output.status.success() && stdout.trim().is_empty() {
            let _ = std::fs::remove_file(&html_temp_path);
            return Err(format!(
                "Python export failed: {}",
                if stderr.is_empty() {
                    "unknown error"
                } else {
                    &stderr
                }
            ));
        }

        let python_output: serde_json::Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|_| serde_json::json!({"status": "error", "error": stdout}));

        let status = python_output
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("error");

        if status == "error" {
            let _ = std::fs::remove_file(&html_temp_path);
            let err_msg = python_output
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Err(format!("Export failed: {}", err_msg));
        }

        let (final_path, final_stored_path, final_filename, final_ext) =
            if status == "fallback_html" {
                let html_filename = output_filename.replace(".pdf", ".html");
                let html_stored = format!("exports/{}", html_filename);
                let html_out = python_output
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| exports_dir.join(&html_filename));
                (html_out, html_stored, html_filename, "html")
            } else {
                (
                    output_path.clone(),
                    stored_path.clone(),
                    output_filename.clone(),
                    output_ext,
                )
            };

        let file_size = std::fs::metadata(&final_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);

        self.db
            .insert_generated_file(
                &file_id,
                &conversation_id,
                None,
                &final_filename,
                &final_stored_path,
                final_ext,
                file_size,
                "report",
                Some(&format!("Conversation export ({})", format)),
                1,
                true,
                None,
                None,
                None,
            )
            .map_err(|e| format!("Failed to record export file: {}", e))?;

        let _ = std::fs::remove_file(&html_temp_path);

        log::info!(
            "export_conversation OK: file_id={}, path={}",
            file_id,
            final_stored_path
        );

        Ok(serde_json::json!({
            "fileId": file_id,
            "fileName": final_filename,
            "storedPath": final_stored_path,
            "fileSize": file_size,
        }))
    }
}
