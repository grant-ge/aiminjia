//! Tracks which employees currently have a dispatched run in flight.
//!
//! `dispatch_employee_run` registers (employee_id → conversation_id) when it
//! spawns the agent loop. The detached task removes the entry on completion
//! (success or failure). UI calls `lookup` (via the Tauri command added in
//! Task 3) to know whether to render the "stop running" button vs. the
//! "dispatch" button.
//!
//! This is the single source of truth for the Activity dimension of the
//! state machine (vs. the time-windowed inbox heuristic in EmployeeCard).

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRun {
    pub employee_id: String,
    pub conversation_id: String,
    pub started_at: DateTime<Utc>,
    pub trigger_kind: TriggerKindLabel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKindLabel {
    OnDemand,
    Cron,
}

#[derive(Default, Debug)]
pub struct EmployeeActiveRuns {
    inner: Mutex<HashMap<String, ActiveRun>>,
}

impl EmployeeActiveRuns {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, run: ActiveRun) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert(run.employee_id.clone(), run);
        }
    }

    pub fn unregister(&self, employee_id: &str) -> Option<ActiveRun> {
        self.inner
            .lock()
            .ok()
            .and_then(|mut g| g.remove(employee_id))
    }

    pub fn lookup(&self, employee_id: &str) -> Option<ActiveRun> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.get(employee_id).cloned())
    }

    #[allow(dead_code)]
    pub fn list_all(&self) -> Vec<ActiveRun> {
        self.inner
            .lock()
            .ok()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(id: &str, conv: &str) -> ActiveRun {
        ActiveRun {
            employee_id: id.to_string(),
            conversation_id: conv.to_string(),
            started_at: Utc::now(),
            trigger_kind: TriggerKindLabel::OnDemand,
        }
    }

    #[test]
    fn register_and_lookup_round_trip() {
        let runs = EmployeeActiveRuns::new();
        runs.register(fixture("emp-1", "conv-a"));
        let got = runs.lookup("emp-1").unwrap();
        assert_eq!(got.conversation_id, "conv-a");
    }

    #[test]
    fn lookup_missing_returns_none() {
        let runs = EmployeeActiveRuns::new();
        assert!(runs.lookup("emp-x").is_none());
    }

    #[test]
    fn unregister_clears() {
        let runs = EmployeeActiveRuns::new();
        runs.register(fixture("emp-1", "conv-a"));
        let removed = runs.unregister("emp-1");
        assert_eq!(removed.unwrap().conversation_id, "conv-a");
        assert!(runs.lookup("emp-1").is_none());
    }

    #[test]
    fn second_register_overwrites_first() {
        let runs = EmployeeActiveRuns::new();
        runs.register(fixture("emp-1", "conv-a"));
        runs.register(fixture("emp-1", "conv-b"));
        assert_eq!(runs.lookup("emp-1").unwrap().conversation_id, "conv-b");
    }
}
