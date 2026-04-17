use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use tokio::sync::watch;

use crate::llm::gateway::MAX_CONCURRENT_AGENTS;
use crate::runtime::ids::RunId;

struct ActiveRun {
    task_id: String,
    run_id: RunId,
    cancel: watch::Sender<bool>,
    started_at: Instant,
}

#[derive(Default)]
pub struct RuntimeRunRegistry {
    active_runs: Mutex<HashMap<String, ActiveRun>>,
}

impl RuntimeRunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn active_runs(&self) -> MutexGuard<'_, HashMap<String, ActiveRun>> {
        match self.active_runs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("RuntimeRunRegistry mutex poisoned; recovering state");
                poisoned.into_inner()
            }
        }
    }

    pub fn reserve(&self, session_id: &str, run_id: RunId) -> Result<(), String> {
        let mut active_runs = self.active_runs();
        if active_runs.contains_key(session_id) {
            return Err("This conversation is already processing.".to_string());
        }
        if active_runs.len() >= MAX_CONCURRENT_AGENTS {
            return Err(format!(
                "Maximum concurrent conversations reached ({}). Please wait.",
                MAX_CONCURRENT_AGENTS
            ));
        }
        let (cancel_tx, _) = watch::channel(false);
        active_runs.insert(
            session_id.to_string(),
            ActiveRun {
                task_id: format!("pre-{}", uuid::Uuid::new_v4()),
                run_id,
                cancel: cancel_tx,
                started_at: Instant::now(),
            },
        );
        Ok(())
    }

    pub fn attach_stream(
        &self,
        session_id: &str,
        task_id: String,
    ) -> anyhow::Result<watch::Receiver<bool>> {
        let mut active_runs = self.active_runs();
        if let Some(existing) = active_runs.get_mut(session_id) {
            if *existing.cancel.borrow() {
                anyhow::bail!("Conversation cancelled before stream started");
            }
            existing.task_id = task_id;
            existing.started_at = Instant::now();
            return Ok(existing.cancel.subscribe());
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        active_runs.insert(
            session_id.to_string(),
            ActiveRun {
                task_id,
                run_id: RunId::new(format!("legacy-{session_id}")),
                cancel: cancel_tx,
                started_at: Instant::now(),
            },
        );
        Ok(cancel_rx)
    }

    pub fn cancel(&self, session_id: &str) {
        let active_runs = self.active_runs();
        if let Some(run) = active_runs.get(session_id) {
            let _ = run.cancel.send_replace(true);
        }
    }

    pub fn clear(&self, session_id: &str) -> Option<RunId> {
        self.active_runs().remove(session_id).map(|run| run.run_id)
    }

    pub fn is_busy(&self) -> bool {
        !self.active_runs().is_empty()
    }

    pub fn is_session_busy(&self, session_id: &str) -> bool {
        self.active_runs().contains_key(session_id)
    }

    pub fn busy_sessions(&self) -> Vec<String> {
        self.active_runs().keys().cloned().collect()
    }

    pub fn run_id_for_session(&self, session_id: &str) -> Option<RunId> {
        self.active_runs()
            .get(session_id)
            .map(|run| run.run_id.clone())
    }

    pub fn is_cancelled(&self, session_id: &str) -> bool {
        self.active_runs()
            .get(session_id)
            .map(|run| *run.cancel.borrow())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poison_registry(registry: &RuntimeRunRegistry) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = registry.active_runs.lock().unwrap();
            panic!("poison registry mutex");
        }));
    }

    #[test]
    fn reserve_recovers_after_mutex_poison() {
        let registry = RuntimeRunRegistry::new();
        poison_registry(&registry);

        registry
            .reserve("conv-poison", RunId::new("run-poison"))
            .expect("reserve should recover from poison");

        assert!(registry.is_session_busy("conv-poison"));
        assert_eq!(
            registry
                .run_id_for_session("conv-poison")
                .expect("run id should be present")
                .as_str(),
            "run-poison"
        );
    }

    #[test]
    fn read_write_operations_recover_after_mutex_poison() {
        let registry = RuntimeRunRegistry::new();
        registry
            .reserve("conv-existing", RunId::new("run-existing"))
            .expect("initial reserve should succeed");
        poison_registry(&registry);

        registry.cancel("conv-existing");
        assert!(registry.is_cancelled("conv-existing"));
        assert!(registry.is_busy());
        assert_eq!(registry.busy_sessions(), vec!["conv-existing".to_string()]);

        let cleared = registry.clear("conv-existing");
        assert_eq!(
            cleared.expect("clear should recover from poison").as_str(),
            "run-existing"
        );
        assert!(!registry.is_busy());
    }
}
