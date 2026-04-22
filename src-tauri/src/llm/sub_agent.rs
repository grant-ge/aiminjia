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
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
    pub read_file_state: Option<Arc<crate::runtime::tools::capability::FileStateCache>>,
    pub app_handle: Option<tauri::AppHandle>,
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
            skill_registry: None,
            skill_sessions: None,
            authorized_workspace: self.authorized_workspace.clone(),
            read_file_state,
            cancellation,
            permission_mode: PermissionMode::Default,
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
    if config.allowed_tools.contains(&"browse_data".to_string()) {
        return Err(anyhow::anyhow!(
            "Sub-agent must not include 'browse_data' in allowed_tools (recursion guard)"
        )
        .into());
    }

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
    fn take_ask_required_decision_preserves_structured_permission_request() {
        let decision = PermissionDecision::Ask {
            message: "need approval".to_string(),
            suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
            remember_options: default_permission_ask().0,
            default_destination: default_permission_ask().1,
            reason: PermissionReason::Other("subagent-inner".to_string()),
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
