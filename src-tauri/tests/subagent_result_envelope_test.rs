use app_lib::runtime::agent::message_bridge;
use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::agent::subagent_result_envelope::{
    SubAgentResultEnvelope, SubAgentTerminalToolResult, SubAgentTranscriptEntry,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::ids::{RunId, SessionId};

#[test]
fn envelope_roundtrip_keeps_core_sidechain_fields() {
    let envelope = SubAgentResultEnvelope {
        schema_version: 1,
        output: "done".to_string(),
        iterations_used: 2,
        generated_files: vec!["/tmp/a.json".to_string(), "/tmp/b.json".to_string()],
        terminal_tool_results: vec![SubAgentTerminalToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "extract_table_data".to_string(),
            success: true,
            summary: "saved 128 rows".to_string(),
            generated_files: vec!["/tmp/a.json".to_string()],
        }],
        transcript_snapshot: vec![
            SubAgentTranscriptEntry {
                role: "user".to_string(),
                content: "提取销售报表".to_string(),
                tool_call_id: None,
                tool_name: None,
            },
            SubAgentTranscriptEntry {
                role: "tool".to_string(),
                content: "已导出 /tmp/a.json".to_string(),
                tool_call_id: Some("call-1".to_string()),
                tool_name: Some("extract_table_data".to_string()),
            },
        ],
        transcript_ref: Some("run-child-1".to_string()),
    };

    let summary = envelope.to_storage_summary();
    let decoded = SubAgentResultEnvelope::from_storage_summary(&summary)
        .expect("summary should decode to envelope");

    assert_eq!(decoded.generated_files, vec!["/tmp/a.json", "/tmp/b.json"]);
    assert_eq!(decoded.terminal_tool_results.len(), 1);
    assert_eq!(decoded.terminal_tool_results[0].tool_name, "extract_table_data");
    assert_eq!(decoded.transcript_snapshot.len(), 2);
    assert_eq!(decoded.transcript_ref.as_deref(), Some("run-child-1"));
}

#[test]
fn message_bridge_formats_envelope_summary_payload() {
    let envelope = SubAgentResultEnvelope {
        schema_version: 1,
        output: "分析完成".to_string(),
        iterations_used: 1,
        generated_files: vec!["/tmp/output.json".to_string()],
        terminal_tool_results: vec![SubAgentTerminalToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "browse_and_extract".to_string(),
            success: false,
            summary: "ACCESS DENIED".to_string(),
            generated_files: Vec::new(),
        }],
        transcript_snapshot: vec![SubAgentTranscriptEntry {
            role: "assistant".to_string(),
            content: "权限不足，已停止".to_string(),
            tool_call_id: None,
            tool_name: None,
        }],
        transcript_ref: Some("run-child-2".to_string()),
    };

    let summary = message_bridge::format_sub_agent_envelope_summary(&envelope);
    let decoded = SubAgentResultEnvelope::from_storage_summary(&summary)
        .expect("bridge summary should keep envelope format");

    assert_eq!(decoded.schema_version, 1);
    assert_eq!(decoded.generated_files, vec!["/tmp/output.json"]);
    assert_eq!(decoded.terminal_tool_results[0].summary, "ACCESS DENIED");
    assert_eq!(decoded.transcript_ref.as_deref(), Some("run-child-2"));
}

#[tokio::test]
async fn background_run_persists_decodable_envelope_summary() {
    let runtime = AgentRuntime::for_test();
    let bus = RuntimeEventBus::new();
    let session_id = SessionId::new("sess-h7-envelope");
    let parent_run_id = RunId::new("run-parent-h7-envelope");

    let mut req = SpawnChildRunRequest::for_test(parent_run_id.clone());
    req.background = true;
    let handle = runtime.spawn_child_run(req).await.unwrap();

    let envelope = SubAgentResultEnvelope {
        schema_version: 1,
        output: "child summarized result".to_string(),
        iterations_used: 4,
        generated_files: vec!["/tmp/h7.json".to_string()],
        terminal_tool_results: vec![SubAgentTerminalToolResult {
            tool_call_id: "call-h7".to_string(),
            tool_name: "extract_table_data".to_string(),
            success: true,
            summary: "saved 42 rows".to_string(),
            generated_files: vec!["/tmp/h7.json".to_string()],
        }],
        transcript_snapshot: vec![SubAgentTranscriptEntry {
            role: "assistant".to_string(),
            content: "child summarized result".to_string(),
            tool_call_id: None,
            tool_name: None,
        }],
        transcript_ref: Some(handle.child_run_id().as_str().to_string()),
    };

    let summary = message_bridge::format_sub_agent_envelope_summary(&envelope);
    runtime
        .complete_background_run(
            handle.child_run_id(),
            Some(&summary),
            session_id,
            parent_run_id,
            bus,
        )
        .await
        .unwrap();

    let stored = runtime.get_summary(handle.child_run_id()).await.unwrap();
    let decoded = SubAgentResultEnvelope::from_storage_summary(
        stored.as_deref().expect("summary must be stored"),
    )
    .expect("stored summary should remain decodable");

    assert_eq!(decoded.output, "child summarized result");
    assert_eq!(decoded.iterations_used, 4);
    assert_eq!(decoded.generated_files, vec!["/tmp/h7.json"]);
    assert_eq!(decoded.terminal_tool_results[0].summary, "saved 42 rows");
    assert_eq!(
        decoded.transcript_ref.as_deref(),
        Some(handle.child_run_id().as_str())
    );
}
