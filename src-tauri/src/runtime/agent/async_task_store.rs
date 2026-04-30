//! In-memory registry for named async sub-agents launched via
//! `spawn_subagent({ run_in_background: true, name: "..." })`.
//!
//! The store maps:
//!   name (String) → AgentId
//!   AgentId        → AsyncTaskHandle  (state + output_file + description)
//!   AgentId        → Vec<String>       (pending messages queued by parent)
//!
//! Terminal states (`Completed`, `Failed`, `Killed`) do **not** remove the
//! entry; callers (P8.1 task_output / SendMessage) need the final state.
//!
//! No `tauri::*` imports — pure runtime module.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::runtime::ids::AgentId;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Life-cycle states for an async sub-agent task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AsyncTaskState {
    Running,
    Backgrounded,
    Completed,
    Failed,
    Killed,
}

impl AsyncTaskState {
    /// Returns `true` for terminal states that exclude the task from
    /// [`AsyncAgentTaskStore::list_active`].
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

/// A snapshot of a registered async agent task.
#[derive(Clone, Debug)]
pub struct AsyncTaskHandle {
    pub agent_id: AgentId,
    pub state: AsyncTaskState,
    /// Path to the output file written by the sub-agent at completion.
    pub output_file: PathBuf,
    /// Human-readable description supplied at registration (e.g. the
    /// `name` / prompt fragment from `spawn_subagent`).
    pub description: String,
}

// ---------------------------------------------------------------------------
// Store implementation
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Inner {
    /// name → AgentId index
    by_name: HashMap<String, AgentId>,
    /// AgentId → handle (includes state)
    by_id: HashMap<AgentId, AsyncTaskHandle>,
    /// AgentId → pending messages from parent
    pending: HashMap<AgentId, Vec<String>>,
}

/// Thread-safe in-memory store for async sub-agent tasks.
///
/// `Clone` is cheap — all clones share the same underlying `Arc<Mutex<Inner>>`.
#[derive(Clone, Default)]
pub struct AsyncAgentTaskStore {
    inner: Arc<Mutex<Inner>>,
}

impl AsyncAgentTaskStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new async agent task under `name`.
    ///
    /// If a task with the same `name` or the same `AgentId` already exists it
    /// is silently overwritten (the caller is responsible for uniqueness).
    pub fn register(&self, name: &str, handle: AsyncTaskHandle) {
        let mut g = self.inner.lock().expect("async_task_store: lock poisoned");
        g.by_name.insert(name.to_owned(), handle.agent_id.clone());
        g.by_id.insert(handle.agent_id.clone(), handle);
    }

    /// Look up a task handle by its registered name.
    pub fn find_by_name(&self, name: &str) -> Option<AsyncTaskHandle> {
        let g = self.inner.lock().expect("async_task_store: lock poisoned");
        let id = g.by_name.get(name)?;
        g.by_id.get(id).cloned()
    }

    /// Look up a task handle directly by `AgentId`.
    pub fn find_by_id(&self, id: &AgentId) -> Option<AsyncTaskHandle> {
        let g = self.inner.lock().expect("async_task_store: lock poisoned");
        g.by_id.get(id).cloned()
    }

    /// Update the state of a registered task.
    ///
    /// If `id` is not registered this is a no-op (log-worthy in callers, but
    /// not an error — the store doesn't own the agent's lifecycle).
    pub fn update_state(&self, id: &AgentId, state: AsyncTaskState) {
        let mut g = self.inner.lock().expect("async_task_store: lock poisoned");
        if let Some(handle) = g.by_id.get_mut(id) {
            handle.state = state;
        }
    }

    /// Enqueue a message from the parent for delivery to the named async agent.
    ///
    /// Returns `Err` if `id` is not registered — the caller likely resolved
    /// the wrong AgentId.
    pub fn queue_pending_message(&self, id: &AgentId, msg: String) -> anyhow::Result<()> {
        let mut g = self.inner.lock().expect("async_task_store: lock poisoned");
        if !g.by_id.contains_key(id) {
            anyhow::bail!(
                "queue_pending_message: agent {} is not registered in AsyncAgentTaskStore",
                id.as_str()
            );
        }
        g.pending.entry(id.clone()).or_default().push(msg);
        Ok(())
    }

    /// Drain all pending messages for `id`, returning them in enqueue order.
    ///
    /// Returns an empty `Vec` if `id` is not registered or has no messages.
    pub fn drain_pending_messages(&self, id: &AgentId) -> Vec<String> {
        let mut g = self.inner.lock().expect("async_task_store: lock poisoned");
        g.pending.remove(id).unwrap_or_default()
    }

    /// Return handles for all tasks whose state is not terminal
    /// (`Completed`, `Failed`, `Killed`).
    pub fn list_active(&self) -> Vec<AsyncTaskHandle> {
        let g = self.inner.lock().expect("async_task_store: lock poisoned");
        g.by_id
            .values()
            .filter(|h| !h.state.is_terminal())
            .cloned()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handle(agent_id: &str, state: AsyncTaskState) -> AsyncTaskHandle {
        AsyncTaskHandle {
            agent_id: AgentId::new(agent_id),
            state,
            output_file: PathBuf::from(format!("/tmp/{agent_id}.output")),
            description: format!("test agent {agent_id}"),
        }
    }

    // P6.1 — test 1 (plan §9)
    #[test]
    fn registers_and_finds_by_name() {
        let store = AsyncAgentTaskStore::new();
        let handle = make_handle("agent-abc", AsyncTaskState::Running);
        store.register("explore", handle.clone());

        let found = store.find_by_name("explore").expect("should find by name");
        assert_eq!(found.agent_id, handle.agent_id);
        assert_eq!(found.state, AsyncTaskState::Running);
    }

    // P6.1 — test 2 (plan §9)
    #[test]
    fn pending_messages_queue_and_drain() {
        let store = AsyncAgentTaskStore::new();
        let id = AgentId::new("agent-xyz");
        store.register("worker", make_handle("agent-xyz", AsyncTaskState::Running));

        store
            .queue_pending_message(&id, "hello".to_owned())
            .unwrap();
        store
            .queue_pending_message(&id, "world".to_owned())
            .unwrap();

        let msgs = store.drain_pending_messages(&id);
        assert_eq!(msgs, vec!["hello", "world"]);

        // Second drain is empty.
        assert!(store.drain_pending_messages(&id).is_empty());
    }

    // P6.1 — test 3: state update keeps handle in store
    #[test]
    fn update_state_persists_handle() {
        let store = AsyncAgentTaskStore::new();
        let id = AgentId::new("agent-persist");
        store.register("persist-test", make_handle("agent-persist", AsyncTaskState::Running));

        store.update_state(&id, AsyncTaskState::Completed);

        let found = store.find_by_id(&id).expect("handle should still exist after Completed");
        assert_eq!(found.state, AsyncTaskState::Completed);
    }

    // P6.1 — test 4: sanity — unknown id returns None
    #[test]
    fn find_by_id_for_unregistered_returns_none() {
        let store = AsyncAgentTaskStore::new();
        let id = AgentId::new("ghost-agent");
        assert!(store.find_by_id(&id).is_none());
    }

    // P6.1 — test 5: queue for unknown id returns Err
    #[test]
    fn queue_pending_message_for_unknown_id_errors() {
        let store = AsyncAgentTaskStore::new();
        let id = AgentId::new("nobody");
        let result = store.queue_pending_message(&id, "msg".to_owned());
        assert!(result.is_err(), "expected Err for unregistered agent_id");
    }

    // P6.1 — test 6: list_active excludes terminal states
    #[test]
    fn list_active_excludes_terminal_states() {
        let store = AsyncAgentTaskStore::new();
        store.register("runner", make_handle("agent-run", AsyncTaskState::Running));
        store.register("done", make_handle("agent-done", AsyncTaskState::Running));

        // Flip second to Completed.
        store.update_state(&AgentId::new("agent-done"), AsyncTaskState::Completed);

        let active = store.list_active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent_id, AgentId::new("agent-run"));
    }
}
