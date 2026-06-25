use app_lib::runtime::chat::tool_result_artifact::{
    CompactionEvidenceConfig, DEFAULT_PREVIEW_CHARS, apply_tool_result_artifact_replacements,
    build_compaction_evidence_messages, build_persisted_tool_result_message,
    build_tool_result_artifact_replacements_from_round_results, persist_tool_result_artifact,
    tool_results_dir, tool_results_manifest_path,
};
use app_lib::runtime::chat::tool_result_collector::collect_results;
use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

#[test]
fn persists_tool_result_and_manifest_record() {
    let tmp = tempfile::tempdir().unwrap();
    let record = persist_tool_result_artifact(
        tmp.path(),
        "call_1",
        "Bash",
        "important fact: BUILD_ID=abc123",
        "text/plain",
    )
    .unwrap();

    assert_eq!(record.schema_version, 1);
    assert_eq!(record.tool_call_id, "call_1");
    assert!(record.path_buf().exists());
    assert_eq!(
        std::fs::read_to_string(record.path_buf()).unwrap(),
        "important fact: BUILD_ID=abc123"
    );

    let manifest = std::fs::read_to_string(tool_results_manifest_path(tmp.path())).unwrap();
    assert!(manifest.contains("\"toolCallId\":\"call_1\""));
    assert!(manifest.contains("\"toolName\":\"Bash\""));
}

#[test]
fn persisted_message_contains_recovery_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let record = persist_tool_result_artifact(
        tmp.path(),
        "tc_<danger>",
        "tool \"quoted\"",
        &"x".repeat(DEFAULT_PREVIEW_CHARS + 20),
        "text/plain",
    )
    .unwrap();

    let message = build_persisted_tool_result_message(&record);
    assert!(message.starts_with("<persisted-tool-result "));
    assert!(message.contains("tool_call_id=\"tc_&lt;danger&gt;\""));
    assert!(message.contains("tool_name=\"tool &quot;quoted&quot;\""));
    assert!(message.contains("Full output saved to:"));
    assert!(message.contains("Sha256:"));
    assert!(message.contains("named deliverable"));
    assert!(message.contains("instead of continuing broad exploratory reads"));
    assert!(message.contains("</persisted-tool-result>"));
    assert_eq!(record.preview.chars().count(), DEFAULT_PREVIEW_CHARS);
}

#[test]
fn duplicate_persist_reuses_manifest_record() {
    let tmp = tempfile::tempdir().unwrap();
    let first = persist_tool_result_artifact(
        tmp.path(),
        "call_same",
        "Bash",
        "first content",
        "text/plain",
    )
    .unwrap();
    let second = persist_tool_result_artifact(
        tmp.path(),
        "call_same",
        "Bash",
        "second content",
        "text/plain",
    )
    .unwrap();

    assert_eq!(first, second);
    assert_eq!(
        std::fs::read_to_string(first.path_buf()).unwrap(),
        "first content"
    );

    let manifest = std::fs::read_to_string(tool_results_manifest_path(tmp.path())).unwrap();
    assert_eq!(manifest.lines().count(), 1);
}

#[test]
fn unsafe_tool_call_id_cannot_escape_artifact_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let record = persist_tool_result_artifact(
        tmp.path(),
        "..\\..\\secret/file",
        "Bash",
        "safe",
        "text/plain",
    )
    .unwrap();

    let artifact_dir = tool_results_dir(tmp.path()).canonicalize().unwrap();
    let artifact_path = record.path_buf().canonicalize().unwrap();
    assert!(artifact_path.starts_with(artifact_dir));
    assert!(
        !artifact_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("..")
    );
}

#[test]
fn artifact_projection_replaces_collector_truncation_with_recoverable_reference() {
    let tmp = tempfile::tempdir().unwrap();
    let important_tail = "FINAL_DECISION=approve-release-42";
    let raw_content = format!("{}{}", "x".repeat(4_096), important_tail);
    let round_results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc_artifact".to_string(),
        tool_name: "Bash".to_string(),
        content: raw_content.clone(),
        is_error: false,
        msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 256,
        context_modifier_message: None,
    })];

    let replacements =
        build_tool_result_artifact_replacements_from_round_results(tmp.path(), &round_results);
    let mut collected = collect_results(round_results).tool_result_messages;
    assert!(
        collected[0]["content"]
            .as_str()
            .unwrap()
            .contains("[Output truncated:")
    );

    apply_tool_result_artifact_replacements(&mut collected, &replacements);

    let content = collected[0]["content"].as_str().unwrap();
    assert!(content.starts_with("<persisted-tool-result "));
    assert!(content.contains("Full output saved to:"));
    assert!(content.contains("Original chars:"));
    assert!(!content.contains("[Output truncated:"));

    let manifest = std::fs::read_to_string(tool_results_manifest_path(tmp.path())).unwrap();
    let record: app_lib::runtime::chat::tool_result_artifact::ToolResultArtifactRef =
        serde_json::from_str(manifest.lines().next().unwrap()).unwrap();
    assert_eq!(
        std::fs::read_to_string(record.path_buf()).unwrap(),
        raw_content
    );
    assert!(
        std::fs::read_to_string(record.path_buf())
            .unwrap()
            .contains(important_tail)
    );
}

#[test]
fn compaction_evidence_expands_persisted_tool_result_beyond_preview() {
    let tmp = tempfile::tempdir().unwrap();
    let tail_fact = "TOOL-ARTIFACT-TAIL-DECISION=keep-remote-logs";
    let raw_content = format!("{}{}", "x".repeat(DEFAULT_PREVIEW_CHARS + 512), tail_fact);
    let record = persist_tool_result_artifact(
        tmp.path(),
        "tc_evidence",
        "Bash",
        &raw_content,
        "text/plain",
    )
    .unwrap();
    let persisted_ref = build_persisted_tool_result_message(&record);
    assert!(!persisted_ref.contains(tail_fact));

    let messages = vec![serde_json::json!({
        "role": "tool",
        "toolCallId": "tc_evidence",
        "name": "Bash",
        "content": persisted_ref,
    })];
    let expanded = build_compaction_evidence_messages(
        &messages,
        &CompactionEvidenceConfig {
            max_chars_per_artifact: raw_content.chars().count(),
            aggregate_char_budget: raw_content.chars().count(),
        },
    );

    let expanded_content = expanded[0]["content"].as_str().unwrap();
    assert!(expanded_content.starts_with("<persisted-tool-result-evidence>"));
    assert!(expanded_content.contains("Recovered artifact content for compaction:"));
    assert!(expanded_content.contains(tail_fact));
}
