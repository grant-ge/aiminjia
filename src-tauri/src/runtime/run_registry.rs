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

enum RunEntry {
    Running(ActiveRun),
    SuspendedForHuman {
        run: ActiveRun,
        interaction_id: String,
    },
}

impl RunEntry {
    fn run(&self) -> &ActiveRun {
        match self {
            RunEntry::Running(run) => run,
            RunEntry::SuspendedForHuman { run, .. } => run,
        }
    }

    fn run_mut(&mut self) -> &mut ActiveRun {
        match self {
            RunEntry::Running(run) => run,
            RunEntry::SuspendedForHuman { run, .. } => run,
        }
    }

    fn into_run(self) -> ActiveRun {
        match self {
            RunEntry::Running(run) => run,
            RunEntry::SuspendedForHuman { run, .. } => run,
        }
    }

    fn is_running(&self) -> bool {
        matches!(self, RunEntry::Running(_))
    }
}

/// RuntimeRunRegistry only tracks stream-level runtime metadata:
/// 1. session -> active run_id mapping
/// 2. provider stream cancel watch channel
/// 3. busy session queries
///
/// Session / turn / tool cancellation ownership lives in `SessionRuntime`.
#[derive(Default)]
pub struct RuntimeRunRegistry {
    active_runs: Mutex<HashMap<String, RunEntry>>,
}

impl RuntimeRunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn active_runs(&self) -> MutexGuard<'_, HashMap<String, RunEntry>> {
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
        if let Some(existing) = active_runs.get(session_id) {
            match existing {
                RunEntry::Running(existing) => {
                    if !*existing.cancel.borrow() {
                        return Err("This conversation is already processing.".to_string());
                    }
                    log::info!(
                        "RuntimeRunRegistry replacing cancelled stale run: session_id={}, old_run_id={}, new_run_id={}",
                        session_id,
                        existing.run_id.as_str(),
                        run_id.as_str()
                    );
                    active_runs.remove(session_id);
                }
                RunEntry::SuspendedForHuman { run, .. } => {
                    log::info!(
                        "RuntimeRunRegistry replacing suspended run with new turn: session_id={}, old_run_id={}, new_run_id={}",
                        session_id,
                        run.run_id.as_str(),
                        run_id.as_str()
                    );
                    active_runs.remove(session_id);
                }
            }
        }
        if active_runs
            .values()
            .filter(|entry| entry.is_running())
            .count()
            >= MAX_CONCURRENT_AGENTS
        {
            return Err(format!(
                "Maximum concurrent conversations reached ({}). Please wait.",
                MAX_CONCURRENT_AGENTS
            ));
        }
        let (cancel_tx, _) = watch::channel(false);
        active_runs.insert(
            session_id.to_string(),
            RunEntry::Running(ActiveRun {
                task_id: format!("pre-{}", uuid::Uuid::new_v4()),
                run_id,
                cancel: cancel_tx,
                started_at: Instant::now(),
            }),
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
            let run = existing.run_mut();
            if *run.cancel.borrow() {
                anyhow::bail!("Conversation cancelled before stream started");
            }
            run.task_id = task_id;
            run.started_at = Instant::now();
            return Ok(run.cancel.subscribe());
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        active_runs.insert(
            session_id.to_string(),
            RunEntry::Running(ActiveRun {
                task_id,
                run_id: RunId::new(format!("legacy-{session_id}")),
                cancel: cancel_tx,
                started_at: Instant::now(),
            }),
        );
        Ok(cancel_rx)
    }

    pub fn cancel(&self, session_id: &str) {
        let active_runs = self.active_runs();
        if let Some(entry) = active_runs.get(session_id) {
            let run = entry.run();
            let _ = run.cancel.send_replace(true);
        }
    }

    pub fn clear(&self, session_id: &str) -> Option<RunId> {
        self.active_runs()
            .remove(session_id)
            .map(|entry| entry.into_run().run_id)
    }

    pub fn clear_for_run(&self, session_id: &str, run_id: &RunId) -> Option<RunId> {
        let mut active_runs = self.active_runs();
        let should_clear = active_runs
            .get(session_id)
            .map(|entry| &entry.run().run_id == run_id)
            .unwrap_or(false);
        if should_clear {
            active_runs
                .remove(session_id)
                .map(|entry| entry.into_run().run_id)
        } else {
            None
        }
    }

    pub fn is_busy(&self) -> bool {
        self.active_runs().values().any(|entry| entry.is_running())
    }

    pub fn is_session_busy(&self, session_id: &str) -> bool {
        self.active_runs()
            .get(session_id)
            .map(|entry| entry.is_running())
            .unwrap_or(false)
    }

    pub fn busy_sessions(&self) -> Vec<String> {
        self.active_runs()
            .iter()
            .filter_map(|(session_id, entry)| entry.is_running().then(|| session_id.clone()))
            .collect()
    }

    pub fn run_id_for_session(&self, session_id: &str) -> Option<RunId> {
        self.active_runs()
            .get(session_id)
            .map(|entry| entry.run().run_id.clone())
    }

    pub fn is_cancelled(&self, session_id: &str) -> bool {
        self.active_runs()
            .get(session_id)
            .map(|entry| *entry.run().cancel.borrow())
            .unwrap_or(false)
    }

    pub fn suspend_for_human(
        &self,
        session_id: &str,
        interaction_id: impl Into<String>,
    ) -> Result<(), String> {
        let mut active_runs = self.active_runs();
        let Some(entry) = active_runs.remove(session_id) else {
            return Err("No active run to suspend.".to_string());
        };
        let run = entry.into_run();
        active_runs.insert(
            session_id.to_string(),
            RunEntry::SuspendedForHuman {
                run,
                interaction_id: interaction_id.into(),
            },
        );
        Ok(())
    }

    pub fn resume_from_human(&self, session_id: &str) -> Result<(), String> {
        let mut active_runs = self.active_runs();
        let Some(entry) = active_runs.remove(session_id) else {
            return Err("No suspended run to resume.".to_string());
        };
        match entry {
            RunEntry::SuspendedForHuman { run, .. } => {
                active_runs.insert(session_id.to_string(), RunEntry::Running(run));
                Ok(())
            }
            RunEntry::Running(run) => {
                active_runs.insert(session_id.to_string(), RunEntry::Running(run));
                Err("Run is not suspended for human interaction.".to_string())
            }
        }
    }

    pub fn is_session_suspended_for_human(&self, session_id: &str) -> bool {
        matches!(
            self.active_runs().get(session_id),
            Some(RunEntry::SuspendedForHuman { .. })
        )
    }

    pub fn suspended_interaction_id(&self, session_id: &str) -> Option<String> {
        match self.active_runs().get(session_id) {
            Some(RunEntry::SuspendedForHuman { interaction_id, .. }) => {
                Some(interaction_id.clone())
            }
            _ => None,
        }
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

    #[test]
    fn suspended_for_human_is_not_busy_but_keeps_run_identity() {
        let registry = RuntimeRunRegistry::new();
        let run_id = RunId::new("run-human");

        registry.reserve("sess", run_id.clone()).unwrap();
        registry.suspend_for_human("sess", "interaction-1").unwrap();

        assert!(!registry.is_session_busy("sess"));
        assert!(registry.is_session_suspended_for_human("sess"));
        assert_eq!(registry.run_id_for_session("sess").unwrap(), run_id);
        assert_eq!(
            registry.suspended_interaction_id("sess").as_deref(),
            Some("interaction-1")
        );
    }

    #[test]
    fn resume_from_human_reacquires_busy_for_same_run() {
        let registry = RuntimeRunRegistry::new();
        let run_id = RunId::new("run-human");

        registry.reserve("sess", run_id.clone()).unwrap();
        registry.suspend_for_human("sess", "interaction-1").unwrap();
        registry.resume_from_human("sess").unwrap();

        assert!(registry.is_session_busy("sess"));
        assert!(!registry.is_session_suspended_for_human("sess"));
        assert_eq!(registry.run_id_for_session("sess").unwrap(), run_id);
    }
}
