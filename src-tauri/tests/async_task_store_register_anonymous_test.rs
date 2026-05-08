//! Unit tests for AsyncAgentTaskStore::register_anonymous.
//!
//! Locks in the bug-fix that register_anonymous correctly inserts into by_id
//! (historically the method existed but the insertion was missing).
//!
//! 3 cases:
//! 1. registered_handle_is_findable_by_id: basic round-trip
//! 2. register_anonymous_does_not_add_name_index: anonymous registration
//!    must NOT pollute the by_name index
//! 3. register_anonymous_overwrites_existing_handle: re-registering same
//!    AgentId replaces the handle (idempotent overwrite semantic)

use std::path::PathBuf;

use app_lib::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState,
};
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::AgentId;

fn make_handle(agent_id: &str, state: AsyncTaskState) -> AsyncTaskHandle {
    AsyncTaskHandle {
        agent_id: AgentId::new(agent_id),
        state,
        output_file: PathBuf::from(format!("/tmp/{agent_id}.out")),
        description: format!("anon test agent {agent_id}"),
        cancel_token: CancellationToken::new(),
    }
}

// ─── Test 1 ──────────────────────────────────────────────────────────────────

#[test]
fn registered_handle_is_findable_by_id() {
    let store = AsyncAgentTaskStore::new();
    let id = AgentId::new("anon-bug-fix-001");
    store.register_anonymous(make_handle("anon-bug-fix-001", AsyncTaskState::Running));

    let found = store
        .find_by_id(&id)
        .expect("register_anonymous must insert into by_id so find_by_id returns Some");
    assert_eq!(found.agent_id, id);
    assert_eq!(found.state, AsyncTaskState::Running);
    assert_eq!(found.description, "anon test agent anon-bug-fix-001");
}

// ─── Test 2 ──────────────────────────────────────────────────────────────────

#[test]
fn register_anonymous_does_not_add_name_index() {
    let store = AsyncAgentTaskStore::new();
    store.register_anonymous(make_handle("anon-bug-fix-002", AsyncTaskState::Running));

    // find_by_name should return None because anonymous registration
    // intentionally skips the by_name index.
    let by_name = store.find_by_name("anon-bug-fix-002");
    assert!(
        by_name.is_none(),
        "register_anonymous must not populate the by_name index; name was not provided"
    );

    // But by_id lookup must work
    let by_id = store.find_by_id(&AgentId::new("anon-bug-fix-002"));
    assert!(by_id.is_some(), "by_id lookup must succeed after register_anonymous");
}

// ─── Test 3 ──────────────────────────────────────────────────────────────────

#[test]
fn register_anonymous_overwrites_existing_handle() {
    let store = AsyncAgentTaskStore::new();
    let id = AgentId::new("anon-overwrite-001");

    // First registration
    store.register_anonymous(AsyncTaskHandle {
        agent_id: id.clone(),
        state: AsyncTaskState::Running,
        output_file: PathBuf::from("/tmp/first.out"),
        description: "first registration".to_string(),
        cancel_token: CancellationToken::new(),
    });

    // Second registration with same id but different state
    store.register_anonymous(AsyncTaskHandle {
        agent_id: id.clone(),
        state: AsyncTaskState::Backgrounded,
        output_file: PathBuf::from("/tmp/second.out"),
        description: "second registration".to_string(),
        cancel_token: CancellationToken::new(),
    });

    // The second registration should overwrite the first
    let found = store
        .find_by_id(&id)
        .expect("handle must be findable after second register_anonymous");
    assert_eq!(
        found.state,
        AsyncTaskState::Backgrounded,
        "second registration must overwrite the first (state should be Backgrounded)"
    );
    assert_eq!(found.description, "second registration");
}
