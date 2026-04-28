// LegacyToolAdapter intentionally wraps the deprecated ToolPlugin trait.
// The deprecation warning is expected and suppressed here — this is the
// *only* place where the two worlds are allowed to touch.
#![allow(deprecated)]

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::FutureExt;
use serde_json::Value;

use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::ToolPlugin;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub type LegacyHandler = dyn Fn(Value, ToolExecutionContext) -> BoxFuture<'static, Result<ToolResult, ToolError>>
    + Send
    + Sync;

pub struct LegacyToolAdapter {
    definition: ToolDefinition,
    handler: Arc<LegacyHandler>,
}

impl LegacyToolAdapter {
    pub fn new(definition: ToolDefinition, handler: Arc<LegacyHandler>) -> Self {
        Self {
            definition,
            handler,
        }
    }

    pub fn for_test(tool_name: &str) -> Self {
        let tool_name_owned = tool_name.to_string();
        let definition =
            ToolDefinition::new(tool_name_owned.clone(), format!("test tool {tool_name}"));
        let handler_name = tool_name_owned.clone();
        Self::new(
            definition,
            Arc::new(move |input, _ctx| {
                let tool_name = handler_name.clone();
                async move {
                    let content = input.to_string();
                    Ok(ToolResult {
                        tool_name,
                        content,
                        data: Some(input),
                        file_meta: None,
                        is_degraded: false,
                        degradation_notice: None,
                    })
                }
                .boxed()
            }),
        )
    }

    pub fn from_plugin(plugin: Arc<dyn ToolPlugin>, plugin_ctx: PluginContext) -> Self {
        let definition = ToolDefinition::new(plugin.name(), plugin.description());
        Self::new(
            definition,
            Arc::new(move |input, exec_ctx| {
                let plugin = plugin.clone();
                let mut plugin_ctx = plugin_ctx.clone();
                async move {
                    plugin_ctx.session_id = exec_ctx.session_id.clone();
                    plugin_ctx.run_id = Some(exec_ctx.run_id.clone());
                    plugin_ctx.agent_id = exec_ctx.agent_id.clone();
                    plugin_ctx.cancellation = Some(exec_ctx.cancellation.clone());
                    plugin_ctx.permission_mode = exec_ctx.permission_mode;
                    let output =
                        plugin
                            .execute(&plugin_ctx, input)
                            .await
                            .map_err(|err| match err {
                                crate::plugin::tool_trait::ToolError::AskRequired(decision) => {
                                    ToolError::AskRequired(decision)
                                }
                                crate::plugin::tool_trait::ToolError::PermissionDenied(message) => {
                                    ToolError::PermissionDenied(message)
                                }
                                other => ToolError::Other(anyhow::anyhow!(other.to_string())),
                            })?;
                    Ok(ToolResult {
                        tool_name: plugin.name().to_string(),
                        content: output.content,
                        data: output.data,
                        file_meta: output.file_meta,
                        is_degraded: output.is_degraded,
                        degradation_notice: output.degradation_notice,
                    })
                }
                .boxed()
            }),
        )
    }
}

#[async_trait]
impl RuntimeTool for LegacyToolAdapter {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        (self.handler)(input, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::tool_trait::{ToolError as LegacyToolError, ToolOutput};
    use crate::runtime::tools::permission::PermissionMode;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct CapturePermissionModePlugin {
        seen_mode: Arc<Mutex<Option<PermissionMode>>>,
    }

    #[async_trait]
    impl ToolPlugin for CapturePermissionModePlugin {
        fn name(&self) -> &str {
            "capture_permission_mode"
        }

        fn description(&self) -> &str {
            "capture permission mode"
        }

        fn input_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {},
            })
        }

        async fn execute(
            &self,
            ctx: &PluginContext,
            _input: Value,
        ) -> Result<ToolOutput, LegacyToolError> {
            *self.seen_mode.lock().unwrap() = Some(ctx.permission_mode);
            Ok(ToolOutput::success("ok"))
        }
    }

    #[test]
    fn from_plugin_copies_runtime_permission_mode_into_plugin_context() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let seen_mode = Arc::new(Mutex::new(None));
        let plugin = Arc::new(CapturePermissionModePlugin {
            seen_mode: seen_mode.clone(),
        });
        let plugin_ctx = PluginContext {
            storage: Arc::new(
                crate::storage::file_store::AppStorage::new(temp_dir.path()).unwrap(),
            ),
            file_manager: Arc::new(crate::storage::file_manager::FileManager::new(
                temp_dir.path(),
            )),
            workspace_path: temp_dir.path().to_path_buf(),
            conversation_id: "conv-legacy-mode".to_string(),
            session_id: crate::runtime::ids::SessionId::new("conv-legacy-mode"),
            run_id: None,
            agent_id: None,
            tavily_api_key: None,
            bocha_api_key: None,
            app_handle: None,
            session_manager: Arc::new(crate::python::session::PythonSessionManager::new(
                temp_dir.path().to_path_buf(),
                None,
            )),
            auth_manager: None,
            connector_engine: None,
            use_cloud: false,
            model: String::new(),
            gateway: None,
            tool_registry: None,
            app_settings: None,
            agent_runtime: None,
            event_bus: None,
            skill_registry: None,
            authorized_workspace: None,
            read_file_state: None,
            cancellation: None,
            permission_mode: PermissionMode::Default,
            runtime_resolver: None,
        };
        let adapter = LegacyToolAdapter::from_plugin(plugin, plugin_ctx);
        let runtime_ctx =
            ToolExecutionContext::for_test("conv-legacy-mode", "run-legacy-mode", "tc-legacy")
                .with_permission_mode(PermissionMode::DontAsk);

        futures::executor::block_on(adapter.execute(json!({}), runtime_ctx)).unwrap();

        assert_eq!(*seen_mode.lock().unwrap(), Some(PermissionMode::DontAsk));
    }
}
