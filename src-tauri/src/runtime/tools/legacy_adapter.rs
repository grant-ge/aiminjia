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
            Arc::new(move |input, _ctx| {
                let plugin = plugin.clone();
                let plugin_ctx = plugin_ctx.clone();
                async move {
                    let output = plugin
                        .execute(&plugin_ctx, input)
                        .await
                        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                    Ok(ToolResult {
                        tool_name: plugin.name().to_string(),
                        content: output.content,
                        data: output.data,
                        file_meta: output.file_meta,
                        is_degraded: output.is_degraded,
                        degradation_notice: output.degradation_notice,
                    })
                }
                .map(|result| result.map_err(ToolError::Other))
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
