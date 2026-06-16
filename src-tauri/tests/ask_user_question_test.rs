//! Integration tests for AskUserQuestionRuntimeTool + Interaction Runtime.

use app_lib::runtime::interaction::{
    InMemoryInteractionControlPlane, InteractionResolution, PendingInteractionControlPlane,
};
use app_lib::runtime::human_interaction::{OutputBinding, TurnOrigin};
use app_lib::runtime::tools::catalog::{DAILY_ALLOWED_TOOLS, TOOL_CATALOG};
use serde_json::json;

#[test]
fn ask_user_question_catalog_entry_exists() {
    let entry = TOOL_CATALOG.get_entry("AskUserQuestion");
    assert!(entry.is_some(), "AskUserQuestion must be in TOOL_CATALOG");
}

#[test]
fn ask_user_question_in_daily_allowed_tools() {
    assert!(
        DAILY_ALLOWED_TOOLS.contains(&"AskUserQuestion"),
        "AskUserQuestion should be in DAILY_ALLOWED_TOOLS"
    );
}

#[tokio::test]
async fn interaction_control_plane_submit_resolves() {
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use app_lib::runtime::ids::{RunId, SessionId, ToolCallId};
    use app_lib::runtime::interaction::types::{
        InteractionId, InteractionKind, InteractionRequest,
    };

    let cp = InMemoryInteractionControlPlane::new();
    let req = InteractionRequest {
        interaction_id: InteractionId::new("i-test-1"),
        session_id: SessionId::from("sess-test".to_string()),
        run_id: RunId::new("run-test"),
        tool_call_id: ToolCallId::new("tc-test"),
        tool_name: "AskUserQuestion".into(),
        kind: InteractionKind::AskUserQuestion,
        payload: json!({ "questions": [] }),
        original_request: RuntimeToolCallRequest {
            tool_call_id: "tc-test".into(),
            tool_name: "AskUserQuestion".into(),
            args: json!({}),
            purpose: None,
        },
        turn_origin: TurnOrigin::App,
        output_binding: OutputBinding::AppOnly,
    };

    let rx = cp.insert_pending(req).unwrap();
    cp.resolve(
        &InteractionId::new("i-test-1"),
        InteractionResolution::Submit {
            value: json!({ "answers": { "Which approach?": "Option A" } }),
        },
    )
    .unwrap();

    let resolution = rx.await.unwrap();
    assert!(matches!(resolution, InteractionResolution::Submit { .. }));
}
