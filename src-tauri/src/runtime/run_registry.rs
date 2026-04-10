use std::collections::HashMap;
use std::sync::Mutex;
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

    pub fn reserve(&self, session_id: &str, run_id: RunId) -> Result<(), String> {
        let mut active_runs = self.active_runs.lock().unwrap();
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
        let mut active_runs = self.active_runs.lock().unwrap();
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
        let active_runs = self.active_runs.lock().unwrap();
        if let Some(run) = active_runs.get(session_id) {
            let _ = run.cancel.send_replace(true);
        }
    }

    pub fn clear(&self, session_id: &str) -> Option<RunId> {
        self.active_runs
            .lock()
            .unwrap()
            .remove(session_id)
            .map(|run| run.run_id)
    }

    pub fn is_busy(&self) -> bool {
        !self.active_runs.lock().unwrap().is_empty()
    }

    pub fn is_session_busy(&self, session_id: &str) -> bool {
        self.active_runs.lock().unwrap().contains_key(session_id)
    }

    pub fn busy_sessions(&self) -> Vec<String> {
        self.active_runs.lock().unwrap().keys().cloned().collect()
    }

    pub fn run_id_for_session(&self, session_id: &str) -> Option<RunId> {
        self.active_runs
            .lock()
            .unwrap()
            .get(session_id)
            .map(|run| run.run_id.clone())
    }

    pub fn is_cancelled(&self, session_id: &str) -> bool {
        self.active_runs
            .lock()
            .unwrap()
            .get(session_id)
            .map(|run| *run.cancel.borrow())
            .unwrap_or(false)
    }
}
