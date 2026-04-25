//! PendingInteractionControlPlane — async wait/resolve for user interactions.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use tokio::sync::oneshot;

use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

use super::types::{InteractionId, InteractionRequest, InteractionResolution};

struct PendingEntry {
    request: InteractionRequest,
    resolution_tx: oneshot::Sender<InteractionResolution>,
}

pub trait PendingInteractionControlPlane: Send + Sync {
    fn insert_pending(
        &self,
        request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionResolution>>;

    fn resolve(
        &self,
        interaction_id: &InteractionId,
        resolution: InteractionResolution,
    ) -> Result<()>;

    fn cancel_for_session(&self, session_id: &str, message: &str) -> usize;

    fn pending_count_for_session(&self, session_id: &str) -> usize;
}

#[derive(Default)]
pub struct InMemoryInteractionControlPlane {
    inner: Mutex<HashMap<String, PendingEntry>>,
}

impl InMemoryInteractionControlPlane {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PendingInteractionControlPlane for InMemoryInteractionControlPlane {
    fn insert_pending(
        &self,
        request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionResolution>> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = request.interaction_id.as_str().to_string();
        if inner.contains_key(&key) {
            return Err(anyhow!(
                "pending interaction already exists for id: {}",
                key
            ));
        }
        let (tx, rx) = oneshot::channel();
        inner.insert(
            key.clone(),
            PendingEntry {
                request,
                resolution_tx: tx,
            },
        );
        let entry = inner.get(&key).unwrap();
        record_diagnostic(
            &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            DiagnosticEvent::new("interaction.required.received", DiagnosticSource::Backend)
                .conversation_id(entry.request.session_id.as_str())
                .run_id(entry.request.run_id.as_str())
                .tool_call_id(entry.request.tool_call_id.as_str())
                .interaction_id(entry.request.interaction_id.as_str())
                .payload(serde_json::json!({
                    "toolName": entry.request.tool_name,
                })),
        );
        Ok(rx)
    }

    fn resolve(
        &self,
        interaction_id: &InteractionId,
        resolution: InteractionResolution,
    ) -> Result<()> {
        let entry = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(interaction_id.as_str())
            .ok_or_else(|| anyhow!("pending interaction not found: {}", interaction_id))?;
        record_diagnostic(
            &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            DiagnosticEvent::new("interaction.resolve.completed", DiagnosticSource::Backend)
                .conversation_id(entry.request.session_id.as_str())
                .run_id(entry.request.run_id.as_str())
                .tool_call_id(entry.request.tool_call_id.as_str())
                .interaction_id(entry.request.interaction_id.as_str())
                .payload(serde_json::json!({
                    "toolName": entry.request.tool_name,
                    "resolution": match resolution {
                        InteractionResolution::Submit { .. } => "submit",
                        InteractionResolution::Cancel { .. } => "cancel",
                    },
                })),
        );
        entry
            .resolution_tx
            .send(resolution)
            .map_err(|_| anyhow!("receiver dropped for interaction: {}", interaction_id))
    }

    fn cancel_for_session(&self, session_id: &str, message: &str) -> usize {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let to_cancel: Vec<String> = inner
            .iter()
            .filter(|(_, entry)| entry.request.session_id.as_str() == session_id)
            .map(|(key, _)| key.clone())
            .collect();
        let count = to_cancel.len();
        for key in to_cancel {
            if let Some(entry) = inner.remove(&key) {
                let _ = entry.resolution_tx.send(InteractionResolution::Cancel {
                    message: message.to_string(),
                });
            }
        }
        count
    }

    fn pending_count_for_session(&self, session_id: &str) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .filter(|entry| entry.request.session_id.as_str() == session_id)
            .count()
    }
}
