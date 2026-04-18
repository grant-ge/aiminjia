use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::storage::file_store::AppStorage;

pub struct ChartRunOutput {
    pub html_bytes: Vec<u8>,
    pub chart_filename: String,
}

pub struct PersistedChartInfo {
    pub file_id: String,
    pub file_name: String,
    pub stored_path: String,
    pub file_size: u64,
}

#[async_trait]
pub trait ChartCapability: Send + Sync + std::fmt::Debug {
    async fn run_chart_python(
        &self,
        workspace_path: &Path,
        chart_type: &str,
        title: &str,
        data: &Value,
        options: &Value,
    ) -> Result<ChartRunOutput>;

    async fn persist_chart(
        &self,
        conversation_id: &str,
        bytes: &[u8],
        filename: &str,
        chart_type: &str,
        title: &str,
    ) -> Result<PersistedChartInfo>;
}

pub struct DefaultChartCapability {
    pub storage: Arc<AppStorage>,
    pub workspace_path: PathBuf,
    pub python_binary: PathBuf,
    pub python_home: Option<PathBuf>,
}

impl std::fmt::Debug for DefaultChartCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultChartCapability")
            .field("workspace_path", &self.workspace_path)
            .finish()
    }
}

#[async_trait]
impl ChartCapability for DefaultChartCapability {
    async fn run_chart_python(
        &self,
        workspace_path: &Path,
        chart_type: &str,
        title: &str,
        data: &Value,
        options: &Value,
    ) -> Result<ChartRunOutput> {
        let temp_dir = workspace_path.join("temp");
        let chart_dir = workspace_path.join("charts");
        std::fs::create_dir_all(&temp_dir)?;
        std::fs::create_dir_all(&chart_dir)?;

        let chart_filename = format!(
            "chart_{}.html",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        );
        let output_path = chart_dir.join(&chart_filename);
        let data_temp = temp_dir.join(format!(
            "chart_data_{}.json",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        ));
        let options_temp = temp_dir.join(format!(
            "chart_opts_{}.json",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        ));

        std::fs::write(
            &data_temp,
            serde_json::to_string(data).unwrap_or_else(|_| "{}".into()),
        )?;
        std::fs::write(
            &options_temp,
            serde_json::to_string(options).unwrap_or_else(|_| "{}".into()),
        )?;

        let python_code = crate::llm::tool_executor::build_chart_python(
            chart_type,
            title,
            &data_temp.to_string_lossy(),
            &options_temp.to_string_lossy(),
            &output_path.to_string_lossy(),
        );
        let runner = crate::python::runner::PythonRunner::with_runtime(
            workspace_path.to_path_buf(),
            crate::python::sandbox::SandboxConfig::for_workspace(&workspace_path.to_path_buf()),
            self.python_binary.clone(),
            self.python_home.clone(),
        );
        let result = runner.execute(&python_code).await?;
        let _ = std::fs::remove_file(&data_temp);
        let _ = std::fs::remove_file(&options_temp);

        if result.exit_code != 0 {
            anyhow::bail!(
                "Chart generation failed (exit {}):\n{}",
                result.exit_code,
                if result.stderr.is_empty() {
                    &result.stdout
                } else {
                    &result.stderr
                }
            );
        }

        let html_bytes = std::fs::read(&output_path)?;
        Ok(ChartRunOutput {
            html_bytes,
            chart_filename,
        })
    }

    async fn persist_chart(
        &self,
        conversation_id: &str,
        bytes: &[u8],
        filename: &str,
        _chart_type: &str,
        title: &str,
    ) -> Result<PersistedChartInfo> {
        let chart_dir = self.workspace_path.join("charts");
        std::fs::create_dir_all(&chart_dir)?;
        let output_path = chart_dir.join(filename);
        if !output_path.exists() {
            std::fs::write(&output_path, bytes)?;
        }

        let stored_path = format!("charts/{}", filename);
        let file_size = bytes.len() as u64;
        let file_id = Uuid::new_v4().to_string();

        self.storage.insert_generated_file(
            &file_id,
            conversation_id,
            None,
            filename,
            &stored_path,
            "html",
            file_size as i64,
            "chart",
            Some(title),
            1,
            true,
            None,
            None,
            None,
        )?;

        Ok(PersistedChartInfo {
            file_id,
            file_name: filename.to_string(),
            stored_path,
            file_size,
        })
    }
}
