use std::sync::Arc;

use crate::runtime::ids::SessionId;
use crate::runtime::interaction::PendingInteractionControlPlane;
use crate::runtime::store::PendingPermissionControlPlane;

use super::{
    HumanInteractionId, HumanInteractionKind, HumanInteractionRef, HumanInteractionStatus,
};

pub struct HumanInteractionControlPlane {
    interactions: Arc<dyn PendingInteractionControlPlane>,
    permissions: Arc<dyn PendingPermissionControlPlane>,
}

impl HumanInteractionControlPlane {
    pub fn new(
        interactions: Arc<dyn PendingInteractionControlPlane>,
        permissions: Arc<dyn PendingPermissionControlPlane>,
    ) -> Self {
        Self {
            interactions,
            permissions,
        }
    }

    pub fn pending_for_session(&self, session_id: &str) -> Vec<HumanInteractionRef> {
        let mut refs = Vec::new();
        refs.extend(
            self.interactions
                .pending_for_session(session_id)
                .into_iter()
                .map(|req| HumanInteractionRef {
                    id: HumanInteractionId::new(req.interaction_id.as_str().to_string()),
                    session_id: req.session_id,
                    run_id: req.run_id,
                    tool_call_id: req.tool_call_id,
                    kind: HumanInteractionKind::AskUserQuestion,
                    turn_origin: req.turn_origin,
                    output_binding: req.output_binding,
                    status: HumanInteractionStatus::Pending,
                }),
        );
        refs.extend(
            self.permissions
                .pending_for_session(&SessionId::new(session_id.to_string()))
                .into_iter()
                .map(|req| HumanInteractionRef {
                    id: HumanInteractionId::new(req.tool_call_id.as_str().to_string()),
                    session_id: req.session_id,
                    run_id: req.run_id,
                    tool_call_id: req.tool_call_id,
                    kind: HumanInteractionKind::PermissionAsk,
                    turn_origin: req.turn_origin,
                    output_binding: req.output_binding,
                    status: HumanInteractionStatus::Pending,
                }),
        );
        refs
    }
}
