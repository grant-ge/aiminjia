use crate::runtime::ids::RunId;

/// Lightweight record returned after a background child run is spawned.
/// Used to track the parent→child relationship so the sub-agent completion
/// path can route the summary back through the message bridge.
#[derive(Clone, Debug)]
pub struct BackgroundRun {
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
}

impl BackgroundRun {
    pub fn new(parent_run_id: RunId, child_run_id: RunId) -> Self {
        Self {
            parent_run_id,
            child_run_id,
        }
    }
}
