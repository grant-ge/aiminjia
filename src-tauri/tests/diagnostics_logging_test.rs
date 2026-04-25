use app_lib::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticLevel, DiagnosticSource};
use tempfile::TempDir;

#[test]
fn frontend_diagnostic_event_serializes_queryable_keys() {
    let event = DiagnosticEvent::new("ipc.invoke.started", DiagnosticSource::Frontend)
        .level(DiagnosticLevel::Debug)
        .ok(true)
        .conversation_id("conv_test")
        .run_id("run_test")
        .message_id("msg_test")
        .client_message_id("client_msg_test")
        .tool_call_id("tool_test")
        .agent_id("agent_test")
        .interaction_id("interaction_test")
        .task_id("task_test")
        .command("send_message")
        .duration_ms(42)
        .elapsed_ms(84)
        .payload(serde_json::json!({"argBytes": 25}));

    let value = serde_json::to_value(event).unwrap();
    assert_eq!(value["category"], "diagnostics");
    assert_eq!(value["source"], "frontend");
    assert_eq!(value["level"], "debug");
    assert_eq!(value["event"], "ipc.invoke.started");
    assert_eq!(value["ok"], true);
    assert_eq!(value["conversationId"], "conv_test");
    assert_eq!(value["runId"], "run_test");
    assert_eq!(value["messageId"], "msg_test");
    assert_eq!(value["clientMessageId"], "client_msg_test");
    assert_eq!(value["toolCallId"], "tool_test");
    assert_eq!(value["agentId"], "agent_test");
    assert_eq!(value["interactionId"], "interaction_test");
    assert_eq!(value["taskId"], "task_test");
    assert_eq!(value["command"], "send_message");
    assert_eq!(value["durationMs"], 42);
    assert_eq!(value["elapsedMs"], 84);
    assert_eq!(value["payload"]["argBytes"], 25);
}

#[test]
fn diagnostics_can_share_metrics_jsonl_file_and_export_all() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path();

    app_lib::telemetry::record(
        "tool",
        workspace,
        &[("name", "read_file"), ("status", "ok")],
    );
    record_diagnostic(
        workspace,
        DiagnosticEvent::new("chat.submit.started", DiagnosticSource::Frontend)
            .conversation_id("conv_test")
            .run_id("run_test"),
    );

    let raw = std::fs::read_to_string(workspace.join("logs").join("metrics.jsonl")).unwrap();
    assert_eq!(raw.lines().count(), 2);
    assert!(raw.contains("chat.submit.started"));
    assert!(raw.contains("conversationId"));

    let (json, count) = app_lib::telemetry::export_all(workspace).unwrap();
    assert_eq!(count, 2);

    let exported: serde_json::Value = serde_json::from_str(&json).unwrap();
    let entries = exported.as_array().unwrap();
    assert!(entries
        .iter()
        .any(|entry| entry["category"] == "tool" && entry["fields"]["name"] == "read_file"));
    assert!(entries.iter().any(|entry| {
        entry["category"] == "diagnostics"
            && entry["source"] == "frontend"
            && entry["event"] == "chat.submit.started"
            && entry["conversationId"] == "conv_test"
            && entry["runId"] == "run_test"
    }));
}

#[test]
fn backend_diagnostic_writes_queryable_jsonl() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path();

    record_diagnostic(
        workspace,
        DiagnosticEvent::new("backend.command.started", DiagnosticSource::Backend)
            .conversation_id("conv-1")
            .run_id("run-1")
            .tool_call_id("tool-1")
            .payload(serde_json::json!({"toolName":"send_message"})),
    );

    let raw = std::fs::read_to_string(workspace.join("logs/metrics.jsonl")).unwrap();
    assert!(!raw.contains('\t'));
    let value: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
    assert_eq!(value["category"], "diagnostics");
    assert_eq!(value["event"], "backend.command.started");
    assert_eq!(value["source"], "backend");
    assert_eq!(value["conversationId"], "conv-1");
    assert_eq!(value["runId"], "run-1");
    assert_eq!(value["toolCallId"], "tool-1");
    assert_eq!(value["payload"]["toolName"], "send_message");
}
