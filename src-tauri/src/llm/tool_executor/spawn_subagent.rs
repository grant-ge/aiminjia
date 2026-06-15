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
use crate::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState,
};
use crate::runtime::agent::definition::AgentPrompt;
use crate::runtime::agent::output_writer;
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::agent::task_notification::{
    build_task_notification_xml, TaskNotificationQueue,
};
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::ids::AgentId;
use crate::runtime::tools::builtin::spawn_subagent::{
    SpawnAsyncOutcome, SpawnForegroundAutoOutcome, SpawnSubagentContext, SpawnSubagentLauncher,
    SpawnSubagentRequest,
};
use crate::storage::user_scoped_paths::UserScopedPathResolver;

const DEFAULT_AGENT_AUTO_BACKGROUND_AFTER_MS: u64 = 15_000;

enum ForegroundPromotionState {
    Waiting(tokio::sync::oneshot::Sender<SubAgentTaskOutcome>),
    Promoted,
    Finished,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::sub_agent::SubAgentResult;
    use crate::runtime::agent::async_task_store::{AsyncTaskHandle, AsyncTaskState};
    use crate::runtime::agent::subagent_result_envelope::SubAgentResultEnvelope;
    use crate::runtime::agent::task_notification::TaskNotificationQueue;
    use crate::runtime::cancellation::CancellationToken;
    use crate::runtime::event_bus::RuntimeEventBus;
    use crate::runtime::events::{AgentIdleScope, RuntimeEventKind};
    use crate::runtime::ids::{AgentId, RunId, SessionId};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn completed_result(output: &str) -> SubAgentResult {
        SubAgentResult {
            output: output.to_string(),
            files: Vec::new(),
            iterations_used: 1,
            envelope: SubAgentResultEnvelope {
                schema_version: 1,
                output: output.to_string(),
                iterations_used: 1,
                generated_files: Vec::new(),
                terminal_tool_results: Vec::new(),
                transcript_snapshot: Vec::new(),
                transcript_ref: None,
                terminal_stop_reason: None,
                max_tokens_recovery_attempts: 0,
            },
        }
    }

    #[tokio::test]
    async fn background_completion_emits_child_agent_idle_after_enqueuing_notification() {
        let task_store = Arc::new(AsyncAgentTaskStore::new());
        let notif_queue = Arc::new(TaskNotificationQueue::new());
        let event_bus = RuntimeEventBus::new();
        let tmp = TempDir::new().expect("tempdir");
        let agent_id = AgentId::new("agent-finished");
        let transcript_path = tmp.path().join("agent-finished.jsonl");
        let session_id = SessionId::new("session-finished");
        let parent_run_id = RunId::new("run-parent");

        task_store.register_anonymous(AsyncTaskHandle {
            agent_id: agent_id.clone(),
            state: AsyncTaskState::Running,
            output_file: transcript_path.clone(),
            description: "test background agent".to_string(),
            cancel_token: CancellationToken::new(),
        });

        let ctx = SpawnBackgroundTaskCtx {
            task_store: task_store.clone(),
            notif_queue: notif_queue.clone(),
            event_bus: Some(event_bus.clone()),
            agent_id: agent_id.clone(),
            transcript_path,
            parent_tool_use_id: "tool-call-1".to_string(),
            parent_session_id: session_id.clone(),
            parent_run_id: Some(parent_run_id.clone()),
            subagent_type: "general-purpose".to_string(),
        };

        finish_background_subagent(
            ctx,
            SubAgentTaskOutcome::Completed(completed_result("done")),
        )
        .await;

        assert_eq!(
            task_store.find_by_id(&agent_id).expect("task handle").state,
            AsyncTaskState::Completed
        );
        assert_eq!(notif_queue.drain_for_session(&session_id).len(), 1);

        let events = event_bus.recorded();
        assert!(
            events.iter().any(|event| {
                event.session_id == session_id
                    && event.run_id == parent_run_id
                    && matches!(
                        &event.kind,
                        RuntimeEventKind::AgentIdle {
                            agent_id: emitted_agent_id,
                            scope: AgentIdleScope::Child,
                        } if emitted_agent_id == &agent_id
                    )
            }),
            "background completion must emit child AgentIdle so the frontend can wake the parent loop; events: {events:?}"
        );
    }
}

struct SpawnBackgroundTaskCtx {
    task_store: Arc<AsyncAgentTaskStore>,
    notif_queue: Arc<TaskNotificationQueue>,
    event_bus: Option<crate::runtime::event_bus::RuntimeEventBus>,
    agent_id: AgentId,
    transcript_path: PathBuf,
    parent_tool_use_id: String,
    parent_session_id: crate::runtime::ids::SessionId,
    parent_run_id: Option<crate::runtime::ids::RunId>,
    subagent_type: String,
}

enum SubAgentTaskOutcome {
    Completed(crate::llm::sub_agent::SubAgentResult),
    Failed(String),
    Panicked(String),
}

fn panic_payload_to_string(panic_payload: Box<dyn std::any::Any + Send>) -> String {
    panic_payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            panic_payload
                .downcast_ref::<&'static str>()
                .map(|s| (*s).to_string())
        })
        .unwrap_or_else(|| "panic with non-string payload".to_string())
}

async fn emit_child_agent_idle(ctx: &SpawnBackgroundTaskCtx) {
    let Some(event_bus) = ctx.event_bus.as_ref() else {
        return;
    };
    let Some(parent_run_id) = ctx.parent_run_id.clone() else {
        log::debug!(
            "[spawn_subagent async {}] no parent run id; skipping child AgentIdle event",
            ctx.agent_id.as_str()
        );
        return;
    };

    let event = crate::runtime::events::RuntimeEvent::new(
        ctx.parent_session_id.clone(),
        parent_run_id,
        crate::runtime::events::RuntimeEventKind::AgentIdle {
            agent_id: ctx.agent_id.clone(),
            scope: crate::runtime::events::AgentIdleScope::Child,
        },
    );
    if let Err(e) = event_bus.emit(event).await {
        log::warn!(
            "[spawn_subagent async {}] emit child AgentIdle failed: {e}",
            ctx.agent_id.as_str()
        );
    }
}

async fn finish_background_subagent(ctx: SpawnBackgroundTaskCtx, outcome: SubAgentTaskOutcome) {
    if matches!(
        ctx.task_store
            .find_by_id(&ctx.agent_id)
            .map(|handle| handle.state),
        Some(AsyncTaskState::Killed)
    ) {
        log::info!(
            "[spawn_subagent async {}] task was killed; suppressing late worker result",
            ctx.agent_id.as_str()
        );
        return;
    }

    match outcome {
        SubAgentTaskOutcome::Completed(sub_result) => {
            let output_ref = sub_result.envelope.output.as_str();
            if let Err(e) = output_writer::append_line(
                &ctx.transcript_path,
                &output_writer::TranscriptLine::assistant(output_ref),
            ) {
                log::warn!(
                    "[spawn_subagent async {}] transcript append failed: {}; downstream task_output may be empty",
                    ctx.agent_id.as_str(),
                    e
                );
            }
            let p_str = ctx.transcript_path.to_string_lossy();
            let xml = build_task_notification_xml(
                ctx.agent_id.as_str(),
                Some(&ctx.parent_tool_use_id),
                &p_str,
                "completed",
                &format!("Agent '{}' completed", ctx.subagent_type),
                Some(output_ref),
                None,
            );
            ctx.notif_queue.enqueue(
                ctx.agent_id.as_str(),
                xml,
                ctx.parent_session_id.clone(),
                ctx.parent_run_id.clone(),
            );
            ctx.task_store
                .update_state(&ctx.agent_id, AsyncTaskState::Completed);
            emit_child_agent_idle(&ctx).await;
        }
        SubAgentTaskOutcome::Failed(err_str) => {
            log::warn!(
                "[spawn_subagent async] agent '{}' ({}) failed: {}",
                ctx.subagent_type,
                ctx.agent_id.as_str(),
                err_str
            );
            if let Err(append_err) = output_writer::append_line(
                &ctx.transcript_path,
                &output_writer::TranscriptLine::failed(&err_str),
            ) {
                log::warn!(
                    "[spawn_subagent async {}] transcript append failed: {}; downstream task_output may be empty",
                    ctx.agent_id.as_str(),
                    append_err
                );
            }
            let p_str = ctx.transcript_path.to_string_lossy();
            let xml = build_task_notification_xml(
                ctx.agent_id.as_str(),
                Some(&ctx.parent_tool_use_id),
                &p_str,
                "failed",
                &format!("Agent '{}' failed", ctx.subagent_type),
                Some(&err_str),
                None,
            );
            ctx.notif_queue.enqueue(
                ctx.agent_id.as_str(),
                xml,
                ctx.parent_session_id.clone(),
                ctx.parent_run_id.clone(),
            );
            ctx.task_store
                .update_state(&ctx.agent_id, AsyncTaskState::Failed);
            emit_child_agent_idle(&ctx).await;
        }
        SubAgentTaskOutcome::Panicked(panic_msg) => {
            log::error!(
                "[spawn_subagent async] agent '{}' ({}) PANICKED: {}",
                ctx.subagent_type,
                ctx.agent_id.as_str(),
                panic_msg
            );
            let panic_summary = format!("panic: {}", panic_msg);
            if let Err(append_err) = output_writer::append_line(
                &ctx.transcript_path,
                &output_writer::TranscriptLine::failed(&panic_summary),
            ) {
                log::warn!(
                    "[spawn_subagent async {}] transcript append failed: {}; downstream task_output may be empty",
                    ctx.agent_id.as_str(),
                    append_err
                );
            }
            let p_str = ctx.transcript_path.to_string_lossy();
            let xml = build_task_notification_xml(
                ctx.agent_id.as_str(),
                Some(&ctx.parent_tool_use_id),
                &p_str,
                "failed",
                &format!("Agent '{}' panicked", ctx.subagent_type),
                Some(&panic_summary),
                None,
            );
            ctx.notif_queue.enqueue(
                ctx.agent_id.as_str(),
                xml,
                ctx.parent_session_id.clone(),
                ctx.parent_run_id.clone(),
            );
            ctx.task_store
                .update_state(&ctx.agent_id, AsyncTaskState::Failed);
            emit_child_agent_idle(&ctx).await;
        }
    }
}

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
        Self {
            deps,
            registry,
            task_store,
            notif_queue,
            paths,
        }
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
        cancel_token: CancellationToken,
    ) -> Result<(
        crate::llm::sub_agent::SubAgentConfig,
        crate::llm::sub_agent::SubAgentRuntimeDeps,
    )> {
        // Resolve agent definition from registry for system_prompt + tool lists.
        let definition = self.registry.get(&request.subagent_type).ok_or_else(|| {
            anyhow!(
                "DefaultSpawnSubagentLauncher: subagent_type '{}' not in AgentRegistry. \
                 Available: {}",
                request.subagent_type,
                self.registry
                    .list()
                    .iter()
                    .map(|d| d.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

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
            Some(cancel_token.clone()),
            self.deps
                .read_file_state
                .as_ref()
                .map(|cache| cache.clone_for_child()),
        );

        // Inject env block so the sub-agent knows the parent's authorized workspace
        // and current working directory. Reuses the same `build_env_info` that the
        // parent uses, so sub-agent and parent see identical environment context.
        let authorized_pair = scoped_deps.authorized_workspace.as_ref().map(|aw| {
            (
                aw.root_path.to_string_lossy().to_string(),
                aw.display_name.clone(),
            )
        });
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
            cancel_token: Some(cancel_token),
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
            agent_runtime: scoped_deps.agent_runtime.clone(),
            event_bus: scoped_deps.event_bus.clone(),
            skill_registry: scoped_deps.skill_registry.clone(),
            authorized_workspace: scoped_deps.authorized_workspace.clone(),
            read_file_state: scoped_deps.read_file_state.clone(),
            app_handle: scoped_deps.app_handle.clone(),
            auth_manager: scoped_deps.auth_manager.clone(),
            runtime_resolver: scoped_deps.runtime_resolver.clone(),
            // Phase 5: snapshot of the parent turn's merged permission_ctx,
            // extracted from SpawnSubagentContext which received it from the
            // parent ToolExecutionContext.capability.storage.permission_ctx.
            // The child's QueryEngine uses this as its base_permission_ctx so
            // path tools run by the sub-agent see the same authorized paths as
            // the parent (UserSettings working dirs + session attachment dirs
            // already merged in at the time of spawning).
            permission_ctx: context.permission_ctx.clone(),
            // Propagate parent turn's active persona id so request-scoped tools
            // (e.g. agenda) inside the sub-agent still bind organizer to the
            // same persona as the parent.
            current_persona_id: scoped_deps.current_persona_id.clone(),
        };

        Ok((config, runtime_deps))
    }

    fn transcript_path_for_agent(&self, agent_id: &AgentId) -> PathBuf {
        match self.paths.require_paths() {
            Ok(p) => {
                output_writer::transcript_path(&p.subagent_transcripts_dir(), agent_id.as_str())
            }
            Err(e) => {
                log::warn!(
                    "[spawn_subagent async] no user scope: {}; transcript disabled",
                    e
                );
                PathBuf::new()
            }
        }
    }

    fn register_background_task(
        &self,
        request: &SpawnSubagentRequest,
        agent_id: &AgentId,
        transcript_path: &PathBuf,
        cancel_token: CancellationToken,
    ) {
        let handle = AsyncTaskHandle {
            agent_id: agent_id.clone(),
            state: AsyncTaskState::Running,
            output_file: transcript_path.clone(),
            description: request.description.clone(),
            cancel_token,
        };
        if let Some(ref name) = request.name {
            self.task_store.register(name, handle);
        } else {
            self.task_store.register_anonymous(handle);
        }
    }

    async fn run_sub_agent_task(
        gateway: Arc<crate::llm::gateway::LlmGateway>,
        tool_registry: Arc<crate::plugin::registry::ToolRegistry>,
        runtime_deps: crate::llm::sub_agent::SubAgentRuntimeDeps,
        config: crate::llm::sub_agent::SubAgentConfig,
        app_settings: Arc<crate::models::settings::AppSettings>,
        log_ctx: crate::log_context::LogContext,
    ) -> SubAgentTaskOutcome {
        use futures::FutureExt;

        let body = std::panic::AssertUnwindSafe(crate::log_context::scoped(log_ctx, async {
            crate::llm::sub_agent::run_sub_agent(
                &gateway,
                &tool_registry,
                &runtime_deps,
                config,
                &app_settings,
            )
            .await
        }));
        match body.catch_unwind().await {
            Ok(Ok(sub_result)) => SubAgentTaskOutcome::Completed(sub_result),
            Ok(Err(e)) => SubAgentTaskOutcome::Failed(e.to_string()),
            Err(panic_payload) => {
                SubAgentTaskOutcome::Panicked(panic_payload_to_string(panic_payload))
            }
        }
    }

    fn sync_result_from_outcome(
        subagent_type: &str,
        outcome: SubAgentTaskOutcome,
    ) -> Result<SpawnForegroundAutoOutcome> {
        match outcome {
            SubAgentTaskOutcome::Completed(result) => {
                log::info!(
                    "[spawn_subagent] '{}' complete: iterations={}, output_len={}",
                    subagent_type,
                    result.iterations_used,
                    result.output.len()
                );
                Ok(SpawnForegroundAutoOutcome::Completed(
                    result.envelope.output,
                ))
            }
            SubAgentTaskOutcome::Failed(err_str) => {
                log::warn!(
                    "[spawn_subagent] sub-agent '{}' failed: {}",
                    subagent_type,
                    err_str
                );
                Err(anyhow!("Sub-agent '{}' failed: {}", subagent_type, err_str))
            }
            SubAgentTaskOutcome::Panicked(panic_msg) => {
                log::error!(
                    "[spawn_subagent] sub-agent '{}' panicked: {}",
                    subagent_type,
                    panic_msg
                );
                Err(anyhow!(
                    "Sub-agent '{}' panicked: {}",
                    subagent_type,
                    panic_msg
                ))
            }
        }
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
        let (config, runtime_deps) = self
            .build_sub_agent_args(&request, &context, false, context.cancellation.clone())
            .await?;

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

    async fn launch_foreground_auto_background(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<SpawnForegroundAutoOutcome> {
        let agent_id = AgentId::new(uuid::Uuid::new_v4().to_string());
        let transcript_path = self.transcript_path_for_agent(&agent_id);
        let cancel_token = CancellationToken::new();
        let (gateway, tool_registry, app_settings) = self.build_run_components()?;
        let (config, runtime_deps) = self
            .build_sub_agent_args(&request, &context, false, cancel_token.clone())
            .await?;

        let parent_tool_use_id = context.parent_tool_use_id.as_str().to_owned();
        let parent_session_id = context.session_id.clone();
        let parent_run_id = context.parent_run_id.clone();
        let subagent_type = request.subagent_type.clone();
        let auto_background_after_ms = request
            .auto_background_after_ms
            .unwrap_or(DEFAULT_AGENT_AUTO_BACKGROUND_AFTER_MS)
            .max(1);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let mut done_rx = done_rx;
        let state = Arc::new(tokio::sync::Mutex::new(ForegroundPromotionState::Waiting(
            done_tx,
        )));

        let worker_state = state.clone();
        let worker_finish_ctx = SpawnBackgroundTaskCtx {
            task_store: self.task_store.clone(),
            notif_queue: self.notif_queue.clone(),
            event_bus: self.deps.event_bus.clone(),
            agent_id: agent_id.clone(),
            transcript_path: transcript_path.clone(),
            parent_tool_use_id: parent_tool_use_id.clone(),
            parent_session_id: parent_session_id.clone(),
            parent_run_id: parent_run_id.clone(),
            subagent_type: subagent_type.clone(),
        };
        let log_ctx = crate::log_context::LogContext::new(
            parent_session_id.as_str(),
            parent_run_id.as_ref().map(|r| r.as_str()).unwrap_or(""),
        )
        .with_agent(agent_id.as_str());

        tokio::spawn(async move {
            let outcome = Self::run_sub_agent_task(
                gateway,
                tool_registry,
                runtime_deps,
                config,
                app_settings,
                log_ctx,
            )
            .await;

            let mut guard = worker_state.lock().await;
            match std::mem::replace(&mut *guard, ForegroundPromotionState::Finished) {
                ForegroundPromotionState::Waiting(tx) => {
                    let _ = tx.send(outcome);
                }
                ForegroundPromotionState::Promoted => {
                    drop(guard);
                    finish_background_subagent(worker_finish_ctx, outcome).await;
                }
                ForegroundPromotionState::Finished => {}
            }
        });

        tokio::select! {
            result = &mut done_rx => {
                match result {
                    Ok(outcome) => Self::sync_result_from_outcome(&subagent_type, outcome),
                    Err(_) => Err(anyhow!("sub-agent worker finished without result")),
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(auto_background_after_ms)) => {
                let mut guard = state.lock().await;
                match &*guard {
                    ForegroundPromotionState::Waiting(_) => {
                        self.register_background_task(
                            &request,
                            &agent_id,
                            &transcript_path,
                            cancel_token.clone(),
                        );
                        *guard = ForegroundPromotionState::Promoted;
                        Ok(SpawnForegroundAutoOutcome::Backgrounded {
                            agent_id,
                            name: request.name.clone(),
                            auto_background_after_ms,
                        })
                    }
                    ForegroundPromotionState::Finished => {
                        drop(guard);
                        match done_rx.await {
                            Ok(outcome) => Self::sync_result_from_outcome(&subagent_type, outcome),
                            Err(_) => Err(anyhow!("sub-agent worker finished without result")),
                        }
                    }
                    ForegroundPromotionState::Promoted => unreachable!("foreground launch cannot re-enter promoted state"),
                }
            }
            _ = crate::runtime::cancellation::wait_for_cancellation(context.cancellation.clone()) => {
                let mut guard = state.lock().await;
                match &*guard {
                    ForegroundPromotionState::Waiting(_) => {
                        *guard = ForegroundPromotionState::Finished;
                        drop(guard);
                        cancel_token.cancel();
                        Err(anyhow!("Sub-agent '{}' cancelled before auto-background promotion", subagent_type))
                    }
                    ForegroundPromotionState::Finished => {
                        drop(guard);
                        match done_rx.await {
                            Ok(outcome) => Self::sync_result_from_outcome(&subagent_type, outcome),
                            Err(_) => Err(anyhow!("sub-agent worker finished without result")),
                        }
                    }
                    ForegroundPromotionState::Promoted => unreachable!("foreground launch cannot observe promoted state after returning"),
                }
            }
        }
    }

    async fn launch_async(
        &self,
        request: SpawnSubagentRequest,
        context: SpawnSubagentContext,
    ) -> Result<SpawnAsyncOutcome> {
        let (gateway, tool_registry, app_settings) = self.build_run_components()?;

        // Generate a fresh AgentId for this async sub-agent.
        let agent_id = AgentId::new(uuid::Uuid::new_v4().to_string());
        let transcript_path = self.transcript_path_for_agent(&agent_id);
        let transcript_path_for_task = transcript_path.clone();

        // Create a fresh cancellation token for this async sub-agent so that
        // TaskStop can cancel it independently of the parent run's token.
        let cancel_token = CancellationToken::new();
        let (config, runtime_deps) = self
            .build_sub_agent_args(&request, &context, true, cancel_token.clone())
            .await?;

        self.register_background_task(&request, &agent_id, &transcript_path, cancel_token);

        // Capture all Arcs + owned data for the spawned task (no &-references).
        let task_store = self.task_store.clone();
        let notif_queue = self.notif_queue.clone();
        let id_for_task = agent_id.clone();
        let parent_tool_use_id = context.parent_tool_use_id.as_str().to_owned();
        let parent_session_id = context.session_id.clone();
        let parent_run_id = context.parent_run_id.clone();
        let subagent_type = request.subagent_type.clone();
        let finish_ctx = SpawnBackgroundTaskCtx {
            task_store: task_store.clone(),
            notif_queue: notif_queue.clone(),
            event_bus: self.deps.event_bus.clone(),
            agent_id: id_for_task.clone(),
            transcript_path: transcript_path_for_task.clone(),
            parent_tool_use_id: parent_tool_use_id.clone(),
            parent_session_id: parent_session_id.clone(),
            parent_run_id: parent_run_id.clone(),
            subagent_type: subagent_type.clone(),
        };

        tokio::spawn(async move {
            let log_ctx = crate::log_context::LogContext::new(
                parent_session_id.as_str(),
                parent_run_id.as_ref().map(|r| r.as_str()).unwrap_or(""),
            )
            .with_agent(id_for_task.as_str());
            let outcome = Self::run_sub_agent_task(
                gateway,
                tool_registry,
                runtime_deps,
                config,
                app_settings,
                log_ctx,
            )
            .await;
            finish_background_subagent(finish_ctx, outcome).await;
        });

        Ok(SpawnAsyncOutcome {
            agent_id,
            name: request.name.clone(),
        })
    }

    async fn build_teammate_llm_engine(
        &self,
        context: &SpawnSubagentContext,
    ) -> Option<crate::runtime::agent::worker_runtime::TeammateLlmEngine> {
        let (gateway, tool_registry, app_settings) = match self.build_run_components() {
            Ok(parts) => parts,
            Err(e) => {
                log::warn!(
                    "[spawn_subagent] build_teammate_llm_engine: missing run components ({e}); Teammate will fall back to stub mode"
                );
                return None;
            }
        };

        let runtime_deps = crate::llm::sub_agent::SubAgentRuntimeDeps {
            storage: self.deps.storage.clone(),
            file_manager: self.deps.file_manager.clone(),
            workspace_path: self.deps.workspace_path.clone(),
            conversation_id: self.deps.conversation_id.clone(),
            session_id: self.deps.session_id.clone(),
            run_id: self.deps.run_id.clone(),
            agent_id: self.deps.agent_id.clone(),
            agent_runtime: self.deps.agent_runtime.clone(),
            event_bus: self.deps.event_bus.clone(),
            skill_registry: self.deps.skill_registry.clone(),
            authorized_workspace: self.deps.authorized_workspace.clone(),
            read_file_state: self.deps.read_file_state.clone(),
            app_handle: self.deps.app_handle.clone(),
            auth_manager: self.deps.auth_manager.clone(),
            runtime_resolver: self.deps.runtime_resolver.clone(),
            permission_ctx: context.permission_ctx.clone(),
            current_persona_id: self.deps.current_persona_id.clone(),
        };

        // LTR registries — pull from tauri AppHandle state (managed by lib.rs
        // at app boot).  Without these the Teammate's tools (SendMessage /
        // TaskList / TaskClaim / etc.) will be silently scoped to local
        // state only and never reach the Lead idle supervisor.
        use tauri::Manager as _;
        let (team_reg, names_reg, inbox_reg, lead_sup, cancel_reg) =
            if let Some(app) = self.deps.app_handle.as_ref() {
                (
                    app.try_state::<Arc<crate::runtime::agent::TeamRegistry>>()
                        .map(|s| s.inner().clone()),
                    app.try_state::<Arc<crate::runtime::agent::AgentNameRegistry>>()
                        .map(|s| s.inner().clone()),
                    app.try_state::<Arc<crate::runtime::agent::InboxRegistry>>()
                        .map(|s| s.inner().clone()),
                    app.try_state::<Arc<crate::runtime::agent::LeadIdleSupervisor>>()
                        .map(|s| s.inner().clone()),
                    app.try_state::<Arc<crate::runtime::agent::CancellationRegistry>>()
                        .map(|s| s.inner().clone()),
                )
            } else {
                (None, None, None, None, None)
            };
        log::info!(
            "[spawn_teammate][engine-build] team_reg={} names_reg={} inbox_reg={} lead_sup={} cancel_reg={}",
            team_reg.is_some(),
            names_reg.is_some(),
            inbox_reg.is_some(),
            lead_sup.is_some(),
            cancel_reg.is_some(),
        );

        Some(crate::runtime::agent::worker_runtime::TeammateLlmEngine {
            gateway,
            tool_registry,
            runtime_deps,
            settings: (*app_settings).clone(),
            // Per-turn iteration cap — bounds runaway tool loops per inbox
            // message. 25 mirrors SubAgentConfig's default max_iterations
            // for production sub-agents.
            max_iterations_per_turn: 25,
            team_registry: team_reg,
            agent_names: names_reg,
            inbox_registry: inbox_reg,
            lead_idle: lead_sup,
            cancellation_registry: cancel_reg,
        })
    }
}
