//! AskUserQuestionRuntimeTool — structured user question tool.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::runtime::interaction::{InteractionId, InteractionKind, InteractionRequest};
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct AskUserQuestionRuntimeTool;

#[async_trait]
impl RuntimeTool for AskUserQuestionRuntimeTool {
    fn id(&self) -> &str { "AskUserQuestion" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        TOOL_CATALOG
            .get("AskUserQuestion")
            .unwrap_or_else(|| ToolDefinition::new("AskUserQuestion", "向用户提问"))
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(resolution) = ctx.interaction_resolution.as_ref() {
            let questions = input.get("questions").cloned().unwrap_or_else(|| json!([]));
            let answers = resolution
                .get("answers")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let annotations = resolution.get("annotations").cloned();

            let mut result_data = json!({
                "questions": questions,
                "answers": answers,
            });
            if let Some(annotations) = annotations {
                result_data["annotations"] = annotations;
            }

            let answers_text = answers
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .map(|(question, answer)| format!("\"{}\"={}", question, answer))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();

            return Ok(ToolResult::new(
                "AskUserQuestion",
                format!(
                    "User has answered your questions: {}. You can now continue with the user's answers in mind.",
                    answers_text
                ),
                Some(result_data),
            ));
        }

        let questions = input
            .get("questions")
            .ok_or_else(|| ToolError::InputValidationError {
                tool_name: "AskUserQuestion".into(),
                message: "missing 'questions' field".into(),
            })?;
        let q_len = questions.as_array().map(|items| items.len()).unwrap_or(0);
        if !(1..=4).contains(&q_len) {
            return Err(ToolError::InputValidationError {
                tool_name: "AskUserQuestion".into(),
                message: format!("questions must have 1-4 items, got {}", q_len),
            });
        }

        let original_request = ctx.current_tool_call_request.clone().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "AskUserQuestion: missing current_tool_call_request in context".into(),
            )
        })?;
        let interaction_request = InteractionRequest {
            interaction_id: InteractionId::new(Uuid::new_v4().to_string()),
            session_id: ctx.session_id.clone(),
            run_id: ctx.run_id.clone(),
            tool_call_id: ctx.tool_call_id.clone(),
            tool_name: "AskUserQuestion".into(),
            kind: InteractionKind::AskUserQuestion,
            payload: json!({
                "questions": questions,
                "metadata": input.get("metadata").cloned().unwrap_or(Value::Null),
            }),
            original_request,
        };

        Err(ToolError::InteractionRequired(Box::new(
            interaction_request,
        )))
    }
}
