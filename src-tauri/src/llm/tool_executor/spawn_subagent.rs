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
use std::path::PathBuf;
use std::sync::Arc;

use crate::plugin::registry::RequestScopedRuntimeDeps;
use crate::runtime::agent::async_task_store::{AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState};
use crate::runtime::agent::definition::AgentPrompt;
use crate::runtime::agent::output_writer;
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::agent::task_notification::{build_task_notification_xml, TaskNotificationQueue};
use crate::runtime::ids::AgentId;
use crate::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnSubagentContext, SpawnSubagentLauncher, SpawnSubagentRequest,
};
use crate::storage::user_scoped_paths::UserScopedPathResolver;

/// Deps snapshot captured at request-scope construction time.
#[derive(Clone)]
pub(crate) struct DefaultSpawnSubagentLauncher {
    deps: RequestScopedRuntimeDeps,
    registry: Arc<AgentRegistry>,
    task_store: Arc<AsyncAgentTaskStore>,
    notif_queue: Arc<TaskNotificationQueue>,
    paths: Arc<dyn UserScopedPathResolver>,
}

impl DefaultSpawnSubagentLauncher {
    pub fn from_runtime_deps(
        deps: RequestScopedRuntimeDeps,
        registry: Arc<AgentRegistry>,
        task_store: Arc<AsyncAgentTaskStore>,
        notif_queue: Arc<TaskNotificationQueue>,
        paths: Arc<dyn UserScopedPathResolver>,
    ) -> Self {
        Self { deps, registry, task_store, notif_queue, paths }
    }

    /// Extract the fields needed to run the sub-agent from `deps`, returning an error
    /// if any required dep is missing.
    fn build_run_components(
        &self,
    ) -> Result<(
        Arc<crate::llm::gateway::LlmGateway>,
        Arc<crate::plugin::registry::ToolRegistry>,
        Arc<crate::models::settings::AppSettings>,
    )> {
        let gateway = self
            .deps
            .gateway
            .as_ref()
            .ok_or_else(|| anyhow!("LLM gateway not available for spawn_subagent"))?
            .clone();
        let tool_registry = self
            .deps
            .tool_registry
            .as_ref()
            .ok_or_else(|| anyhow!("Tool registry not available for spawn_subagent"))?
            .clone();
        let app_settings = self
            .deps
            .app_settings
            .as_ref()
            .ok_or_else(|| anyhow!("App settings not available for spawn_subagent"))?
            .clone();
        Ok((gateway, tool_registry, app_settings))
    }

    /// Build `SubAgentConfig` and `SubAgentRuntimeDeps` from the request + context.
    async fn build_sub_agent_args(
        &self,
        request: &SpawnSubagentRequest,
        context: &SpawnSubagentContext,
        background: bool,
    ) -> Result<(
        crate::llm::sub_agent::SubAgentConfig,
        crate::llm::sub_agent::SubAgentRuntimeDeps,
    )> {
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
        let mut system_prompt = match &definition.system_prompt {
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

        // Inject env block so the sub-agent knows the parent's authorized workspace
        // and current working directory. Reuses the same `build_env_info` that the
        // parent uses, so sub-agent and parent see identical environment context.
        let authorized_pair = scoped_deps
            .authorized_workspace
            .as_ref()
            .map(|aw| (aw.root_path.to_string_lossy().to_string(), aw.display_name.clone()));
        let env_info = crate::runtime::chat::context_builder::build_env_info(
            &scoped_deps.workspace_path,
            authorized_pair
                .as_ref()
                .map(|(root, name)| (root.as_str(), name.as_str())),
            None,
        )
        .await;
        if !env_info.is_empty() {
            if !system_prompt.is_empty() {
                system_prompt.push_str("\n\n");
            }
            system_prompt.push_str(&env_info);
        }

        let config = crate::llm::sub_agent::SubAgentConfig {
            task: request.prompt.clone(),
            system_prompt,
            allowed_tools: definition.allowed_tools.clone(),
            disallowed_tools: definition.disallowed_tools.clone(),
            max_iterations: definition.max_iterations,
            dynamic_context: String::new(),
            conversation_id: scoped_deps.conversation_id.clone(),
            parent_run_id: context.parent_run_id.clone(),
            background,
            app_handle: scoped_deps.app_handle.clone(),
            cancel_token: Some(context.cancellation.clone()),
            permission_mode: context.permission_mode,
            model_override: request.effective_model.clone(),
            agent_name: request.name.clone(),
            parent_tool_use_id: Some(context.parent_tool_use_id.as_str().to_owned()),
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

        Ok((config, runtime_deps))
    }
}

#[async_trait]
impl SpawnSubagentLauncher for DefaultSpawnSubagentLauncher {
    async fn launch_sync(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<String> {
        let (gateway, tool_registry, app_settings) = self.build_run_components()?;
        let (config, runtime_deps) = self.build_sub_agent_args(&request, &context, false).await?;

        let result = crate::llm::sub_agent::run_sub_agent(
            &gateway,
            &tool_registry,
            &runtime_deps,
            config,
            &app_settings,
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

    async fn launch_async(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        let (gateway, tool_registry, app_settings) = self.build_run_components()?;
        let (config, runtime_deps) = self.build_sub_agent_args(&request, &context, true).await?;

        // Generate a fresh AgentId for this async sub-agent.
        let agent_id = AgentId::new(uuid::Uuid::new_v4().to_string());

        let transcript_path = match self.paths.require_paths() {
            Ok(p) => output_writer::transcript_path(
                &p.subagent_transcripts_dir(),
                agent_id.as_str(),
            ),
            Err(e) => {
                log::warn!("[spawn_subagent async] no user scope: {}; transcript disabled", e);
                std::path::PathBuf::new()
            }
        };
        let transcript_path_for_task = transcript_path.clone();

        // If the request has a name, register it in AsyncAgentTaskStore.
        if let Some(ref name) = request.name {
            let handle = AsyncTaskHandle {
                agent_id: agent_id.clone(),
                state: AsyncTaskState::Running,
                output_file: transcript_path.clone(),
                description: request.description.clone(),
            };
            self.task_store.register(name, handle);
        }

        // Capture all Arcs + owned data for the spawned task (no &-references).
        let task_store = self.task_store.clone();
        let notif_queue = self.notif_queue.clone();
        let id_for_task = agent_id.clone();
        let parent_tool_use_id = context.parent_tool_use_id.as_str().to_owned();
        let parent_session_id = context.session_id.clone();
        let parent_run_id = context.parent_run_id.clone();
        let subagent_type = request.subagent_type.clone();

        tokio::spawn(async move {
            // catch_unwind protects against panics inside run_sub_agent so
            // the lifecycle always terminates with state update + notification.
            // Without this, a panic would drop the JoinHandle silently and
            // the parent would observe Running forever.
            use futures::FutureExt;
            let body = std::panic::AssertUnwindSafe(async {
                crate::llm::sub_agent::run_sub_agent(
                    &gateway,
                    &tool_registry,
                    &runtime_deps,
                    config,
                    &app_settings,
                )
                .await
            });
            let outcome = body.catch_unwind().await;

            match outcome {
                Ok(Ok(sub_result)) => {
                    let output_ref = sub_result.envelope.output.as_str();
                    if let Err(e) = output_writer::append_line(
                        &transcript_path_for_task,
                        &output_writer::TranscriptLine::assistant(output_ref),
                    ) {
                        log::warn!(
                            "[spawn_subagent async {}] transcript append failed: {}; downstream task_output may be empty",
                            id_for_task.as_str(),
                            e
                        );
                    }
                    let p_str = transcript_path_for_task.to_string_lossy();
                    let xml = build_task_notification_xml(
                        id_for_task.as_str(),
                        Some(&parent_tool_use_id),
                        &p_str,
                        "completed",
                        &format!("Agent '{}' completed", subagent_type),
                        Some(output_ref),
                        None,
                    );
                    notif_queue.enqueue(
                        id_for_task.as_str(),
                        xml,
                        parent_session_id.clone(),
                        parent_run_id.clone(),
                    );
                    // Terminal state LAST — guarantees that any observer seeing
                    // AsyncTaskState::Completed can already read the transcript
                    // and find the notification in the queue. Inverting this
                    // order would let parents see Completed but get an empty
                    // task_output / missing notification.
                    task_store.update_state(&id_for_task, AsyncTaskState::Completed);
                }
                Ok(Err(e)) => {
                    let err_str = e.to_string();
                    log::warn!(
                        "[spawn_subagent async] agent '{}' ({}) failed: {}",
                        subagent_type,
                        id_for_task.as_str(),
                        err_str
                    );
                    if let Err(append_err) = output_writer::append_line(
                        &transcript_path_for_task,
                        &output_writer::TranscriptLine::failed(&err_str),
                    ) {
                        log::warn!(
                            "[spawn_subagent async {}] transcript append failed: {}; downstream task_output may be empty",
                            id_for_task.as_str(),
                            append_err
                        );
                    }
                    let p_str = transcript_path_for_task.to_string_lossy();
                    let xml = build_task_notification_xml(
                        id_for_task.as_str(),
                        Some(&parent_tool_use_id),
                        &p_str,
                        "failed",
                        &format!("Agent '{}' failed", subagent_type),
                        Some(&err_str),
                        None,
                    );
                    notif_queue.enqueue(
                        id_for_task.as_str(),
                        xml,
                        parent_session_id.clone(),
                        parent_run_id.clone(),
                    );
                    task_store.update_state(&id_for_task, AsyncTaskState::Failed);
                }
                Err(panic_payload) => {
                    let panic_msg = panic_payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| {
                            panic_payload
                                .downcast_ref::<&'static str>()
                                .map(|s| (*s).to_string())
                        })
                        .unwrap_or_else(|| "panic with non-string payload".to_string());
                    log::error!(
                        "[spawn_subagent async] agent '{}' ({}) PANICKED: {}",
                        subagent_type,
                        id_for_task.as_str(),
                        panic_msg
                    );
                    let panic_summary = format!("panic: {}", panic_msg);
                    if let Err(append_err) = output_writer::append_line(
                        &transcript_path_for_task,
                        &output_writer::TranscriptLine::failed(&panic_summary),
                    ) {
                        log::warn!(
                            "[spawn_subagent async {}] transcript append failed: {}; downstream task_output may be empty",
                            id_for_task.as_str(),
                            append_err
                        );
                    }
                    let p_str = transcript_path_for_task.to_string_lossy();
                    let xml = build_task_notification_xml(
                        id_for_task.as_str(),
                        Some(&parent_tool_use_id),
                        &p_str,
                        "failed",
                        &format!("Agent '{}' panicked", subagent_type),
                        Some(&panic_summary),
                        None,
                    );
                    notif_queue.enqueue(
                        id_for_task.as_str(),
                        xml,
                        parent_session_id.clone(),
                        parent_run_id.clone(),
                    );
                    task_store.update_state(&id_for_task, AsyncTaskState::Failed);
                }
            }
        });

        Ok(SpawnAsyncOutcome {
            agent_id,
            name: request.name.clone(),
        })
    }
}

