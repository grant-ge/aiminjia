use std::sync::Arc;

use app_lib::runtime::chat::tool_round_types::{RuntimeToolCallOutcome, RuntimeToolCallRequest};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::builtin::browse_data::{
    BrowseDataLaunchContext, BrowseDataLaunchRequest, BrowseDataLaunchResult, BrowseDataLauncher,
    BrowseDataRuntimeTool,
};
use app_lib::runtime::tools::permission::{
    default_permission_ask, PermissionDecision, PermissionReason,
};
use app_lib::runtime::tools::{AllowAllPermissionPipeline, ToolDispatcher};
use async_trait::async_trait;
use serde_json::json;
use tempfile::TempDir;

struct AskingBrowseDataLauncher;

#[async_trait]
impl BrowseDataLauncher for AskingBrowseDataLauncher {
    async fn launch(
        &self,
        _request: BrowseDataLaunchRequest,
        _context: BrowseDataLaunchContext,
    ) -> anyhow::Result<BrowseDataLaunchResult> {
        Ok(BrowseDataLaunchResult::ask(PermissionDecision::Ask {
            message: "subagent needs permission to continue".to_string(),
            suggestions: vec![
                "Allow once".to_string(),
                "Always allow".to_string(),
                "Deny".to_string(),
            ],
            remember_options: default_permission_ask().0,
            default_destination: default_permission_ask().1,
            reason: PermissionReason::Other("subagent_inner_tool".to_string()),
            path_auth_scope: None,
        }))
    }
}

#[tokio::test]
async fn browse_data_runtime_tool_surfaces_structured_ask_without_completed_event() {
    let tmp = TempDir::new().expect("TempDir::new failed");
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(BrowseDataRuntimeTool::with_launcher(Arc::new(
        AskingBrowseDataLauncher,
    ))));

    let engine =
        QueryEngine::with_dispatcher(dispatcher).with_workspace_path(tmp.path().to_path_buf());
    let mapping = IdentityMapping::from_legacy_conversation_id("subagent-ask-conv".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-subagent-ask"), "ask".to_string());
    let bus = RuntimeEventBus::new();

    let outcome = engine
        .run_tool_call_with_bus(
            &turn,
            &bus,
            RuntimeToolCallRequest {
                tool_call_id: "tc-subagent-ask".to_string(),
                tool_name: "browse_data".to_string(),
                args: json!({ "task": "抓取订单" }),
                purpose: None,
            },
        )
        .await
        .expect("query engine should surface structured ask");

    match outcome {
        RuntimeToolCallOutcome::AskRequired {
            tool_call_id,
            tool_name,
            decision,
            ..
        } => {
            assert_eq!(tool_call_id, "tc-subagent-ask");
            assert_eq!(tool_name, "browse_data");
            match decision {
                PermissionDecision::Ask {
                    message,
                    suggestions,
                    ..
                } => {
                    assert_eq!(message, "subagent needs permission to continue");
                    assert_eq!(
                        suggestions,
                        vec![
                            "Allow once".to_string(),
                            "Always allow".to_string(),
                            "Deny".to_string(),
                        ]
                    );
                }
                other => panic!("expected ask decision, got: {:?}", other),
            }
        }
        other => panic!("expected AskRequired outcome, got: {:?}", other),
    }

    assert!(
        !bus.recorded().iter().any(|event| {
            matches!(
                &event.kind,
                RuntimeEventKind::ToolCallCompleted { tool_call_id, .. }
                    if tool_call_id.as_str() == "tc-subagent-ask"
            )
        }),
        "AskRequired must not emit ToolCallCompleted before the parent resolves the request"
    );
}
