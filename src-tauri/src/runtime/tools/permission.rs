use anyhow::Result;
use serde_json::Value;

use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;

pub trait PermissionPipeline: Send + Sync {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Result<()>;
}

#[derive(Clone, Default)]
pub struct AllowAllPermissionPipeline;

impl PermissionPipeline for AllowAllPermissionPipeline {
    fn authorize(
        &self,
        _definition: &ToolDefinition,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Result<()> {
        Ok(())
    }
}
