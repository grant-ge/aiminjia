pub mod agent_invocation_store;
pub mod permission_store;
pub mod authorized_workspace_store;
pub mod audit_store;
pub mod conversation_store;
pub mod file_record_store;
pub mod memory_store;
pub mod pending_permission_request_store;
pub mod persona_store;
pub mod run_store;
pub mod session_store;
pub mod settings_store;
pub mod task_store;
pub mod tool_call_store;

use std::sync::Arc;

pub use agent_invocation_store::{
    AgentInvocationRecord, AgentInvocationStore, InMemoryAgentInvocationStore,
};
pub use authorized_workspace_store::{
    AuthorizedWorkspace, AuthorizedWorkspaceRef, AuthorizedWorkspaceStore,
    FileAuthorizedWorkspaceStore, InMemoryAuthorizedWorkspaceStore,
};
pub use audit_store::{AuditRecord, AuditStore, InMemoryAuditStore};
pub use conversation_store::{ConversationStore, InMemoryConversationStore};
pub use file_record_store::FileRecordStore;
pub use memory_store::{InMemoryMemoryStore, MemoryEntry, MemoryStore};
pub use pending_permission_request_store::{
    PendingPermissionControlPlane, PendingPermissionRequest,
    PendingPermissionRequestStore, PendingPermissionResolution,
};
pub use persona_store::{PersonaRecord, PersonaStore, PersonaSummary};
pub use permission_store::{PolicyDecision, PermissionStore};
pub use run_store::{InMemoryRunStore, RunRecord, RunStatus, RunStore};
pub use session_store::{InMemorySessionStore, SessionRecord, SessionStore};
pub use settings_store::{InMemorySettingsStore, SettingsStore};
pub use task_store::{InMemoryTaskStore, TaskStore};
pub use tool_call_store::{InMemoryToolCallStore, ToolCallRecord, ToolCallStatus, ToolCallStore};

#[derive(Default)]
pub struct RuntimeStoresBuilder {
    pub run_store: Option<Arc<dyn RunStore>>,
    pub task_store: Option<Arc<dyn TaskStore>>,
    pub tool_call_store: Option<Arc<dyn ToolCallStore>>,
    pub agent_invocation_store: Option<Arc<dyn AgentInvocationStore>>,
}

impl RuntimeStoresBuilder {
    pub fn build(self) -> RuntimeStores {
        RuntimeStores {
            run_store: self
                .run_store
                .unwrap_or_else(|| Arc::new(InMemoryRunStore::new())),
            task_store: self
                .task_store
                .unwrap_or_else(|| Arc::new(InMemoryTaskStore::new())),
            tool_call_store: self
                .tool_call_store
                .unwrap_or_else(|| Arc::new(InMemoryToolCallStore::new())),
            agent_invocation_store: self
                .agent_invocation_store
                .unwrap_or_else(|| Arc::new(InMemoryAgentInvocationStore::new())),
        }
    }
}

pub struct RuntimeStores {
    pub run_store: Arc<dyn RunStore>,
    pub task_store: Arc<dyn TaskStore>,
    pub tool_call_store: Arc<dyn ToolCallStore>,
    pub agent_invocation_store: Arc<dyn AgentInvocationStore>,
}

impl RuntimeStores {
    pub fn builder() -> RuntimeStoresBuilder {
        RuntimeStoresBuilder::default()
    }
}
