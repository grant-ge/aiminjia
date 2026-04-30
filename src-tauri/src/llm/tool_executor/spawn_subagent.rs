//! Default implementation of [`SpawnSubagentLauncher`] that drives the actual
//! sub-agent execution via `llm::sub_agent::run_sub_agent`.
//!
//! This module is intentionally placed in the infrastructure layer
//! (`llm/tool_executor/`) rather than inside `runtime/tools/builtin/` so that
//! `spawn_subagent.rs` stays free of gateway / registry imports.
//!
//! Pattern mirrors `DefaultBrowseDataLauncher` / `BrowseDataLauncherDeps`.

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use crate::plugin::registry::RequestScopedRuntimeDeps;
use crate::runtime::agent::definition::AgentPrompt;
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::tools::builtin::spawn_subagent::{
    SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
};
use std::sync::Arc;

/// Deps snapshot captured at request-scope construction time.
#[derive(Clone)]
pub(crate) struct DefaultSpawnSubagentLauncher {
    deps: RequestScopedRuntimeDeps,
    registry: Arc<AgentRegistry>,
}

impl DefaultSpawnSubagentLauncher {
    pub fn from_runtime_deps(deps: RequestScopedRuntimeDeps, registry: Arc<AgentRegistry>) -> Self {
        Self { deps, registry }
    }
}

#[async_trait]
impl SpawnSubagentLauncher for DefaultSpawnSubagentLauncher {
    async fn launch_sync(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<String> {
        // Pull gateway + tool_registry + app_settings from deps (same as browse_data).
        let gateway = self
            .deps
            .gateway
            .as_ref()
            .ok_or_else(|| anyhow!("LLM gateway not available for spawn_subagent"))?;
        let tool_registry = self
            .deps
            .tool_registry
            .as_ref()
            .ok_or_else(|| anyhow!("Tool registry not available for spawn_subagent"))?;
        let app_settings = self
            .deps
            .app_settings
            .as_ref()
            .ok_or_else(|| anyhow!("App settings not available for spawn_subagent"))?;

        // Resolve agent definition from registry for system_prompt + tool lists.
        let definition = self
            .registry
            .get(&request.subagent_type)
            .ok_or_else(|| {
                anyhow!(
                    "unknown subagent_type '{}' in DefaultSpawnSubagentLauncher",
                    request.subagent_type
                )
            })?
            .clone();

        // Build system prompt string from definition.
        let system_prompt = match &definition.system_prompt {
            AgentPrompt::Inline(s) => s.clone(),
            AgentPrompt::File(path) => std::fs::read_to_string(path).unwrap_or_else(|e| {
                log::warn!(
                    "[spawn_subagent] Failed to read system prompt file {:?}: {}",
                    path,
                    e
                );
                String::new()
            }),
        };

        // Scope runtime deps to this sub-agent's parent run identity.
        let scoped_deps = self.deps.clone().with_run_scope(
            context.parent_run_id.clone(),
            context.parent_agent_id.clone(),
            Some(context.cancellation.clone()),
            self.deps
                .read_file_state
                .as_ref()
                .map(|cache| cache.clone_for_child()),
        );

        let config = crate::llm::sub_agent::SubAgentConfig {
            task: request.prompt.clone(),
            system_prompt,
            allowed_tools: definition.allowed_tools.clone(),
            disallowed_tools: definition.disallowed_tools.clone(),
            max_iterations: definition.max_iterations,
            dynamic_context: String::new(),
            conversation_id: scoped_deps.conversation_id.clone(),
            parent_run_id: context.parent_run_id.clone(),
            background: false, // sync path only
            app_handle: scoped_deps.app_handle.clone(),
            cancel_token: Some(context.cancellation.clone()),
            permission_mode: context.permission_mode,
            model_override: request.effective_model.clone(),
            agent_name: request.name.clone(),
        };

        let runtime_deps = crate::llm::sub_agent::SubAgentRuntimeDeps {
            storage: scoped_deps.storage.clone(),
            file_manager: scoped_deps.file_manager.clone(),
            workspace_path: scoped_deps.workspace_path.clone(),
            conversation_id: scoped_deps.conversation_id.clone(),
            session_id: scoped_deps.session_id.clone(),
            run_id: scoped_deps.run_id.clone(),
            agent_id: scoped_deps.agent_id.clone(),
            session_manager: scoped_deps.session_manager.clone(),
            connector_engine: scoped_deps.connector_engine.clone(),
            agent_runtime: scoped_deps.agent_runtime.clone(),
            event_bus: scoped_deps.event_bus.clone(),
            skill_registry: scoped_deps.skill_registry.clone(),
            authorized_workspace: scoped_deps.authorized_workspace.clone(),
            read_file_state: scoped_deps.read_file_state.clone(),
            app_handle: scoped_deps.app_handle.clone(),
            runtime_resolver: scoped_deps.runtime_resolver.clone(),
        };

        let result = crate::llm::sub_agent::run_sub_agent(
            gateway,
            tool_registry,
            &runtime_deps,
            config,
            app_settings,
        )
        .await
        .map_err(|e| {
            log::warn!(
                "[spawn_subagent] sub-agent '{}' failed: {}",
                request.subagent_type,
                e
            );
            anyhow!("Sub-agent '{}' failed: {}", request.subagent_type, e)
        })?;

        log::info!(
            "[spawn_subagent] '{}' complete: iterations={}, output_len={}",
            request.subagent_type,
            result.iterations_used,
            result.output.len()
        );

        Ok(result.envelope.output)
    }
}
