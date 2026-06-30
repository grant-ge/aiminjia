use serde::Serialize;
use tauri::{Emitter, State};

use crate::runtime::dependencies::WorkspaceDependencies;
use crate::runtime::dependencies::{
    ManagedRuntimeManager, RuntimeDownloadOptions, RuntimeDownloadProgress,
    RuntimeDownloadProgressSink,
};
use std::sync::Arc;

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
        emit_runtime_operation(
            &app,
            &operation_id,
            "ensure",
            "manifest",
            "failed",
            Some(err.to_string()),
        );
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
    emit_runtime_operation(
        &app,
        &operation_id,
        "reinstall",
        "manifest",
        "started",
        None,
    );
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
        emit_runtime_operation(
            &app,
            &operation_id,
            "reinstall",
            "manifest",
            "failed",
            Some(err.to_string()),
        );
        err.to_string()
    })?;
    let health = runtime_health_from_manager(manager.inner()).map_err(|err| err.to_string())?;
    emit_runtime_operation(
        &app,
        &operation_id,
        "reinstall",
        "health",
        "completed",
        None,
    );
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

// ─── Managed runtime diagnostics ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnosticsPayload {
    /// "installed" | "none"
    pub active_resolver: String,
    pub installed_version: Option<String>,
    pub node: String,
    pub python: String,
    pub uv: String,
}

#[tauri::command]
pub async fn runtime_diagnostics(
    app: tauri::AppHandle,
) -> Result<RuntimeDiagnosticsPayload, String> {
    use crate::runtime::dependencies::RuntimeResolver;
    use tauri::Manager;

    let mgr_state = app
        .try_state::<ManagedRuntimeManager>()
        .ok_or_else(|| "RuntimeManager not registered".to_string())?;
    let mgr: ManagedRuntimeManager = mgr_state.inner().clone();

    let deps = mgr
        .workspace_dependencies()
        .map_err(|e| format!("resolve failed: {e}"))?;

    let active = if mgr.resolver().workspace_dependencies().is_ok() {
        "installed"
    } else {
        "none"
    };

    let node_v = diag_run_version(&deps.node)
        .await
        .unwrap_or_else(|| "unknown".into());
    let py_v = diag_run_version(&deps.python)
        .await
        .unwrap_or_else(|| "unknown".into());
    let uv_v = diag_run_version(&deps.uv)
        .await
        .unwrap_or_else(|| "unknown".into());

    let installed_version = diag_read_installed_version(&app);

    Ok(RuntimeDiagnosticsPayload {
        active_resolver: active.to_string(),
        installed_version,
        node: node_v,
        python: py_v,
        uv: uv_v,
    })
}

async fn diag_run_version(path: &std::path::Path) -> Option<String> {
    use crate::storage::process_ext::NoWindowExt;
    let fut = tokio::process::Command::new(path)
        .arg("--version")
        .no_window()
        .output();
    // Defensive timeout: a corrupted binary that hangs on --version would
    // otherwise block the entire diagnostics command until Tauri's IPC timeout.
    let out = tokio::time::timeout(std::time::Duration::from_secs(5), fut)
        .await
        .ok()?
        .ok()?;
    let merged = if out.stdout.is_empty() {
        out.stderr
    } else {
        out.stdout
    };
    Some(String::from_utf8_lossy(&merged).trim().to_string())
}

fn diag_read_installed_version(app: &tauri::AppHandle) -> Option<String> {
    use tauri::Manager;
    let home = app.try_state::<crate::storage::aijia_home::AiJiaHome>()?;
    let pointer = home
        .root()
        .join("runtimes/renlijia-primary-runtime/current");
    let content = std::fs::read_to_string(&pointer).ok()?;
    let trimmed = content.trim().trim_start_matches("versions/");
    Some(trimmed.to_string())
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn diagnostics_payload_serializes_active_resolver_and_versions_in_camelcase() {
        let payload = RuntimeDiagnosticsPayload {
            active_resolver: "installed".to_string(),
            installed_version: None,
            node: "v20.18.0".to_string(),
            python: "Python 3.12.7".to_string(),
            uv: "uv 0.4.27".to_string(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(
            json.contains("\"activeResolver\":\"installed\""),
            "missing activeResolver in {json}"
        );
        assert!(
            json.contains("\"installedVersion\":null"),
            "expected null installedVersion in {json}"
        );
    }
}
