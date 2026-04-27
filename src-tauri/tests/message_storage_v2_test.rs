mod common;

use std::fs;

use app_lib::storage::file_store::messages::{
    get_messages_v2, insert_message_v2, migrate_shards_to_single_file,
};
use app_lib::storage::file_store::types::StoredMessage;
use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

fn setup_storage() -> (AppStorage, TempDir) {
    let dir = TempDir::new().expect("create temp dir");
    let storage = AppStorage::new(dir.path()).expect("create app storage");
    storage
        .create_conversation("c1", "Test")
        .expect("create conversation");
    (storage, dir)
}

fn setup_raw_base() -> (std::path::PathBuf, TempDir) {
    let dir = TempDir::new().expect("create temp dir");
    let base = dir.path().to_path_buf();
    app_lib::storage::file_store::conversations::create_conversation(&base, "c1", "Test")
        .expect("create conversation");
    (base, dir)
}

fn shard_path(
    base_dir: &std::path::Path,
    conversation_id: &str,
    shard_num: u64,
) -> std::path::PathBuf {
    base_dir
        .join("conversations")
        .join(conversation_id)
        .join(format!("messages.{}.jsonl", shard_num))
}

#[test]
fn new_fields_serialize_correctly() {
    let mut msg = common::make_tool_result("1", "tc_1", "execute_python", "done");
    msg.seq = Some(7);
    msg.rev = Some(3);
    msg.run_id = Some("run_1".into());
    msg.schema_version = Some(2);
    msg.sequence = Some(42);
    let json = serde_json::to_value(&msg).expect("serialize stored message");

    assert_eq!(json["toolCallId"], "tc_1");
    assert_eq!(json["name"], "execute_python");
    assert_eq!(json["runId"], "run_1");
    assert_eq!(json["sequence"], 42);
    assert_eq!(json["schemaVersion"], 2);
    assert!(
        json.get("toolCalls").is_none(),
        "toolCalls should be absent for tool result fixtures"
    );
}

#[test]
fn assistant_tool_calls_serialize_with_camel_case_field_names() {
    let msg = common::make_assistant_with_tc("2", "tc_2", "execute_python");
    let json = serde_json::to_value(&msg).expect("serialize assistant tool call message");

    assert_eq!(json["toolCalls"][0]["id"], "tc_2");
    assert!(
        json.get("toolCallId").is_none(),
        "assistant message should not emit toolCallId when absent"
    );
    assert!(
        json.get("runId").is_none(),
        "runId should be omitted when absent"
    );
    assert!(
        json.get("sequence").is_none(),
        "sequence should be omitted when absent"
    );
}

#[test]
fn legacy_seq_and_rev_still_serialize_during_shard_storage_transition() {
    let mut msg = common::make_user("3", "hello");
    msg.seq = Some(9);
    msg.rev = Some(2);

    let json = serde_json::to_value(&msg).expect("serialize transitional shard message");

    assert_eq!(json["seq"], 9);
    assert_eq!(json["_rev"], 2);
}

#[test]
fn old_v1_message_deserializes_without_new_fields() {
    let old = r#"{"id":"m1","conversationId":"c1","role":"user","content":{"text":"hi"},"createdAt":"2026-04-24T00:00:00Z"}"#;
    let msg: StoredMessage = serde_json::from_str(old).expect("deserialize v1 stored message");

    assert!(msg.tool_calls.is_none());
    assert!(msg.tool_call_id.is_none());
    assert!(msg.name.is_none());
    assert!(msg.run_id.is_none());
    assert!(msg.schema_version.is_none());
    assert!(msg.sequence.is_none());
    assert!(msg.seq.is_none());
    assert!(msg.rev.is_none());
    assert_eq!(msg.text(), "hi");
}

#[test]
fn legacy_shard_records_still_persist_seq_and_rev_for_dedup() {
    let (base, _dir) = setup_raw_base();

    app_lib::storage::file_store::messages::insert_message(
        &base,
        "m1",
        "c1",
        "user",
        r#"{"text":"original"}"#,
    )
    .expect("insert original");
    app_lib::storage::file_store::messages::update_message_content(
        &base,
        "m1",
        "c1",
        r#"{"text":"updated"}"#,
    )
    .expect("update original");

    let shard_raw = fs::read_to_string(shard_path(&base, "c1", 1)).expect("read shard");
    for line in shard_raw.lines() {
        let json_str = line
            .split_once('\t')
            .map(|(json, _)| json)
            .expect("strip jsonl completion marker");
        let json: serde_json::Value = serde_json::from_str(json_str).expect("parse shard line");
        assert!(
            json.get("seq").is_some(),
            "legacy shard record must still persist seq"
        );
        assert!(
            json.get("_rev").is_some(),
            "legacy shard record must still persist _rev"
        );
    }
}

#[test]
fn missing_seq_records_do_not_collapse_into_one_dedup_bucket() {
    let (storage, _dir) = setup_storage();
    let shard = shard_path(storage.base_dir(), "c1", 1);
    let first = serde_json::json!({
        "id": "m1",
        "conversationId": "c1",
        "role": "user",
        "content": { "text": "first" },
        "createdAt": "2026-04-24T00:00:01Z"
    });
    let second = serde_json::json!({
        "id": "m2",
        "conversationId": "c1",
        "role": "user",
        "content": { "text": "second" },
        "createdAt": "2026-04-24T00:00:02Z"
    });
    fs::write(
        &shard,
        format!(
            "{}\t✓\n{}\t✓\n",
            serde_json::to_string(&first).expect("serialize first"),
            serde_json::to_string(&second).expect("serialize second")
        ),
    )
    .expect("write shard");

    migrate_shards_to_single_file(storage.base_dir(), "c1").expect("migrate shard");

    let messages = storage.get_messages("c1").expect("read messages");
    assert_eq!(
        messages.len(),
        2,
        "records without seq must not dedup into one bucket"
    );
    assert_eq!(messages[0]["content"]["text"], "first");
    assert_eq!(messages[1]["content"]["text"], "second");
}

#[test]
fn top_level_tool_fields_survive_get_messages_read_path() {
    let (storage, _dir) = setup_storage();
    let shard = shard_path(storage.base_dir(), "c1", 1);
    let assistant = StoredMessage {
        id: "assistant-1".into(),
        conversation_id: "c1".into(),
        role: "assistant".into(),
        content: serde_json::json!({"text": ""}),
        created_at: "2026-04-24T00:00:01Z".into(),
        tool_calls: Some(vec![serde_json::json!({
            "id": "tc-top",
            "type": "function",
            "function": {"name": "execute_python", "arguments": "{}"}
        })]),
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: Some(2),
        sequence: Some(1),
        seq: Some(1),
        rev: Some(1),
    };
    let tool = StoredMessage {
        id: "tool-1".into(),
        conversation_id: "c1".into(),
        role: "tool".into(),
        content: serde_json::json!({"text": "tool result"}),
        created_at: "2026-04-24T00:00:02Z".into(),
        tool_calls: None,
        tool_call_id: Some("tc-top".into()),
        name: Some("execute_python".into()),
        run_id: None,
        schema_version: Some(2),
        sequence: Some(2),
        seq: Some(2),
        rev: Some(1),
    };
    fs::write(
        &shard,
        format!(
            "{}\t✓\n{}\t✓\n",
            serde_json::to_string(&assistant).expect("serialize assistant"),
            serde_json::to_string(&tool).expect("serialize tool")
        ),
    )
    .expect("write top-level shard");

    migrate_shards_to_single_file(storage.base_dir(), "c1").expect("migrate shard");

    let messages = storage.get_messages("c1").expect("read messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["toolCalls"][0]["id"], "tc-top");
    assert_eq!(messages[1]["toolResult"]["toolCallId"], "tc-top");
    assert_eq!(messages[1]["toolResult"]["name"], "execute_python");
    assert_eq!(messages[1]["toolResult"]["content"], "tool result");
}

#[test]
fn repeated_updates_on_missing_seq_records_still_keep_latest_content() {
    let (storage, _dir) = setup_storage();
    let conv_dir = storage.base_dir().join("conversations").join("c1");
    let shard = shard_path(storage.base_dir(), "c1", 1);
    let original = serde_json::json!({
        "id": "m1",
        "conversationId": "c1",
        "role": "user",
        "content": { "text": "original" },
        "createdAt": "2026-04-24T00:00:01Z"
    });
    fs::write(
        &shard,
        format!(
            "{}\t✓\n",
            serde_json::to_string(&original).expect("serialize original")
        ),
    )
    .expect("write shard");
    fs::write(conv_dir.join("_current"), "2:1").expect("force later updates into shard 2");

    migrate_shards_to_single_file(storage.base_dir(), "c1").expect("migrate shard");

    storage
        .update_message_content("m1", "c1", r#"{"text":"updated once"}"#)
        .expect("first update");
    storage
        .update_message_content("m1", "c1", r#"{"text":"updated twice"}"#)
        .expect("second update");

    let messages = storage.get_messages("c1").expect("read messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]["content"]["text"], "updated twice",
        "missing-seq update path must keep the latest revision even across shards"
    );
}

#[test]
fn insert_and_get_single_file() {
    let (storage, _dir) = setup_storage();

    let msg = common::make_user("1", "hello");
    insert_message_v2(storage.base_dir(), &msg).expect("insert single-file message");
    insert_message_v2(storage.base_dir(), &msg).expect("insert duplicate single-file message");

    let msgs = get_messages_v2(storage.base_dir(), "c1").expect("read single-file messages");
    assert_eq!(msgs.len(), 1, "same id should dedup in single-file storage");
    assert_eq!(msgs[0].text(), "hello");
}

#[test]
fn app_storage_insert_message_facade_writes_single_file_only() {
    let (storage, _dir) = setup_storage();

    storage
        .insert_message("m1", "c1", "user", r#"{"text":"hello"}"#)
        .expect("insert through legacy facade");

    let conv_dir = storage.base_dir().join("conversations").join("c1");
    assert!(conv_dir.join("messages.jsonl").exists());
    assert!(!conv_dir.join("messages.1.jsonl").exists());
    assert!(!conv_dir.join("_current").exists());

    let messages = storage.get_messages("c1").expect("read messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["content"]["text"], "hello");
}

#[test]
fn app_storage_get_messages_reads_single_file_transcript() {
    let (storage, _dir) = setup_storage();

    let mut first = common::make_user("1", "hello");
    first.sequence = Some(1);
    let mut second = common::make_assistant("2", "hi");
    second.sequence = Some(2);
    insert_message_v2(storage.base_dir(), &first).expect("insert first single-file message");
    insert_message_v2(storage.base_dir(), &second).expect("insert second single-file message");

    let messages = storage.get_messages("c1").expect("read frontend messages");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["content"]["text"], "hello");
    assert_eq!(messages[1]["content"]["text"], "hi");
}

#[test]
fn migrate_shards_merges_legacy_shards_into_existing_single_file() {
    let (storage, _dir) = setup_storage();
    let conv_dir = storage.base_dir().join("conversations").join("c1");

    let legacy_user = serde_json::json!({
        "seq": 1,
        "_rev": 1,
        "id": "legacy-user",
        "conversationId": "c1",
        "role": "user",
        "content": {"text": "你有哪些技能可以使用呢？"},
        "createdAt": "2026-04-26T06:59:18.370259+00:00"
    });
    fs::write(
        conv_dir.join("messages.1.jsonl"),
        format!("{}\t✓\n", serde_json::to_string(&legacy_user).unwrap()),
    )
    .expect("write legacy shard");
    fs::write(conv_dir.join("_current"), "1:2").expect("write shard cursor");

    let mut v2_tool = common::make_tool_result("tool-1", "tc-1", "switch_skill", "Switched");
    v2_tool.created_at = "2026-04-26T06:59:18.970640+00:00".into();
    v2_tool.sequence = Some(2);
    insert_message_v2(storage.base_dir(), &v2_tool).expect("insert v2 tool message");

    migrate_shards_to_single_file(storage.base_dir(), "c1").expect("merge mixed transcript");

    let messages = storage.get_messages("c1").expect("read merged messages");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["id"], "legacy-user");
    assert_eq!(messages[0]["content"]["text"], "你有哪些技能可以使用呢？");
    assert_eq!(messages[1]["id"], "tool-1");
    assert_eq!(messages[1]["toolResult"]["name"], "switch_skill");
}

#[test]
fn get_recent_messages_reads_single_file_after_shard_merge_migration() {
    let (storage, _dir) = setup_storage();
    let conv_dir = storage.base_dir().join("conversations").join("c1");

    let legacy_user = serde_json::json!({
        "seq": 1,
        "_rev": 1,
        "id": "legacy-user",
        "conversationId": "c1",
        "role": "user",
        "content": {"text": "old question"},
        "createdAt": "2026-04-26T06:59:18.000000+00:00"
    });
    fs::write(
        conv_dir.join("messages.1.jsonl"),
        format!("{}\t✓\n", serde_json::to_string(&legacy_user).unwrap()),
    )
    .expect("write legacy shard");
    fs::write(conv_dir.join("_current"), "1:2").expect("write shard cursor");

    let mut v2_tool = common::make_tool_result("tool-1", "tc-1", "switch_skill", "Switched");
    v2_tool.created_at = "2026-04-26T06:59:19.000000+00:00".into();
    insert_message_v2(storage.base_dir(), &v2_tool).expect("insert v2 tool message");

    migrate_shards_to_single_file(storage.base_dir(), "c1").expect("merge mixed transcript");

    let messages = storage
        .get_recent_messages("c1", 10)
        .expect("read recent messages");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["id"], "legacy-user");
    assert_eq!(messages[1]["id"], "tool-1");
}

#[test]
fn update_message_content_updates_migrated_legacy_record_in_single_file() {
    let (storage, _dir) = setup_storage();
    let conv_dir = storage.base_dir().join("conversations").join("c1");

    let legacy_user = serde_json::json!({
        "seq": 1,
        "_rev": 1,
        "id": "legacy-user",
        "conversationId": "c1",
        "role": "user",
        "content": {"text": "old text"},
        "createdAt": "2026-04-26T06:59:18.000000+00:00"
    });
    fs::write(
        conv_dir.join("messages.1.jsonl"),
        format!("{}\t✓\n", serde_json::to_string(&legacy_user).unwrap()),
    )
    .expect("write legacy shard");
    fs::write(conv_dir.join("_current"), "1:2").expect("write shard cursor");

    let mut v2_tool = common::make_tool_result("tool-1", "tc-1", "switch_skill", "Switched");
    v2_tool.created_at = "2026-04-26T06:59:19.000000+00:00".into();
    insert_message_v2(storage.base_dir(), &v2_tool).expect("insert v2 tool message");

    migrate_shards_to_single_file(storage.base_dir(), "c1").expect("merge mixed transcript");

    storage
        .update_message_content("legacy-user", "c1", r#"{"text":"updated text"}"#)
        .expect("update legacy message through mixed store");

    let messages = storage.get_messages("c1").expect("read merged messages");
    let updated = messages
        .iter()
        .find(|m| m["id"] == "legacy-user")
        .expect("legacy message present");
    assert_eq!(updated["content"]["text"], "updated text");
}

#[test]
fn update_via_same_id_last_wins() {
    let (storage, _dir) = setup_storage();

    let mut msg = common::make_user("1", "original");
    insert_message_v2(storage.base_dir(), &msg).expect("insert original");
    msg.content = serde_json::json!({"text": "updated"});
    insert_message_v2(storage.base_dir(), &msg).expect("insert updated");

    let msgs = get_messages_v2(storage.base_dir(), "c1").expect("read single-file messages");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].text(), "updated");
}

#[test]
fn messages_ordered_by_sequence_then_created_at() {
    let (storage, _dir) = setup_storage();

    let mut first = common::make_user("1", "first");
    first.created_at = "2026-04-24T00:00:00Z".into();
    first.sequence = Some(1);
    let mut second = common::make_user("2", "second");
    second.created_at = "2026-04-24T00:00:00Z".into();
    second.sequence = Some(2);

    insert_message_v2(storage.base_dir(), &second).expect("insert second");
    insert_message_v2(storage.base_dir(), &first).expect("insert first");

    let msgs = get_messages_v2(storage.base_dir(), "c1").expect("read ordered messages");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].text(), "first");
    assert_eq!(msgs[1].text(), "second");
}

#[test]
fn migrates_old_shards_to_single_file() {
    let (storage, _dir) = setup_storage();
    let conv_dir = storage.base_dir().join("conversations").join("c1");

    for (i, (id, text)) in [("m1", "hello"), ("m2", "world")].iter().enumerate() {
        let record = serde_json::json!({
            "seq": i + 1,
            "_rev": 1,
            "id": id,
            "conversationId": "c1",
            "role": "user",
            "content": {"text": text},
            "createdAt": format!("2026-04-24T00:00:0{}Z", i + 1)
        });
        fs::write(
            conv_dir.join(format!("messages.{}.jsonl", i + 1)),
            format!(
                "{}\t✓\n",
                serde_json::to_string(&record).expect("serialize shard record")
            ),
        )
        .expect("write legacy shard");
    }
    fs::write(conv_dir.join("_current"), "2:3").expect("write shard cursor");

    migrate_shards_to_single_file(storage.base_dir(), "c1").expect("migrate legacy shards");

    assert!(!conv_dir.join("messages.1.jsonl").exists());
    assert!(!conv_dir.join("messages.2.jsonl").exists());
    assert!(!conv_dir.join("_current").exists());
    assert!(conv_dir.join("messages.jsonl").exists());

    let msgs = get_messages_v2(storage.base_dir(), "c1").expect("read migrated messages");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].id, "m1");
    assert_eq!(msgs[1].id, "m2");
}

#[test]
fn app_storage_exposes_v2_message_api_after_migration() {
    let (storage, _dir) = setup_storage();
    let conv_dir = storage.base_dir().join("conversations").join("c1");
    let legacy = serde_json::json!({
        "seq": 1,
        "_rev": 1,
        "id": "m1",
        "conversationId": "c1",
        "role": "user",
        "content": {"text": "legacy"},
        "createdAt": "2026-04-24T00:00:01Z"
    });
    fs::write(
        conv_dir.join("messages.1.jsonl"),
        format!(
            "{}\t✓\n",
            serde_json::to_string(&legacy).expect("serialize legacy record")
        ),
    )
    .expect("write legacy shard");
    fs::write(conv_dir.join("_current"), "1:2").expect("write shard cursor");

    migrate_shards_to_single_file(storage.base_dir(), "c1").expect("migrate legacy shard");

    let msgs = storage
        .get_messages_v2("c1")
        .expect("read messages through storage v2 api");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].id, "m1");
    assert_eq!(msgs[0].text(), "legacy");
}
