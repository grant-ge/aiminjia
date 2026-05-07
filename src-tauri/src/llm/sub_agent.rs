//! Sub-agent executor — 仅保留 worker runtime 的请求入口与共享数据结构。

use std::sync::Arc;

use crate::llm::gateway::LlmGateway;
use crate::models::settings::AppSettings;
use crate::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use crate::plugin::tool_trait::ToolError as LegacyToolError;
use crate::runtime::agent::subagent_result_envelope::SubAgentResultEnvelope;
use crate::runtime::agent::worker_runtime::SubagentWorkerRuntime;
use crate::runtime::agent::AgentRuntime;
use crate::runtime::ids::RunId;
use crate::runtime::tools::permission::PermissionMode;

#[cfg(test)]
fn take_ask_required_decision(
    err: &LegacyToolError,
) -> Option<crate::runtime::tools::permission::PermissionDecision> {
    match err {
        LegacyToolError::AskRequired(decision) => Some(decision.clone()),
        _ => None,
    }
}

#[derive(Clone)]
pub struct SubAgentRuntimeDeps {
    pub storage: Arc<crate::storage::file_store::AppStorage>,
    pub file_manager: Arc<crate::storage::file_manager::FileManager>,
    pub workspace_path: std::path::PathBuf,
    pub conversation_id: String,
    pub session_id: crate::runtime::ids::SessionId,
    pub run_id: Option<RunId>,
    pub agent_id: Option<crate::runtime::ids::AgentId>,
    pub session_manager: Arc<crate::python::session::PythonSessionManager>,
    pub connector_engine: Option<Arc<crate::connector::ConnectorEngine>>,
    pub agent_runtime: Option<Arc<AgentRuntime>>,
    pub event_bus: Option<crate::runtime::event_bus::RuntimeEventBus>,
    pub skill_registry:
        Option<Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>>,
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
    pub read_file_state: Option<Arc<crate::runtime::tools::capability::FileStateCache>>,
    pub app_handle: Option<tauri::AppHandle>,
    pub runtime_resolver: Option<crate::runtime::dependencies::ManagedRuntimeResolver>,
}

impl SubAgentRuntimeDeps {
    pub fn request_scoped_tool_deps(
        &self,
        run_id: RunId,
        agent_id: Option<crate::runtime::ids::AgentId>,
        cancellation: Option<crate::runtime::cancellation::CancellationToken>,
        read_file_state: Option<Arc<crate::runtime::tools::capability::FileStateCache>>,
    ) -> RequestScopedRuntimeDeps {
        RequestScopedRuntimeDeps {
            storage: self.storage.clone(),
            file_manager: self.file_manager.clone(),
            workspace_path: self.workspace_path.clone(),
            conversation_id: self.conversation_id.clone(),
            session_id: self.session_id.clone(),
            run_id: Some(run_id),
            agent_id,
            tavily_api_key: None,
            bocha_api_key: None,
            app_handle: self.app_handle.clone(),
            session_manager: self.session_manager.clone(),
            auth_manager: None,
            connector_engine: self.connector_engine.clone(),
            use_cloud: false,
            model: String::new(),
            gateway: None,
            tool_registry: None,
            app_settings: None,
            agent_runtime: self.agent_runtime.clone(),
            event_bus: self.event_bus.clone(),
            skill_registry: self.skill_registry.clone(),
            authorized_workspace: self.authorized_workspace.clone(),
            read_file_state,
            cancellation,
            permission_mode: PermissionMode::Default,
            runtime_resolver: self.runtime_resolver.clone(),
        }
    }
}

/// Configuration for a sub-agent run.
pub struct SubAgentConfig {
    pub task: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    pub max_iterations: usize,
    pub dynamic_context: String,
    pub conversation_id: String,
    pub parent_run_id: Option<RunId>,
    pub background: bool,
    pub app_handle: Option<tauri::AppHandle>,
    pub cancel_token: Option<crate::runtime::cancellation::CancellationToken>,
    pub permission_mode: PermissionMode,
    /// Caller-supplied model override (e.g. "haiku"). When `Some(non_empty)`,
    /// the sub-agent's gateway call uses a per-call AppSettings clone with
    /// `primary_model` set to this value. `None` or empty string = inherit
    /// parent's model. Consumed by P2.2 effective_settings_for_subagent.
    pub model_override: Option<String>,
    /// Optional name for async sub-agent registration in AsyncAgentTaskStore
    /// (P6.1). When `Some`, parent can SendMessage to this name. `None` =
    /// anonymous (no SendMessage routing).
    pub agent_name: Option<String>,
    /// The parent agent's `tool_call_id` for the spawn_subagent call that
    /// created this sub-agent. Stamped onto every `tool:executing` /
    /// `tool:completed` event the sub-agent emits, so a future "sub-agent
    /// detail panel" can fold sub-agent tool history under the originating
    /// spawn_subagent step (mirrors claude-code-best's `parent_tool_use_id`
    /// single-track design). `None` when caller didn't supply one (legacy /
    /// internal callers).
    pub parent_tool_use_id: Option<String>,
    /// Tool blacklist from the AgentDefinition. Combined with system-level
    /// `ALL_AGENT_DISALLOWED` and the definition's `allowed_tools` by
    /// `resolve_agent_tools` (P4.2) to produce the final tool whitelist.
    pub disallowed_tools: Vec<String>,
}

/// Result from a sub-agent run.
pub struct SubAgentResult {
    pub output: String,
    pub files: Vec<String>,
    pub iterations_used: usize,
    pub envelope: SubAgentResultEnvelope,
}

pub async fn run_sub_agent(
    gateway: &LlmGateway,
    tool_registry: &ToolRegistry,
    runtime_deps: &SubAgentRuntimeDeps,
    config: SubAgentConfig,
    settings: &AppSettings,
) -> std::result::Result<SubAgentResult, LegacyToolError> {
    let runtime = SubagentWorkerRuntime::new(gateway, tool_registry, runtime_deps, settings);
    runtime.run(config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::tool_trait::ToolError as LegacyToolError;
    use crate::runtime::tools::permission::{
        default_permission_ask, PermissionDecision, PermissionReason,
    };

    #[test]
    fn sub_agent_config_carries_model_override_and_agent_name() {
        use crate::runtime::tools::permission::PermissionMode;
        let cfg = SubAgentConfig {
            task: "do".into(),
            system_prompt: "you are".into(),
            allowed_tools: vec![],
            disallowed_tools: vec!["dangerous_tool".into()],
            max_iterations: 5,
            dynamic_context: String::new(),
            conversation_id: "c1".into(),
            parent_run_id: None,
            background: false,
            app_handle: None,
            cancel_token: None,
            permission_mode: PermissionMode::Default,
            model_override: Some("haiku".into()),
            agent_name: Some("worker1".into()),
            parent_tool_use_id: Some("toolu_parent_abc".into()),
        };
        assert_eq!(cfg.model_override.as_deref(), Some("haiku"));
        assert_eq!(cfg.agent_name.as_deref(), Some("worker1"));
        assert_eq!(cfg.parent_tool_use_id.as_deref(), Some("toolu_parent_abc"));
        assert_eq!(cfg.disallowed_tools, vec!["dangerous_tool".to_string()]);
    }

    #[test]
    fn sub_agent_config_defaults_omit_overrides() {
        use crate::runtime::tools::permission::PermissionMode;
        let cfg = SubAgentConfig {
            task: "do".into(),
            system_prompt: "you are".into(),
            allowed_tools: vec![],
            disallowed_tools: vec![],
            max_iterations: 5,
            dynamic_context: String::new(),
            conversation_id: "c1".into(),
            parent_run_id: None,
            background: false,
            app_handle: None,
            cancel_token: None,
            permission_mode: PermissionMode::Default,
            model_override: None,
            agent_name: None,
            parent_tool_use_id: None,
        };
        assert!(cfg.model_override.is_none());
        assert!(cfg.agent_name.is_none());
        assert!(cfg.parent_tool_use_id.is_none());
        assert!(cfg.disallowed_tools.is_empty());
    }

    #[test]
    fn take_ask_required_decision_preserves_structured_permission_request() {
        let decision = PermissionDecision::Ask {
            message: "need approval".to_string(),
            suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
            remember_options: default_permission_ask().0,
            default_destination: default_permission_ask().1,
            reason: PermissionReason::Other("subagent-inner".to_string()),
            path_auth_scope: None,
        };

        let extracted = take_ask_required_decision(&LegacyToolError::AskRequired(decision.clone()))
            .expect("AskRequired must stay structured");

        match extracted {
            PermissionDecision::Ask {
                message,
                suggestions,
                ..
            } => {
                assert_eq!(message, "need approval");
                assert_eq!(
                    suggestions,
                    vec!["Allow once".to_string(), "Deny".to_string()]
                );
            }
            other => panic!("expected ask decision, got: {:?}", other),
        }
    }
}
