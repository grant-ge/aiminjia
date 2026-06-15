use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{HumanInteractionId, HumanInteractionRef, HumanInteractionStatus, InboundUserMessage};

#[derive(Clone, Default)]
pub struct HumanInteractionRegistry {
    inner: Arc<Mutex<RegistryState>>,
}

#[derive(Default)]
struct RegistryState {
    live: HashMap<String, Vec<HumanInteractionRef>>,
    early: HashMap<String, Vec<InboundUserMessage>>,
}

impl HumanInteractionRegistry {
    pub fn register(&self, interaction: HumanInteractionRef) {
        let mut guard = self.inner.lock().expect("human interaction registry lock");
        guard
            .live
            .entry(interaction.session_id.as_str().to_string())
            .or_default()
            .push(interaction);
    }

    pub fn register_and_drain(&self, interaction: HumanInteractionRef) -> Vec<InboundUserMessage> {
        let session_id = interaction.session_id.as_str().to_string();
        let mut guard = self.inner.lock().expect("human interaction registry lock");
        guard
            .live
            .entry(session_id.clone())
            .or_default()
            .push(interaction);
        guard.early.remove(&session_id).unwrap_or_default()
    }

    pub fn latest_live_for_session(&self, session_id: &str) -> Option<HumanInteractionRef> {
        let guard = self.inner.lock().expect("human interaction registry lock");
        guard.live.get(session_id).and_then(|items| {
            items
                .iter()
                .rev()
                .find(|item| item.status == HumanInteractionStatus::Pending)
                .cloned()
        })
    }

    pub fn mark_resolved(&self, interaction_id: &HumanInteractionId) {
        self.mark_status(interaction_id, HumanInteractionStatus::Resolved);
    }

    pub fn mark_cancelled(&self, interaction_id: &HumanInteractionId) {
        self.mark_status(interaction_id, HumanInteractionStatus::Cancelled);
    }

    pub fn mark_abandoned(&self, interaction_id: &HumanInteractionId) {
        self.mark_status(interaction_id, HumanInteractionStatus::Abandoned);
    }

    pub fn buffer_early_message(&self, message: InboundUserMessage) {
        let mut guard = self.inner.lock().expect("human interaction registry lock");
        guard
            .early
            .entry(message.session_id.as_str().to_string())
            .or_default()
            .push(message);
    }

    pub fn take_early_messages(&self, session_id: &str) -> Vec<InboundUserMessage> {
        let mut guard = self.inner.lock().expect("human interaction registry lock");
        guard.early.remove(session_id).unwrap_or_default()
    }

    fn mark_status(&self, interaction_id: &HumanInteractionId, status: HumanInteractionStatus) {
        let mut guard = self.inner.lock().expect("human interaction registry lock");
        for items in guard.live.values_mut() {
            for item in items.iter_mut() {
                if item.id.as_str() == interaction_id.as_str() {
                    item.status = status;
                }
            }
        }
    }
}
