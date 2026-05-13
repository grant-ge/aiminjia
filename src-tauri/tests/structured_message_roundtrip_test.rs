//! P2.1: serde JSON roundtrip + wire-shape assertions for StructuredMessage.
//!
//! Locks down the wire format the LLM produces / consumes:
//! - `type` is the discriminator field
//! - all variant names are snake_case
//! - all 5 variants survive a JSON roundtrip without data loss

use app_lib::runtime::messaging::StructuredMessage;
use serde_json::json;

fn roundtrip(msg: StructuredMessage) -> StructuredMessage {
    let s = serde_json::to_string(&msg).unwrap();
    serde_json::from_str(&s).unwrap()
}

#[test]
fn text_variant_wire_shape() {
    let msg = StructuredMessage::text("hello");
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["type"], "text");
    assert_eq!(v["content"], "hello");

    let back = roundtrip(msg);
    assert!(matches!(back, StructuredMessage::Text { content } if content == "hello"));
}

#[test]
fn shutdown_request_wire_shape() {
    let msg = StructuredMessage::ShutdownRequest {
        reason: Some("task done".into()),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["type"], "shutdown_request");
    assert_eq!(v["reason"], "task done");

    let back = roundtrip(msg);
    match back {
        StructuredMessage::ShutdownRequest { reason } => {
            assert_eq!(reason.as_deref(), Some("task done"));
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}

#[test]
fn shutdown_request_omits_reason_when_none() {
    let msg = StructuredMessage::ShutdownRequest { reason: None };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["type"], "shutdown_request");
    assert!(
        v.get("reason").is_none(),
        "None reason should be skipped, got: {v}"
    );
}

#[test]
fn shutdown_response_wire_shape() {
    let msg = StructuredMessage::ShutdownResponse {
        request_id: "req-1".into(),
        approve: true,
        reason: None,
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["type"], "shutdown_response");
    assert_eq!(v["request_id"], "req-1");
    assert_eq!(v["approve"], true);

    let back = roundtrip(msg);
    assert!(matches!(
        back,
        StructuredMessage::ShutdownResponse { ref request_id, approve: true, reason: None }
            if request_id == "req-1"
    ));
}

#[test]
fn plan_approval_request_wire_shape() {
    let msg = StructuredMessage::PlanApprovalRequest {
        request_id: "req-2".into(),
        plan: "refactor X then Y".into(),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["type"], "plan_approval_request");
    assert_eq!(v["request_id"], "req-2");
    assert_eq!(v["plan"], "refactor X then Y");

    let back = roundtrip(msg);
    assert!(matches!(
        back,
        StructuredMessage::PlanApprovalRequest { ref request_id, ref plan }
            if request_id == "req-2" && plan == "refactor X then Y"
    ));
}

#[test]
fn plan_approval_response_wire_shape() {
    let msg = StructuredMessage::PlanApprovalResponse {
        request_id: "req-3".into(),
        approve: false,
        feedback: Some("missed edge case".into()),
    };
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["type"], "plan_approval_response");
    assert_eq!(v["approve"], false);
    assert_eq!(v["feedback"], "missed edge case");

    let back = roundtrip(msg);
    assert!(matches!(
        back,
        StructuredMessage::PlanApprovalResponse { ref request_id, approve: false, ref feedback }
            if request_id == "req-3" && feedback.as_deref() == Some("missed edge case")
    ));
}

#[test]
fn unknown_type_value_fails_to_parse() {
    let v = json!({ "type": "totally_made_up", "content": "x" });
    let parsed: Result<StructuredMessage, _> = serde_json::from_value(v);
    assert!(
        parsed.is_err(),
        "unknown discriminator must error, not silently accept"
    );
}
