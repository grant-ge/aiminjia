use app_lib::runtime::agent::subagent_transcript_store::{
    FileSubagentTranscriptStore, InMemorySubagentTranscriptStore, SubagentTranscriptEntryRecord,
    SubagentTranscriptStore,
};
use tempfile::TempDir;

#[test]
fn in_memory_transcript_store_roundtrips_entries_by_ref() {
    let store = InMemorySubagentTranscriptStore::new();
    let transcript_ref = "subagent://run-child-1";
    let entries = vec![SubagentTranscriptEntryRecord {
        role: "assistant".to_string(),
        content: "done".to_string(),
        tool_call_id: None,
        tool_name: None,
    }];

    store.put(transcript_ref, &entries).unwrap();
    let loaded = store.get(transcript_ref).unwrap().unwrap();
    assert_eq!(loaded, entries);
}

#[test]
fn file_backed_transcript_store_roundtrips_entries_by_ref() {
    let temp = TempDir::new().unwrap();
    let store = FileSubagentTranscriptStore::new(temp.path().to_path_buf()).unwrap();
    let transcript_ref = "subagent://run-child-2";

    store
        .put(
            transcript_ref,
            &[SubagentTranscriptEntryRecord {
                role: "tool".to_string(),
                content: "saved /tmp/a.json".to_string(),
                tool_call_id: Some("call-1".to_string()),
                tool_name: Some("Bash".to_string()),
            }],
        )
        .unwrap();

    let loaded = store.get(transcript_ref).unwrap().unwrap();
    assert_eq!(loaded[0].tool_name.as_deref(), Some("Bash"));
}
