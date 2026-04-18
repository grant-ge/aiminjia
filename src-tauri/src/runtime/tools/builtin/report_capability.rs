use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::auth::AuthManager;
use crate::llm::tool_executor::FileGenResult;
use crate::plugin::tool_trait::FileMeta;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

pub struct ReportGenOutput {
    pub bytes: Vec<u8>,
    pub extension: String,
    pub actual_format: String,
    pub is_degraded: bool,
    pub degradation_notice: Option<String>,
}

pub struct PersistedFileInfo {
    pub file_id: String,
    pub file_name: String,
    pub stored_path: String,
    pub file_size: u64,
}

#[async_trait]
pub trait ReportCapability: Send + Sync + std::fmt::Debug {
    async fn generate_report_bytes(
        &self,
        workspace_path: &Path,
        title: &str,
        sections: &[Value],
        format: &str,
        unmask_map: &HashMap<String, String>,
        product_name: Option<&str>,
    ) -> Result<ReportGenOutput>;

    fn get_pii_unmask_map(&self, conversation_id: &str) -> HashMap<String, String>;

    async fn get_product_name(&self) -> Option<String>;

    async fn persist_file(
        &self,
        conversation_id: &str,
        bytes: &[u8],
        extension: &str,
        title: &str,
        _actual_format: &str,
    ) -> Result<PersistedFileInfo>;
}

pub struct DefaultReportCapability {
    pub storage: Arc<AppStorage>,
    pub file_manager: Arc<FileManager>,
    pub auth_manager: Option<Arc<AuthManager>>,
    pub workspace_path: PathBuf,
    pub python_binary: PathBuf,
    pub python_home: Option<PathBuf>,
}

impl std::fmt::Debug for DefaultReportCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultReportCapability")
            .field("workspace_path", &self.workspace_path)
            .finish()
    }
}

#[async_trait]
impl ReportCapability for DefaultReportCapability {
    async fn generate_report_bytes(
        &self,
        workspace_path: &Path,
        title: &str,
        sections: &[Value],
        format: &str,
        unmask_map: &HashMap<String, String>,
        product_name: Option<&str>,
    ) -> Result<ReportGenOutput> {
        let generated = crate::llm::tool_executor::generate_report_bytes_core(
            workspace_path,
            title,
            sections,
            format,
            unmask_map,
            product_name,
            Some((&self.python_binary, self.python_home.as_ref())),
        )
        .await?;
        Ok(generated)
    }

    fn get_pii_unmask_map(&self, conversation_id: &str) -> HashMap<String, String> {
        crate::llm::tool_executor::file_load::get_pii_unmask_map(&self.storage, conversation_id)
    }

    async fn get_product_name(&self) -> Option<String> {
        let auth = self.auth_manager.as_ref()?;
        auth.get_auth_info()
            .await
            .tenant
            .and_then(|tenant| tenant.product_name.filter(|name| !name.is_empty()))
    }

    async fn persist_file(
        &self,
        conversation_id: &str,
        bytes: &[u8],
        extension: &str,
        title: &str,
        _actual_format: &str,
    ) -> Result<PersistedFileInfo> {
        let file_name = format!(
            "report_{}_{}.{}",
            crate::llm::tool_executor::slugify(title),
            Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
            extension,
        );

        let file_info = self.file_manager.write_file("reports", &file_name, bytes)?;
        let file_id = Uuid::new_v4().to_string();

        if let Err(e) = self.storage.insert_generated_file(
            &file_id,
            conversation_id,
            None,
            &file_info.file_name,
            &file_info.stored_path,
            &file_info.file_type,
            file_info.file_size as i64,
            "report",
            Some(title),
            1,
            true,
            None,
            None,
            None,
        ) {
            let _ = std::fs::remove_file(self.file_manager.full_path(&file_info.stored_path));
            return Err(e.into());
        }

        Ok(PersistedFileInfo {
            file_id,
            file_name: file_info.file_name,
            stored_path: file_info.stored_path,
            file_size: file_info.file_size,
        })
    }
}

pub fn build_file_gen_result(
    content: String,
    persisted: PersistedFileInfo,
    requested_format: &str,
    _actual_format: &str,
    is_degraded: bool,
    degradation_notice: Option<String>,
) -> FileGenResult {
    FileGenResult {
        content,
        file_meta: FileMeta {
            file_id: persisted.file_id,
            file_name: persisted.file_name,
            requested_format: requested_format.to_string(),
            actual_format: if is_degraded {
                "html".to_string()
            } else {
                requested_format.to_string()
            },
            file_size: persisted.file_size,
            stored_path: persisted.stored_path,
            category: "report".to_string(),
        },
        is_degraded,
        degradation_notice,
    }
}
