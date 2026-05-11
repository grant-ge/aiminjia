use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use crate::runtime::ids::{RunId, SessionId, ToolCallId};
use crate::runtime::tools::permission::{PermissionDestination, PermissionMode};

#[derive(Clone, Debug)]
pub struct PendingPermissionRequest {
    pub tool_call_id: ToolCallId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_name: String,
    pub capability_scopes: Vec<String>,
    pub message: String,
    pub suggestions: Vec<String>,
    pub mode: PermissionMode,
    pub remember_options: Vec<PermissionDestination>,
    pub default_destination: Option<PermissionDestination>,
    pub original_request: RuntimeToolCallRequest,
    /// Encodes path-auth persistence kind for §7.8 routing.
    /// - `Some("path:<canonical>")` → step-6 Ask → append_working_dir on "永久"
    /// - `Some("pathwrite:<canonical>")` → step-4b write Ask → append_path_allow_rule(Write)
    /// - `None` → non-path Ask
    pub path_auth_scope: Option<String>,
}

#[derive(Clone, Debug)]
pub enum PendingPermissionResolution {
    Allow {
        updated_input: Option<Value>,
        remember: bool,
        destination: Option<PermissionDestination>,
    },
    Deny {
        message: String,
        remember: bool,
        destination: Option<PermissionDestination>,
    },
    Cancel {
        message: String,
    },
}

struct PendingPermissionEntry {
    request: PendingPermissionRequest,
    resolution_tx: oneshot::Sender<PendingPermissionResolution>,
}

pub trait PendingPermissionControlPlane: Send + Sync {
    fn insert_pending_request(
        &self,
        request: PendingPermissionRequest,
    ) -> Result<oneshot::Receiver<PendingPermissionResolution>>;

    fn resolve_pending_request(
        &self,
        tool_call_id: &ToolCallId,
        resolution: PendingPermissionResolution,
    ) -> Result<()>;

    fn cancel_for_session(&self, session_id: &SessionId, message: &str) -> usize;

    fn pending_count_for_session(&self, session_id: &SessionId) -> usize;

    fn is_pending(&self, tool_call_id: &ToolCallId) -> bool;
}

#[derive(Default)]
pub struct PendingPermissionRequestStore {
    inner: Mutex<HashMap<String, PendingPermissionEntry>>,
}

impl PendingPermissionRequestStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &self,
        request: PendingPermissionRequest,
    ) -> Result<oneshot::Receiver<PendingPermissionResolution>> {
        let mut inner = self.inner.lock().unwrap();
        let key = request.tool_call_id.as_str().to_string();
        if inner.contains_key(&key) {
            return Err(anyhow!(
                "pending permission request already exists for tool_call_id: {}",
                key
            ));
        }

        let (resolution_tx, resolution_rx) = oneshot::channel();
        inner.insert(
            key,
            PendingPermissionEntry {
                request,
                resolution_tx,
            },
        );
        Ok(resolution_rx)
    }

    pub fn get(&self, tool_call_id: &ToolCallId) -> Option<PendingPermissionRequest> {
        self.inner
            .lock()
            .unwrap()
            .get(tool_call_id.as_str())
            .map(|entry| entry.request.clone())
    }

    pub fn resolve(
        &self,
        tool_call_id: &ToolCallId,
        resolution: PendingPermissionResolution,
    ) -> Result<()> {
        let entry = self
            .inner
            .lock()
            .unwrap()
            .remove(tool_call_id.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "pending permission request not found: {}",
                    tool_call_id.as_str()
                )
            })?;

        entry
            .resolution_tx
            .send(resolution)
            .map_err(|_| anyhow!("failed to deliver permission resolution"))?;
        Ok(())
    }

    pub fn cancel_for_session(&self, session_id: &SessionId, message: &str) -> usize {
        let ids: Vec<ToolCallId> = self
            .inner
            .lock()
            .unwrap()
            .values()
            .filter(|entry| entry.request.session_id == *session_id)
            .map(|entry| entry.request.tool_call_id.clone())
            .collect();

        let mut cancelled = 0usize;
        for id in ids {
            if self
                .resolve(
                    &id,
                    PendingPermissionResolution::Cancel {
                        message: message.to_string(),
                    },
                )
                .is_ok()
            {
                cancelled += 1;
            }
        }
        cancelled
    }

    pub fn pending_count_for_session(&self, session_id: &SessionId) -> usize {
        self.inner
            .lock()
            .unwrap()
            .values()
            .filter(|entry| entry.request.session_id == *session_id)
            .count()
    }
}

impl PendingPermissionControlPlane for PendingPermissionRequestStore {
    fn insert_pending_request(
        &self,
        request: PendingPermissionRequest,
    ) -> Result<oneshot::Receiver<PendingPermissionResolution>> {
        self.insert(request)
    }

    fn resolve_pending_request(
        &self,
        tool_call_id: &ToolCallId,
        resolution: PendingPermissionResolution,
    ) -> Result<()> {
        self.resolve(tool_call_id, resolution)
    }

    fn cancel_for_session(&self, session_id: &SessionId, message: &str) -> usize {
        PendingPermissionRequestStore::cancel_for_session(self, session_id, message)
    }

    fn pending_count_for_session(&self, session_id: &SessionId) -> usize {
        PendingPermissionRequestStore::pending_count_for_session(self, session_id)
    }

    fn is_pending(&self, tool_call_id: &ToolCallId) -> bool {
        self.get(tool_call_id).is_some()
    }
}
