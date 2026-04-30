use crate::runtime::ids::{RunId, SessionId};

#[derive(Clone, Debug)]
pub struct IdentityMapping {
    pub session_id: SessionId,
    pub legacy_conversation_id: Option<String>,
}

impl IdentityMapping {
    pub fn from_legacy_conversation_id(conversation_id: impl Into<String>) -> Self {
        let conversation_id = conversation_id.into();
        Self {
            session_id: SessionId::new(conversation_id.clone()),
            legacy_conversation_id: Some(conversation_id),
        }
    }

    pub fn direct(session_id: SessionId) -> Self {
        Self {
            session_id,
            legacy_conversation_id: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeIdentity {
    session_id: SessionId,
    run_id: RunId,
}

impl RuntimeIdentity {
    pub fn new(session_id: SessionId, run_id: RunId) -> Self {
        Self { session_id, run_id }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }
}
