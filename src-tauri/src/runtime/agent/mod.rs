pub mod agent_runtime;
pub mod async_task_store;
pub mod background;
pub mod builtin;
pub mod cancellation_registry;
pub mod child_run;
pub mod definition;
pub mod employee_projection;
pub mod empty_response_recovery;
pub mod file_agent_invocation_store;
pub mod inbox;
pub mod inbox_registry;
pub mod invocation;
pub mod lead_idle;
pub mod markdown_loader;
pub mod message_bridge;
pub mod name_registry;
pub mod output_writer;
pub mod registry;
pub mod registry_loader;
// required_tools removed: Teammate-required tools are now injected at
// runtime via `tool_whitelist::TEAMMATE_TOOLS` (see src/runtime/agent/
// tool_whitelist.rs and spawn_subagent.rs Teammate dispatch path) rather
// than enforced as a hard pre-spawn gate on employee/agent definitions.
// Aligns with claude-code-best `IN_PROCESS_TEAMMATE_ALLOWED_TOOLS`.
pub mod resume;
pub mod subagent_result_envelope;
pub mod subagent_transcript_store;
pub mod task_notification;
pub mod task_notification_lead;
pub mod team;
pub mod team_context;
pub mod team_paths;
pub mod teammate_addendum;
pub mod tool_whitelist;
pub mod worker_runtime;
pub mod worktree;

pub use agent_runtime::AgentRuntime;
pub use cancellation_registry::CancellationRegistry;
pub use invocation::{
    AgentInvocation, AgentStatus, ChildRunHandle, ResumeChildRunRequest, SpawnChildRunRequest,
};
pub use subagent_transcript_store::{
    FileSubagentTranscriptStore, InMemorySubagentTranscriptStore, SubagentTranscriptEntryRecord,
    SubagentTranscriptStore,
};
pub use inbox::{AgentInbox, InboxItem, MessageSource, ShutdownRequest, TaskNotificationItem};
pub use inbox_registry::InboxRegistry;
pub use lead_idle::{LeadIdleSupervisor, LeadKey};
pub use name_registry::{AgentNameRegistry, NameRegistryError};
pub use team::{Member, MemberRole, MemberSnapshot, Team, TeamError, TeamPersistError, TeamRegistry, TeamSnapshot, MAX_TEAMMATES};
