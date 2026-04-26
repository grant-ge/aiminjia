use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_lib::runtime::agent::subagent_result_envelope::{
    build_subagent_transcript_ref, SubAgentResultEnvelope, SubAgentTerminalToolResult,
    SubAgentTranscriptEntry,
};
use app_lib::runtime::agent::subagent_transcript_store::SubagentTranscriptEntryRecord;
use app_lib::runtime::agent::AgentRuntime;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::tools::capability::{CapabilityContext, FileState, FileStateCache};
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::Value;

#[test]
fn parent_cancellation_cascades_to_child_token() {
    let parent = CancellationToken::new();
    let child = parent.child_token();

    assert!(!child.is_cancelled());
    parent.cancel();
    assert!(child.is_cancelled());
}

#[test]
fn child_file_state_cache_inherits_parent_snapshot_without_polluting_parent() {
    let file_a = PathBuf::from("/tmp/file_a.csv");
    let file_b = PathBuf::from("/tmp/file_b.csv");
    let parent_cache = Arc::new(FileStateCache::new());
    parent_cache.set(
        file_a.clone(),
        FileState {
            content: "parent file".to_string(),
            mtime_secs: 1,
            offset: None,
            limit: None,
        },
    );

    let child_cache = parent_cache.clone_for_child();
    child_cache.set(
        file_b.clone(),
        FileState {
            content: "child file".to_string(),
            mtime_secs: 2,
            offset: None,
            limit: None,
        },
    );

    assert!(parent_cache.get(&file_b).is_none());
    assert_eq!(child_cache.get(&file_a).unwrap().content, "parent file");
    assert!(!Arc::ptr_eq(&parent_cache, &child_cache));
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SeenSubagentContext {
    agent_id: Option<String>,
    is_subagent: bool,
}

struct CaptureSubagentContextTool {
    seen: Arc<Mutex<Vec<SeenSubagentContext>>>,
}

#[async_trait]
impl RuntimeTool for CaptureSubagentContextTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("capture_subagent_context", "capture subagent context")
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        self.seen.lock().unwrap().push(SeenSubagentContext {
            agent_id: ctx.agent_id.as_ref().map(|id| id.as_str().to_string()),
            is_subagent: ctx
                .capability
                .as_ref()
                .map(|cap| cap.is_subagent)
                .unwrap_or(false),
        });
        Ok(ToolResult::new("capture_subagent_context", "ok", None))
    }
}

#[tokio::test]
async fn subagent_tool_context_marks_is_subagent_and_child_agent_id() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let tool = CaptureSubagentContextTool { seen: seen.clone() };
    let child_agent_id = app_lib::runtime::ids::AgentId::new("child-agent-123");
    let cap =
        CapabilityContext::with_workspace(PathBuf::from("/tmp"), "ws-child").with_subagent(true);
    let mut ctx =
        ToolExecutionContext::for_test("conv", "run", "tc").with_capability(Arc::new(cap));
    ctx.agent_id = Some(child_agent_id.clone());

    tool.execute(serde_json::json!({}), ctx).await.unwrap();

    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[SeenSubagentContext {
            agent_id: Some(child_agent_id.as_str().to_string()),
            is_subagent: true,
        }]
    );
}

#[test]
fn worker_runtime_source_guards_iteration_limit_cancel_and_ask_bubbling_messages() {
    let source = std::fs::read_to_string("src/runtime/agent/worker_runtime.rs")
        .expect("read worker runtime source");
    assert!(source.contains("Sub-agent reached iteration limit."));
    assert!(source.contains("Sub-agent cancelled."));
    assert!(source.contains("return Err(LegacyToolError::AskRequired(decision));"));
    assert!(source.contains("annotate_subagent_ask_decision"));
    assert!(source.contains("Permission Ask required"));
}

#[test]
fn result_envelope_contains_output_iterations_files_and_transcript_snapshot() {
    let mut generated_files = vec![
        "reports/result.md".to_string(),
        "reports/result.md".to_string(),
    ];
    generated_files.sort();
    generated_files.dedup();
    let envelope = SubAgentResultEnvelope {
        schema_version: 1,
        output: "分析完成".to_string(),
        iterations_used: 2,
        generated_files,
        terminal_tool_results: vec![SubAgentTerminalToolResult {
            tool_call_id: "tc-1".to_string(),
            tool_name: "dummy_tool".to_string(),
            success: true,
            summary: "ok".to_string(),
            generated_files: vec!["reports/result.md".to_string()],
        }],
        transcript_snapshot: vec![SubAgentTranscriptEntry {
            role: "assistant".to_string(),
            content: "分析完成".to_string(),
            tool_call_id: None,
            tool_name: None,
        }],
        transcript_ref: Some(build_subagent_transcript_ref("child-run-1")),
    };

    assert_eq!(envelope.schema_version, 1);
    assert_eq!(envelope.output, "分析完成");
    assert_eq!(envelope.iterations_used, 2);
    assert_eq!(envelope.generated_files, vec!["reports/result.md"]);
    assert!(envelope.transcript_snapshot.len() <= 16);
    assert!(envelope
        .transcript_ref
        .as_deref()
        .unwrap()
        .starts_with("subagent://"));
}

#[test]
fn envelope_storage_summary_roundtrips_core_fields() {
    let envelope = SubAgentResultEnvelope {
        schema_version: 1,
        output: "test output".to_string(),
        iterations_used: 3,
        generated_files: Vec::new(),
        terminal_tool_results: Vec::new(),
        transcript_snapshot: Vec::new(),
        transcript_ref: None,
    };

    let summary = envelope.to_storage_summary();
    assert!(summary.starts_with("subagent-envelope:v1:"));
    let decoded = SubAgentResultEnvelope::from_storage_summary(&summary).unwrap();
    assert_eq!(decoded.output, "test output");
    assert_eq!(decoded.schema_version, 1);
    assert_eq!(decoded.iterations_used, 3);
}

#[test]
fn stored_full_transcript_entry_count_matches_message_rounds() {
    let runtime = AgentRuntime::for_test();
    let entries = vec![
        SubagentTranscriptEntryRecord {
            role: "user".into(),
            content: "task".into(),
            tool_call_id: None,
            tool_name: None,
        },
        SubagentTranscriptEntryRecord {
            role: "assistant".into(),
            content: "calling tool".into(),
            tool_call_id: None,
            tool_name: None,
        },
        SubagentTranscriptEntryRecord {
            role: "tool".into(),
            content: "tool result".into(),
            tool_call_id: Some("tc-1".into()),
            tool_name: Some("dummy_tool".into()),
        },
        SubagentTranscriptEntryRecord {
            role: "assistant".into(),
            content: "done".into(),
            tool_call_id: None,
            tool_name: None,
        },
    ];

    runtime
        .store_transcript("subagent://child-transcript", &entries)
        .unwrap();
    let loaded = runtime
        .transcript_store_get("subagent://child-transcript")
        .unwrap()
        .unwrap();

    assert_eq!(loaded.len(), 4);
    assert_eq!(loaded, entries);
}
