use serde::Serialize;
use tauri::{Emitter, State};

use crate::runtime::dependencies::{
    ManagedRuntimeManager, RuntimeDownloadOptions, RuntimeDownloadProgress,
    RuntimeDownloadProgressSink,
};
use std::sync::Arc;
use crate::runtime::dependencies::WorkspaceDependencies;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolHealthPayload {
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealthPayload {
    pub bundle_version: String,
    pub node: Option<RuntimeToolHealthPayload>,
    pub npm: Option<RuntimeToolHealthPayload>,
    pub npx: Option<RuntimeToolHealthPayload>,
    pub python: Option<RuntimeToolHealthPayload>,
    pub uv: Option<RuntimeToolHealthPayload>,
    pub uvx: Option<RuntimeToolHealthPayload>,
}


#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOperationProgressPayload {
    pub operation_id: String,
    pub kind: String,
    pub phase: String,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub percent: Option<f64>,
    pub attempt: u8,
    pub max_attempts: u8,
    pub resumed: bool,
    pub status: String,
    pub message: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCleanupPayload {
    pub removed_versions: Vec<String>,
    pub kept_versions: Vec<String>,
}

#[tauri::command]
pub async fn runtime_get_health(
    manager: State<'_, ManagedRuntimeManager>,
) -> Result<RuntimeHealthPayload, String> {
    runtime_health_from_manager(manager.inner()).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn runtime_ensure(
    app: tauri::AppHandle,
    manager: State<'_, ManagedRuntimeManager>,
) -> Result<RuntimeHealthPayload, String> {
    let operation_id = uuid::Uuid::new_v4().to_string();
    let cancellation = manager
        .begin_operation(operation_id.clone())
        .map_err(|err| err.to_string())?;
    emit_runtime_operation(&app, &operation_id, "ensure", "manifest", "started", None);
    let result = manager
        .ensure_managed_with_download_options(RuntimeDownloadOptions {
            cancellation,
            progress: Some(Arc::new(TauriRuntimeProgressSink::new(
                app.clone(),
                operation_id.clone(),
                "ensure",
            ))),
            ..RuntimeDownloadOptions::default()
        })
        .await;
    manager.finish_operation(&operation_id);
    result.map_err(|err| {
        emit_runtime_operation(&app, &operation_id, "ensure", "manifest", "failed", Some(err.to_string()));
        err.to_string()
    })?;
    let health = runtime_health_from_manager(manager.inner()).map_err(|err| err.to_string())?;
    emit_runtime_operation(&app, &operation_id, "ensure", "health", "completed", None);
    Ok(health)
}

#[tauri::command]
pub async fn runtime_reinstall(
    app: tauri::AppHandle,
    manager: State<'_, ManagedRuntimeManager>,
) -> Result<RuntimeHealthPayload, String> {
    let operation_id = uuid::Uuid::new_v4().to_string();
    let cancellation = manager
        .begin_operation(operation_id.clone())
        .map_err(|err| err.to_string())?;
    emit_runtime_operation(&app, &operation_id, "reinstall", "manifest", "started", None);
    let result = manager
        .reinstall_managed_with_download_options(RuntimeDownloadOptions {
            cancellation,
            progress: Some(Arc::new(TauriRuntimeProgressSink::new(
                app.clone(),
                operation_id.clone(),
                "reinstall",
            ))),
            ..RuntimeDownloadOptions::default()
        })
        .await;
    manager.finish_operation(&operation_id);
    result.map_err(|err| {
        emit_runtime_operation(&app, &operation_id, "reinstall", "manifest", "failed", Some(err.to_string()));
        err.to_string()
    })?;
    let health = runtime_health_from_manager(manager.inner()).map_err(|err| err.to_string())?;
    emit_runtime_operation(&app, &operation_id, "reinstall", "health", "completed", None);
    Ok(health)
}


#[tauri::command]
pub async fn runtime_cleanup_old_versions(
    manager: State<'_, ManagedRuntimeManager>,
    keep_versions: usize,
) -> Result<RuntimeCleanupPayload, String> {
    let result = manager
        .cleanup_old_versions(keep_versions)
        .map_err(|err| err.to_string())?;
    Ok(RuntimeCleanupPayload {
        removed_versions: result.removed_versions,
        kept_versions: result.kept_versions,
    })
}

#[tauri::command]
pub async fn runtime_cancel_operation(
    manager: State<'_, ManagedRuntimeManager>,
    operation_id: String,
) -> Result<bool, String> {
    Ok(manager.cancel_operation(&operation_id))
}

fn emit_runtime_operation(
    app: &tauri::AppHandle,
    operation_id: &str,
    kind: &str,
    phase: &str,
    status: &str,
    error: Option<String>,
) {
    let payload = RuntimeOperationProgressPayload {
        operation_id: operation_id.to_string(),
        kind: kind.to_string(),
        phase: phase.to_string(),
        downloaded_bytes: None,
        total_bytes: None,
        percent: None,
        attempt: 1,
        max_attempts: 1,
        resumed: false,
        status: status.to_string(),
        message: None,
        error,
    };
    let _ = app.emit("runtime:operation-progress", payload);
}


#[derive(Clone)]
struct TauriRuntimeProgressSink {
    app: tauri::AppHandle,
    operation_id: String,
    kind: String,
}

impl TauriRuntimeProgressSink {
    fn new(app: tauri::AppHandle, operation_id: String, kind: &str) -> Self {
        Self {
            app,
            operation_id,
            kind: kind.to_string(),
        }
    }
}

impl RuntimeDownloadProgressSink for TauriRuntimeProgressSink {
    fn on_progress(&self, progress: RuntimeDownloadProgress) {
        let percent = progress.total_bytes.and_then(|total| {
            if total == 0 {
                None
            } else {
                Some((progress.downloaded_bytes as f64 / total as f64) * 100.0)
            }
        });
        let payload = RuntimeOperationProgressPayload {
            operation_id: self.operation_id.clone(),
            kind: self.kind.clone(),
            phase: "download".to_string(),
            downloaded_bytes: Some(progress.downloaded_bytes),
            total_bytes: progress.total_bytes,
            percent,
            attempt: progress.attempt,
            max_attempts: progress.max_attempts,
            resumed: progress.resumed,
            status: "progress".to_string(),
            message: None,
            error: None,
        };
        let _ = self.app.emit("runtime:operation-progress", payload);
    }

    fn on_retry(&self, attempt: u8, max_attempts: u8, message: &str) {
        let payload = RuntimeOperationProgressPayload {
            operation_id: self.operation_id.clone(),
            kind: self.kind.clone(),
            phase: "download".to_string(),
            downloaded_bytes: None,
            total_bytes: None,
            percent: None,
            attempt,
            max_attempts,
            resumed: false,
            status: "retrying".to_string(),
            message: Some(message.to_string()),
            error: None,
        };
        let _ = self.app.emit("runtime:operation-progress", payload);
    }
}

fn runtime_health_from_manager(
    manager: &ManagedRuntimeManager,
) -> Result<RuntimeHealthPayload, crate::runtime::dependencies::RuntimeManagerError> {
    let deps = manager.dependencies()?;
    let report = manager.health()?;
    Ok(RuntimeHealthPayload {
        bundle_version: manager.bundle_version().to_string(),
        node: Some(tool_payload(&report, "node", deps.node)),
        npm: Some(tool_payload(&report, "npm", deps.npm)),
        npx: Some(tool_payload(&report, "npx", deps.npx)),
        python: Some(tool_payload(&report, "python", deps.python)),
        uv: Some(tool_payload(&report, "uv", deps.uv)),
        uvx: Some(tool_payload(&report, "uvx", deps.uvx)),
    })
}

fn tool_payload(
    report: &crate::runtime::dependencies::RuntimeHealthReport,
    name: &str,
    path: std::path::PathBuf,
) -> RuntimeToolHealthPayload {
    RuntimeToolHealthPayload {
        version: report.tool_version(name).unwrap_or("unknown").to_string(),
        path: path.display().to_string(),
    }
}

pub fn runtime_health_payload_from_dependencies(
    bundle_version: &str,
    deps: WorkspaceDependencies,
) -> RuntimeHealthPayload {
    RuntimeHealthPayload {
        bundle_version: bundle_version.to_string(),
        node: Some(RuntimeToolHealthPayload {
            version: "unknown".to_string(),
            path: deps.node.display().to_string(),
        }),
        npm: Some(RuntimeToolHealthPayload {
            version: "unknown".to_string(),
            path: deps.npm.display().to_string(),
        }),
        npx: Some(RuntimeToolHealthPayload {
            version: "unknown".to_string(),
            path: deps.npx.display().to_string(),
        }),
        python: Some(RuntimeToolHealthPayload {
            version: "unknown".to_string(),
            path: deps.python.display().to_string(),
        }),
        uv: Some(RuntimeToolHealthPayload {
            version: "unknown".to_string(),
            path: deps.uv.display().to_string(),
        }),
        uvx: Some(RuntimeToolHealthPayload {
            version: "unknown".to_string(),
            path: deps.uvx.display().to_string(),
        }),
    }
}
