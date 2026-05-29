use app_lib::runtime::chat::ChatTurnOutcome;
use app_lib::runtime::events::{
    AgentIdleScope, RunningTool, RuntimeEvent, RuntimeEventKind, TurnStage,
};
use app_lib::runtime::ids::{AgentId, RunId, SessionId, TaskId, ToolCallId};
use app_lib::runtime::tools::permission::{PermissionDestination, PermissionMode};
use app_lib::transport::tauri_event_adapter::map_runtime_event;
use serde_json::json;

fn event(kind: RuntimeEventKind) -> RuntimeEvent {
    RuntimeEvent::new(SessionId::new("conv-123"), RunId::new("run-456"), kind)
}

fn mapped(kind: RuntimeEventKind) -> app_lib::transport::tauri_event_adapter::LegacyEvent {
    map_runtime_event(&event(kind)).expect("runtime event should map to legacy event")
}

#[test]
fn stream_delta_maps_to_streaming_delta_with_content_and_run_id() {
    let legacy = mapped(RuntimeEventKind::StreamDelta {
        content: "Hello".to_string(),
    });

    assert_eq!(legacy.name, "streaming:delta");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["delta"], "Hello");
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn stream_done_maps_to_streaming_done_with_conversation_and_run_id() {
    let legacy = mapped(RuntimeEventKind::StreamDone);

    assert_eq!(legacy.name, "streaming:done");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn stream_error_maps_to_streaming_error_with_error_and_raw_error() {
    let legacy = mapped(RuntimeEventKind::StreamError {
        error: "LLM 超时".to_string(),
        raw_error: Some("upstream timeout".to_string()),
    });

    assert_eq!(legacy.name, "streaming:error");
    assert_eq!(legacy.payload["error"], "LLM 超时");
    assert_eq!(legacy.payload["rawError"], "upstream timeout");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn tool_call_executing_maps_to_tool_executing_with_tool_id_name_and_input() {
    let legacy = mapped(RuntimeEventKind::ToolCallExecuting {
        tool_call_id: ToolCallId::new("tc-001"),
        tool_name: "file_write".to_string(),
        input: json!({}),
    });

    assert_eq!(legacy.name, "tool:executing");
    assert_eq!(legacy.payload["toolName"], "file_write");
    assert_eq!(legacy.payload["toolId"], "tc-001");
    assert_eq!(legacy.payload["input"], json!({}));
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn tool_call_completed_success_maps_to_full_tool_completed_payload() {
    let legacy = mapped(RuntimeEventKind::ToolCallCompleted {
        tool_call_id: ToolCallId::new("tc-001"),
        tool_name: "file_write".to_string(),
        is_error: false,
        content: "写入成功".to_string(),
        msg_id: "msg-789".to_string(),
        duration_ms: Some(42),
    });

    assert_eq!(legacy.name, "tool:completed");
    assert_eq!(legacy.payload["id"], "msg-789");
    assert_eq!(legacy.payload["role"], "tool");
    assert_eq!(legacy.payload["content"], json!({}));
    assert_eq!(legacy.payload["toolResult"]["toolCallId"], "tc-001");
    assert_eq!(legacy.payload["toolResult"]["name"], "file_write");
    assert_eq!(legacy.payload["toolResult"]["isError"], false);
    assert_eq!(legacy.payload["toolResult"]["content"], "写入成功");
    assert_eq!(legacy.payload["toolResult"]["durationMs"], 42);
    assert_eq!(legacy.payload["success"], true);
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn tool_call_completed_failure_reverses_is_error_and_success_fields() {
    let legacy = mapped(RuntimeEventKind::ToolCallCompleted {
        tool_call_id: ToolCallId::new("tc-002"),
        tool_name: "Bash".to_string(),
        is_error: true,
        content: "语法错误：第 3 行".to_string(),
        msg_id: "msg-999".to_string(),
        duration_ms: None,
    });

    assert_eq!(legacy.name, "tool:completed");
    assert_eq!(legacy.payload["toolResult"]["isError"], true);
    assert_eq!(legacy.payload["success"], false);
    assert_eq!(legacy.payload["toolResult"]["content"], "语法错误：第 3 行");
    assert!(legacy.payload["toolResult"]["durationMs"].is_null());
}

#[test]
fn permission_ask_required_maps_to_permission_ask_with_full_confirmation_payload() {
    let legacy = mapped(RuntimeEventKind::PermissionAskRequired {
        tool_call_id: ToolCallId::new("tc-003"),
        tool_name: "browse".to_string(),
        message: "是否允许浏览网页？".to_string(),
        suggestions: vec!["允许一次".to_string(), "总是允许".to_string()],
        mode: PermissionMode::Default,
        remember_options: vec![
            PermissionDestination::Session,
            PermissionDestination::Workspace,
        ],
        default_destination: Some(PermissionDestination::Session),
        primary_model: "deepseek-v3".into(),
    });

    assert_eq!(legacy.name, "permission:ask");
    assert_eq!(legacy.payload["toolName"], "browse");
    assert_eq!(legacy.payload["toolCallId"], "tc-003");
    assert_eq!(legacy.payload["message"], "是否允许浏览网页？");
    assert_eq!(legacy.payload["suggestions"].as_array().unwrap().len(), 2);
    assert_eq!(legacy.payload["suggestions"][0], "允许一次");
    assert_eq!(legacy.payload["suggestions"][1], "总是允许");
    assert_eq!(
        legacy.payload["rememberOptions"],
        json!(["session", "workspace"])
    );
    assert_eq!(legacy.payload["defaultDestination"], "session");
    assert_eq!(legacy.payload["mode"], "default");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn permission_ask_required_dont_ask_mode_maps_mode_as_dont_ask() {
    let legacy = mapped(RuntimeEventKind::PermissionAskRequired {
        tool_call_id: ToolCallId::new("tc-003"),
        tool_name: "browse".to_string(),
        message: "是否允许浏览网页？".to_string(),
        suggestions: vec!["允许一次".to_string(), "总是允许".to_string()],
        mode: PermissionMode::DontAsk,
        remember_options: vec![
            PermissionDestination::Session,
            PermissionDestination::Workspace,
        ],
        default_destination: Some(PermissionDestination::Session),
        primary_model: "deepseek-v3".into(),
    });

    assert_eq!(legacy.name, "permission:ask");
    assert_eq!(legacy.payload["mode"], "dontAsk");
}

#[test]
fn agent_idle_primary_scope_maps_to_agent_idle_with_primary_scope() {
    let legacy = mapped(RuntimeEventKind::AgentIdle {
        agent_id: AgentId::new("agent-run-001"),
        scope: AgentIdleScope::Primary,
    });

    assert_eq!(legacy.name, "agent:idle");
    assert_eq!(legacy.payload["scope"], "primary");
    assert_eq!(legacy.payload["agentId"], "agent-run-001");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn agent_idle_child_scope_maps_to_agent_idle_with_child_scope() {
    let legacy = mapped(RuntimeEventKind::AgentIdle {
        agent_id: AgentId::new("agent-child-002"),
        scope: AgentIdleScope::Child,
    });

    assert_eq!(legacy.name, "agent:idle");
    assert_eq!(legacy.payload["scope"], "child");
    assert_eq!(legacy.payload["agentId"], "agent-child-002");
}

#[test]
fn message_persisted_maps_to_message_updated_with_ids_role_run_id_and_created_at() {
    let legacy = mapped(RuntimeEventKind::MessagePersisted {
        message_id: "msg-001".to_string(),
        role: "assistant".to_string(),
        content: json!({ "text": "你好" }),
        client_message_id: None,
        tool_calls: None,
        error: None,
    });

    assert_eq!(legacy.name, "message:updated");
    assert_eq!(legacy.payload["messageId"], "msg-001");
    assert_eq!(legacy.payload["id"], "msg-001");
    assert_eq!(legacy.payload["role"], "assistant");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
    assert!(legacy.payload.get("createdAt").is_some());
}

#[test]
fn internal_runtime_events_do_not_map_to_legacy_frontend_events() {
    let unmapped = vec![
        RuntimeEventKind::RunStarted,
        RuntimeEventKind::RunCancelled,
        RuntimeEventKind::StreamStarted,
        RuntimeEventKind::OrphanedPermissionDetected { count: 1 },
    ];

    for kind in unmapped {
        assert!(map_runtime_event(&event(kind)).is_none());
    }
}

#[test]
fn turn_completed_maps_to_turn_completed_with_outcome_tokens_cost_and_denial_count() {
    let legacy = mapped(RuntimeEventKind::TurnCompleted {
        outcome: ChatTurnOutcome::Success,
        total_input_tokens: 100,
        total_output_tokens: 50,
        total_cache_creation_input_tokens: 0,
        total_cache_read_input_tokens: 0,
        total_cost_usd: Some(0.002),
        permission_denial_count: 3,
    });

    assert_eq!(legacy.name, "turn:completed");
    assert_eq!(legacy.payload["outcome"], "Success");
    assert_eq!(legacy.payload["totalInputTokens"], 100);
    assert_eq!(legacy.payload["totalOutputTokens"], 50);
    assert_eq!(legacy.payload["totalCostUsd"], 0.002);
    assert_eq!(legacy.payload["permissionDenialCount"], 3);
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn task_status_changed_maps_to_task_status_changed_with_task_status_and_context() {
    let legacy = mapped(RuntimeEventKind::TaskStatusChanged {
        task_id: TaskId::new("task-001"),
        status: "in_progress".to_string(),
        subject: "分析数据".to_string(),
        active_form: Some("正在分析数据".to_string()),
        owner_agent_id: Some(AgentId::new("agent-run-001")),
    });

    assert_eq!(legacy.name, "task:status-changed");
    assert_eq!(legacy.payload["taskId"], "task-001");
    assert_eq!(legacy.payload["status"], "in_progress");
    assert_eq!(legacy.payload["subject"], "分析数据");
    assert_eq!(legacy.payload["activeForm"], "正在分析数据");
    assert_eq!(legacy.payload["owner"], "agent-run-001");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
}

#[test]
fn turn_stage_changed_waiting_llm_maps_with_iteration_and_started_at() {
    let legacy = mapped(RuntimeEventKind::TurnStageChanged {
        stage: TurnStage::WaitingLlm { iteration: 3 },
        stage_started_at_ms: 1_700_000_000_000,
    });

    assert_eq!(legacy.name, "turn:stage");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
    assert_eq!(legacy.payload["stageStartedAtMs"], 1_700_000_000_000_u64);
    assert_eq!(legacy.payload["stage"]["kind"], "waitingLlm");
    assert_eq!(legacy.payload["stage"]["iteration"], 3);
}

#[test]
fn turn_stage_changed_tools_serializes_running_list_and_completed_count() {
    let legacy = mapped(RuntimeEventKind::TurnStageChanged {
        stage: TurnStage::Tools {
            iteration: 1,
            running: vec![
                RunningTool {
                    tool_name: "Bash".to_string(),
                    tool_call_id: "tc-1".to_string(),
                    started_at_ms: 1_700_000_001_000,
                },
                RunningTool {
                    tool_name: "Read".to_string(),
                    tool_call_id: "tc-2".to_string(),
                    started_at_ms: 1_700_000_001_500,
                },
            ],
            completed_in_batch: 1,
        },
        stage_started_at_ms: 1_700_000_000_000,
    });

    assert_eq!(legacy.name, "turn:stage");
    assert_eq!(legacy.payload["stage"]["kind"], "tools");
    assert_eq!(legacy.payload["stage"]["iteration"], 1);
    assert_eq!(legacy.payload["stage"]["completedInBatch"], 1);
    assert_eq!(legacy.payload["stage"]["running"][0]["toolName"], "Bash");
    assert_eq!(legacy.payload["stage"]["running"][0]["toolCallId"], "tc-1");
    assert_eq!(legacy.payload["stage"]["running"][1]["toolName"], "Read");
}

#[test]
fn turn_stage_changed_waiting_permission_uses_camelcase_fields() {
    let legacy = mapped(RuntimeEventKind::TurnStageChanged {
        stage: TurnStage::WaitingPermission {
            tool_name: "Write".to_string(),
            tool_call_id: "tc-9".to_string(),
        },
        stage_started_at_ms: 1_700_000_000_000,
    });

    assert_eq!(legacy.payload["stage"]["kind"], "waitingPermission");
    assert_eq!(legacy.payload["stage"]["toolName"], "Write");
    assert_eq!(legacy.payload["stage"]["toolCallId"], "tc-9");
}

#[test]
fn turn_heartbeat_maps_with_elapsed_ms_fields() {
    let legacy = mapped(RuntimeEventKind::TurnHeartbeat {
        stage_elapsed_ms: 2400,
        turn_elapsed_ms: 18_500,
    });

    assert_eq!(legacy.name, "turn:heartbeat");
    assert_eq!(legacy.payload["conversationId"], "conv-123");
    assert_eq!(legacy.payload["runId"], "run-456");
    assert_eq!(legacy.payload["stageElapsedMs"], 2400);
    assert_eq!(legacy.payload["turnElapsedMs"], 18_500);
}

#[test]
fn message_persisted_with_error_forwards_error_field() {
    use app_lib::runtime::events::RuntimeEvent;
    use app_lib::storage::file_store::types::{ErrorKind, MessageError};
    use app_lib::transport::tauri_event_adapter::map_runtime_event;

    let event = RuntimeEvent::message_persisted_with_error(
        "test-session".into(),
        "test-run".into(),
        "msg-1",
        "assistant",
        serde_json::json!({"text": "占位"}),
        MessageError {
            kind: ErrorKind::ChunkTimeout,
            message: "AI 服务暂时无法响应".to_string(),
            raw: None,
        },
    );

    let legacy = map_runtime_event(&event).expect("should produce legacy event");
    assert_eq!(legacy.name, "message:updated");

    let error = legacy.payload.get("error").expect("error field must be forwarded to frontend");
    assert_eq!(error.get("kind").and_then(|v| v.as_str()), Some("chunk_timeout"));
    assert_eq!(error.get("message").and_then(|v| v.as_str()), Some("AI 服务暂时无法响应"));
}

#[test]
fn message_persisted_without_error_omits_error_field() {
    use app_lib::runtime::events::RuntimeEvent;
    use app_lib::transport::tauri_event_adapter::map_runtime_event;

    let event = RuntimeEvent::message_persisted(
        "test-session".into(),
        "test-run".into(),
        "msg-1",
        "assistant",
        serde_json::json!({"text": "normal"}),
    );

    let legacy = map_runtime_event(&event).expect("should produce legacy event");
    assert!(legacy.payload.get("error").is_none(), "正常 MessagePersisted 不应携带 error 字段");
}
