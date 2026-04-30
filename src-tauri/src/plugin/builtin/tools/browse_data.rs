//! browse_data — delegate data extraction from internal systems to a browser sub-agent.
//!
//! The main agent calls this tool with a task description. A sub-agent with
//! its own system prompt and browser tools handles the multi-step browsing,
//! then returns file paths and a summary.

// This builtin tool implements the deprecated ToolPlugin trait.
// It is intentionally in the legacy zone and will be migrated to RuntimeTool.
#![allow(deprecated)]

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};
use crate::runtime::tools::builtin::browse_data::BrowseDataLaunchResult;

fn map_browse_data_launch_result(result: BrowseDataLaunchResult) -> Result<ToolOutput, ToolError> {
    if let Some(decision) = result.ask_decision {
        return Err(ToolError::AskRequired(decision));
    }
    Ok(ToolOutput::success(result.content))
}

pub struct BrowseDataTool;

#[async_trait]
impl ToolPlugin for BrowseDataTool {
    fn name(&self) -> &str {
        "browse_data"
    }

    fn description(&self) -> &str {
        "Extract data from internal business systems (ERP/OA/CRM/HR). \
         Describe what data you need (system URL, data type, time range, filters). \
         An AI browser agent will automatically navigate the system, discover APIs, \
         extract data and save it as a JSON file. Returns the file path — use \
         execute_python to load and process the data. \
         \n\nExample: browse_data(task=\"从 zeus.renlijia.com 拉取本月订单数据\", \
         url=\"https://zeus.renlijia.com\")"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "What data to extract, e.g. '从 zeus 系统拉取本月订单数据'"
                },
                "url": {
                    "type": "string",
                    "description": "Target system URL (optional, if known)"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::execute_browse_data(ctx, &input).await {
            Ok(result) => map_browse_data_launch_result(result),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::builtin::browse_data::BrowseDataLaunchResult;
    use crate::runtime::tools::permission::{
        default_permission_ask, PermissionDecision, PermissionReason,
    };

    #[test]
    fn map_browse_data_launch_result_preserves_ask_required() {
        let result =
            map_browse_data_launch_result(BrowseDataLaunchResult::ask(PermissionDecision::Ask {
                message: "need approval".to_string(),
                suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
                remember_options: default_permission_ask().0,
                default_destination: default_permission_ask().1,
                reason: PermissionReason::UnknownScope,
            }));

        match result.expect_err("ask decision must stay structured") {
            ToolError::AskRequired(PermissionDecision::Ask {
                message,
                suggestions,
                ..
            }) => {
                assert_eq!(message, "need approval");
                assert_eq!(
                    suggestions,
                    vec!["Allow once".to_string(), "Deny".to_string()]
                );
            }
            other => panic!("expected ToolError::AskRequired, got: {:?}", other),
        }
    }
}
