use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use crate::runtime::chat::ChatTurnRequest;
use crate::runtime::human_interaction::{
    AskQuestionSpec, HumanInteractionControlPlane, HumanInteractionId,
    HumanInteractionJudgeDecision, HumanInteractionKind, HumanInteractionRef,
    HumanInteractionRouter, HumanInteractionStatus, HumanReplyRoute, ImPlatform, JudgeAction,
    JudgeKind, OutputBinding, PermissionAskSpec, PermissionDecisionIntent, TurnOrigin,
};
use crate::runtime::ids::{RunId, SessionId, ToolCallId};
use crate::runtime::interaction::{
    InteractionId, InteractionKind, InteractionRequest, PendingInteractionControlPlane,
};
use crate::runtime::store::{PendingPermissionControlPlane, PendingPermissionRequest};
use crate::runtime::tools::permission::PermissionMode;
use std::sync::Arc;

#[test]
fn chat_turn_request_defaults_to_app_origin_and_app_only_output() {
    let request = ChatTurnRequest::new("session-1", "hello", Vec::new());

    assert_eq!(request.turn_origin, TurnOrigin::App);
    assert_eq!(request.output_binding, OutputBinding::AppOnly);
}

#[test]
fn im_output_binding_preserves_platform_and_target() {
    let binding = OutputBinding::im(ImPlatform::Dingtalk, "session-1", "conversation-1", true);

    match binding {
        OutputBinding::Im {
            platform,
            target,
            allow_streaming_reply,
        } => {
            assert_eq!(platform, ImPlatform::Dingtalk);
            assert_eq!(target.session_id, "session-1");
            assert_eq!(target.external_conversation_key, "conversation-1");
            assert!(allow_streaming_reply);
        }
        OutputBinding::AppOnly => panic!("expected IM binding"),
    }
}

fn ask_ref(kind: HumanInteractionKind) -> HumanInteractionRef {
    HumanInteractionRef {
        id: HumanInteractionId::new("hi-1"),
        session_id: SessionId::new("sess"),
        run_id: RunId::new("run"),
        tool_call_id: ToolCallId::new("tool"),
        kind,
        turn_origin: TurnOrigin::App,
        output_binding: OutputBinding::AppOnly,
        status: HumanInteractionStatus::Pending,
    }
}

#[test]
fn ask_user_question_free_text_is_consumed_as_answer() {
    let route = HumanInteractionRouter::route_ask_user_question(
        &ask_ref(HumanInteractionKind::AskUserQuestion),
        &AskQuestionSpec {
            questions: vec!["专业领域".into()],
        },
        "HR/人事",
    );

    match route {
        HumanReplyRoute::ResolveAskUserQuestion { answers, raw_text } => {
            assert_eq!(raw_text, "HR/人事");
            assert_eq!(answers.get("专业领域").unwrap(), "HR/人事");
        }
        other => panic!("unexpected route: {other:?}"),
    }
}

#[test]
fn ask_user_question_topic_change_abandons_and_starts_new_turn() {
    let route = HumanInteractionRouter::route_ask_user_question(
        &ask_ref(HumanInteractionKind::AskUserQuestion),
        &AskQuestionSpec {
            questions: vec!["专业领域".into()],
        },
        "算了，看看别的文件",
    );

    assert!(matches!(
        route,
        HumanReplyRoute::AbandonAndStartNewTurn { .. }
    ));
}

#[test]
fn permission_allow_once_is_structured_intent() {
    let route = HumanInteractionRouter::route_permission_reply(
        &ask_ref(HumanInteractionKind::PermissionAsk),
        &PermissionAskSpec {
            tool_name: "Read".into(),
            requested_path: Some("/private/tmp/aijia-permission-test/secret.txt".into()),
            current_scope: Some("path:/private/tmp/aijia-permission-test".into()),
        },
        "好的，那就允许你访问一次吧",
    );

    assert!(matches!(
        route,
        HumanReplyRoute::ResolvePermission {
            intent: PermissionDecisionIntent::AllowOnce
        }
    ));
}

#[test]
fn permission_reply_explicit_deny_is_not_llm_judge_work() {
    let route = HumanInteractionRouter::route_permission_reply(
        &ask_ref(HumanInteractionKind::PermissionAsk),
        &PermissionAskSpec {
            tool_name: "Read".into(),
            requested_path: Some("/private/tmp/aijia-permission-test/secret3.txt".into()),
            current_scope: None,
        },
        "好的，先拒绝吧",
    );

    assert_eq!(
        route,
        HumanReplyRoute::ResolvePermission {
            intent: PermissionDecisionIntent::Deny { reason: None }
        }
    );
}

#[test]
fn permission_reply_new_topic_abandons_permission_and_starts_new_turn() {
    let route = HumanInteractionRouter::route_permission_reply(
        &ask_ref(HumanInteractionKind::PermissionAsk),
        &PermissionAskSpec {
            tool_name: "Read".into(),
            requested_path: Some("/private/tmp/aijia-permission-test/secret3.txt".into()),
            current_scope: None,
        },
        "问我三个问题",
    );

    match route {
        HumanReplyRoute::AbandonAndStartNewTurn { text, .. } => assert_eq!(text, "问我三个问题"),
        other => panic!("expected abandon route, got {other:?}"),
    }
}

#[test]
fn ask_user_question_plain_multiline_answer_is_submitted_directly() {
    let route = HumanInteractionRouter::route_ask_user_question(
        &ask_ref(HumanInteractionKind::AskUserQuestion),
        &AskQuestionSpec {
            questions: vec!["专业领域".into(), "输出风格".into()],
        },
        "HR/人事\n结论优先",
    );

    match route {
        HumanReplyRoute::ResolveAskUserQuestion { answers, raw_text } => {
            assert_eq!(raw_text, "HR/人事\n结论优先");
            assert_eq!(answers["专业领域"], "HR/人事");
            assert_eq!(answers["输出风格"], "结论优先");
        }
        other => panic!("expected ask-user-question resolution, got {other:?}"),
    }
}

#[test]
fn judge_decision_must_parse_to_structured_schema() {
    let parsed = HumanInteractionJudgeDecision::parse_json(
        r#"{
            "action": "resolve",
            "kind": "permission",
            "payload": { "decision": "allow_once" },
            "reason": "user approved once"
        }"#,
    )
    .expect("valid judge schema");

    assert_eq!(parsed.action, JudgeAction::Resolve);
    assert_eq!(parsed.kind, JudgeKind::Permission);
    assert_eq!(parsed.payload["decision"], "allow_once");
}

#[test]
fn interaction_ref_preserves_origin_and_output_binding() {
    let origin = TurnOrigin::Im {
        platform: ImPlatform::Feishu,
        external_conversation_key: "chat-1".into(),
        sender_id: Some("sender-1".into()),
        sender_label: Some("飞书用户".into()),
        account_id: Some("bot-1".into()),
        thread_id: None,
    };
    let binding = OutputBinding::im(ImPlatform::Feishu, "sess", "chat-1", true);
    let interaction = HumanInteractionRef {
        id: HumanInteractionId::new("hi-1"),
        session_id: SessionId::new("sess"),
        run_id: RunId::new("run"),
        tool_call_id: ToolCallId::new("tool"),
        kind: HumanInteractionKind::AskUserQuestion,
        turn_origin: origin.clone(),
        output_binding: binding.clone(),
        status: HumanInteractionStatus::Pending,
    };

    assert_eq!(interaction.turn_origin, origin);
    assert_eq!(interaction.output_binding, binding);
}

#[test]
fn control_plane_lists_permission_and_question_for_session() {
    let interactions =
        Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
    let permissions = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());

    let original_request = RuntimeToolCallRequest {
        tool_call_id: "tool-1".into(),
        tool_name: "AskUserQuestion".into(),
        args: serde_json::json!({}),
        purpose: None,
    };
    let _rx = interactions
        .insert_pending(InteractionRequest {
            interaction_id: InteractionId::new("ask-1"),
            session_id: SessionId::new("sess"),
            run_id: RunId::new("run-ask"),
            tool_call_id: ToolCallId::new("tool-1"),
            tool_name: "AskUserQuestion".into(),
            kind: InteractionKind::AskUserQuestion,
            payload: serde_json::json!({"questions":[]}),
            original_request: original_request.clone(),
            turn_origin: TurnOrigin::App,
            output_binding: OutputBinding::AppOnly,
        })
        .unwrap();
    let _permission_rx = permissions
        .insert_pending_request(PendingPermissionRequest {
            tool_call_id: ToolCallId::new("perm-1"),
            session_id: SessionId::new("sess"),
            run_id: RunId::new("run-perm"),
            tool_name: "Read".into(),
            capability_scopes: vec![],
            message: "read file".into(),
            suggestions: vec![],
            mode: PermissionMode::Default,
            remember_options: vec![],
            default_destination: None,
            original_request,
            turn_origin: TurnOrigin::App,
            output_binding: OutputBinding::AppOnly,
            path_auth_scope: None,
        })
        .unwrap();

    let control_plane = HumanInteractionControlPlane::new(interactions, permissions);
    let refs = control_plane.pending_for_session("sess");

    assert_eq!(refs.len(), 2);
    assert!(refs
        .iter()
        .any(|item| item.kind == HumanInteractionKind::AskUserQuestion));
    assert!(refs
        .iter()
        .any(|item| item.kind == HumanInteractionKind::PermissionAsk));
}
