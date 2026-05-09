//! Narrow dependencies for agenda RuntimeTools.
//!
//! Each agenda tool receives `AgendaToolDeps` at construction time. The
//! `current_persona_id` field is bound here from
//! `RequestScopedRuntimeDeps.current_persona_id` (resolved by chat.rs main
//! path) so that LLM cannot forge organizer identity through tool input.

use std::path::PathBuf;
use std::sync::Arc;

use crate::runtime::agenda::AgendaStore;

pub struct AgendaToolDeps {
    pub store: Arc<AgendaStore>,
    pub current_persona_id: String,
}

impl AgendaToolDeps {
    pub fn new(base_dir: PathBuf, current_persona_id: String) -> Self {
        Self {
            store: Arc::new(AgendaStore::new(base_dir)),
            current_persona_id,
        }
    }
}
