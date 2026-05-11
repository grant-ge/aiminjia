//! P1.6 integration tests for transcript path routing.
//!
//! Verifies:
//! 1. AsyncOneShot spawn → transcript written to `subagents/agent-{id}.jsonl`
//!    + `.meta.json` contains `"kind": "subagent"`.
//! 2. TeammateIdle spawn → transcript written to `teammates/agent-{id}.jsonl`
//!    + `.meta.json` contains `"kind": "teammate"`, `"team_id"`, `"employee_id"`.
//! 3. Both paths coexist in the same conversation directory without collision.

use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

use app_lib::runtime::agent::inbox::AgentInbox;
use app_lib::runtime::agent::output_writer::{
    append_line, read_from, AgentTranscriptMeta, TranscriptKind, TranscriptLine,
    meta_path_for_kind, transcript_path_for_kind, write_meta,
};
use app_lib::runtime::agent::team::{Member, MemberRole, Team};
use app_lib::runtime::agent::worker_runtime::{run_worker, TeammateWorkerCtx, WorkerMode};
use app_lib::runtime::agent::AgentNameRegistry;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{AgentId, SessionId};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_team(session_id: &str, agent_id: AgentId, agent_name: &str) -> Arc<Mutex<Team>> {
    let lead = Member {
        agent_id: AgentId::new("lead-agent"),
        name: "lead".to_string(),
        role: MemberRole::Lead,
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    };
    let mut team = Team::new(SessionId::new(session_id), lead, "test-team".to_string());
    let teammate = Member {
        agent_id,
        name: agent_name.to_string(),
        role: MemberRole::Teammate {
            employee_id: "emp-routing-1".to_string(),
            spawned_by: AgentId::new("lead-agent"),
        },
        created_at: chrono::Utc::now(),
        last_active_at: chrono::Utc::now(),
    };
    team.add_teammate(teammate).unwrap();
    Arc::new(Mutex::new(team))
}

fn subagent_meta(conv_dir_path: &std::path::Path, agent_id: &str) -> AgentTranscriptMeta {
    AgentTranscriptMeta {
        agent_id: agent_id.to_string(),
        agent_name: None,
        kind: TranscriptKind::Subagent,
        employee_id: None,
        team_id: None,
        spawned_by: Some("parent-run".to_string()),
        spawned_at: chrono::Utc::now(),
        model: Some("sonnet".to_string()),
        is_async: true,
        boot_system_prompt: None,
            tool_whitelist: vec!["Read".to_string()],
    }
}

fn teammate_meta(conv_id: &str, agent_id: &str) -> AgentTranscriptMeta {
    AgentTranscriptMeta {
        agent_id: agent_id.to_string(),
        agent_name: Some("researcher".to_string()),
        kind: TranscriptKind::Teammate,
        employee_id: Some("emp-routing-1".to_string()),
        team_id: Some(conv_id.to_string()),
        spawned_by: Some("lead-agent".to_string()),
        spawned_at: chrono::Utc::now(),
        model: None,
        is_async: true,
        boot_system_prompt: None,
            tool_whitelist: vec!["Read".to_string(), "SendMessage".to_string()],
    }
}

// ─── Test 1: AsyncOneShot transcript uses subagents/ directory ────────────────

#[test]
fn async_oneshot_transcript_path_is_under_subagents() {
    let tmp = TempDir::new().unwrap();
    let conv_dir = tmp.path().join("conversations").join("conv-routing-sub");
    let agent_id = "agent-sub-route-1";

    // Write sidecar for subagent.
    let meta = subagent_meta(&conv_dir, agent_id);
    write_meta(&conv_dir, &meta).unwrap();

    // Verify sidecar is at subagents/ directory.
    let meta_path = meta_path_for_kind(&conv_dir, &TranscriptKind::Subagent, agent_id);
    assert!(
        meta_path.exists(),
        "subagent meta should exist at {meta_path:?}"
    );
    let meta_body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    assert_eq!(meta_body["kind"].as_str(), Some("subagent"));
    assert!(meta_body.get("team_id").is_none() || meta_body["team_id"].is_null());

    // Write a transcript line.
    let transcript = transcript_path_for_kind(&conv_dir, &TranscriptKind::Subagent, agent_id);
    append_line(&transcript, &TranscriptLine::assistant("subagent output")).unwrap();

    // Verify it's in subagents/ not teammates/.
    assert!(
        transcript.to_string_lossy().contains("subagents"),
        "transcript should be in subagents/, got: {:?}",
        transcript
    );
    assert!(
        !transcript.to_string_lossy().contains("teammates"),
        "transcript should NOT be in teammates/"
    );
}

// ─── Test 2: TeammateIdle transcript uses teammates/ directory ���───────────────

#[tokio::test]
async fn teammate_idle_transcript_path_is_under_teammates() {
    let tmp = TempDir::new().unwrap();
    let session_id = "conv-routing-tm";
    let conv_dir = tmp.path().join("conversations").join(session_id);
    let agent_id = AgentId::new("agent-tm-route-1");

    let team = make_team(session_id, agent_id.clone(), "researcher");
    let name_registry = AgentNameRegistry::new();
    let cancel = CancellationToken::new();
    let inbox = AgentInbox::new(4);
    let meta = teammate_meta(session_id, agent_id.as_str());

    let ctx = TeammateWorkerCtx {
        agent_id: agent_id.clone(),
        session_id: SessionId::new(session_id),
        conv_id: session_id.to_string(),
        cancel: cancel.clone(),
        inbox: inbox.clone(),
        agent_names: name_registry.clone(),
        inbox_registry: None,
        cancellation_registry: None,
        conv_dir: Some(conv_dir.clone()),
        meta,
    };

    // Spawn idle loop with an initial prompt so it writes a transcript line.
    let team_clone = team.clone();
    let handle = tokio::spawn(async move {
        run_worker(
            WorkerMode::TeammateIdle {
                team_handle: team_clone,
                agent_name: "researcher".to_string(),
            },
            ctx,
            Some("hello teammate".to_string()),
        )
        .await
    });

    // Give it time to process the initial prompt.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
        .await
        .expect("loop should exit");

    // Verify sidecar is in teammates/ directory.
    let meta_path =
        meta_path_for_kind(&conv_dir, &TranscriptKind::Teammate, agent_id.as_str());
    assert!(
        meta_path.exists(),
        "teammate meta should exist at {meta_path:?}"
    );
    let meta_body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    assert_eq!(meta_body["kind"].as_str(), Some("teammate"));
    assert_eq!(meta_body["team_id"].as_str(), Some(session_id));
    assert_eq!(meta_body["employee_id"].as_str(), Some("emp-routing-1"));

    // Verify transcript is in teammates/ directory.
    let transcript =
        transcript_path_for_kind(&conv_dir, &TranscriptKind::Teammate, agent_id.as_str());
    assert!(
        transcript.to_string_lossy().contains("teammates"),
        "transcript should be in teammates/, got: {:?}",
        transcript
    );
    assert!(
        !transcript.to_string_lossy().contains("subagents"),
        "transcript should NOT be in subagents/"
    );

    // Verify transcript has content.
    let (lines, total) = read_from(&transcript, 0).unwrap();
    assert!(
        total >= 2,
        "transcript should have at least 2 lines after initial prompt, got {}",
        total
    );
    assert!(
        lines[0].contains("hello teammate"),
        "first line should contain the initial prompt; got: {}",
        lines[0]
    );
}

// ─── Test 3: both paths coexist in same conversation without collision ─────────

#[tokio::test]
async fn subagent_and_teammate_coexist_in_same_conversation() {
    let tmp = TempDir::new().unwrap();
    let conv_id = "conv-routing-coexist";
    let conv_dir = tmp.path().join("conversations").join(conv_id);

    let subagent_id = "agent-sub-coexist";
    let teammate_id = AgentId::new("agent-tm-coexist");

    // ── Write subagent transcript ──────────────────────────────────────────
    let sub_meta = subagent_meta(&conv_dir, subagent_id);
    write_meta(&conv_dir, &sub_meta).unwrap();
    let sub_transcript =
        transcript_path_for_kind(&conv_dir, &TranscriptKind::Subagent, subagent_id);
    append_line(
        &sub_transcript,
        &TranscriptLine::assistant("subagent answer"),
    )
    .unwrap();

    // ── Run teammate idle loop ─────────────────────────────────────────────
    let team = make_team(conv_id, teammate_id.clone(), "researcher");
    let name_registry = AgentNameRegistry::new();
    let cancel = CancellationToken::new();
    let inbox = AgentInbox::new(4);
    let tm_meta = teammate_meta(conv_id, teammate_id.as_str());

    let ctx = TeammateWorkerCtx {
        agent_id: teammate_id.clone(),
        session_id: SessionId::new(conv_id),
        conv_id: conv_id.to_string(),
        cancel: cancel.clone(),
        inbox: inbox.clone(),
        agent_names: name_registry.clone(),
        inbox_registry: None,
        cancellation_registry: None,
        conv_dir: Some(conv_dir.clone()),
        meta: tm_meta,
    };

    let team_clone = team.clone();
    let handle = tokio::spawn(async move {
        run_worker(
            WorkerMode::TeammateIdle {
                team_handle: team_clone,
                agent_name: "researcher".to_string(),
            },
            ctx,
            Some("colleague task".to_string()),
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
        .await
        .expect("loop should exit");

    // ── Assertions ────────────────────────────────────────────────────────

    // Subagent transcript still intact.
    let (sub_lines, _) = read_from(&sub_transcript, 0).unwrap();
    assert!(!sub_lines.is_empty(), "subagent transcript should still exist");
    assert!(sub_lines[0].contains("subagent answer"));

    // Teammate transcript is separate.
    let tm_transcript =
        transcript_path_for_kind(&conv_dir, &TranscriptKind::Teammate, teammate_id.as_str());
    assert!(tm_transcript.exists(), "teammate transcript should exist");

    // Paths do not overlap.
    assert_ne!(
        sub_transcript.canonicalize().unwrap_or(sub_transcript.clone()),
        tm_transcript.canonicalize().unwrap_or(tm_transcript.clone()),
        "subagent and teammate transcripts must be different files"
    );

    // Subagent transcript does NOT appear in teammates/ dir.
    let teammates_dir = conv_dir.join("teammates");
    let sub_in_teammates = teammates_dir.join(format!("{subagent_id}.jsonl"));
    assert!(
        !sub_in_teammates.exists(),
        "subagent transcript must NOT be in teammates/"
    );

    // Teammate transcript does NOT appear in subagents/ dir.
    let subagents_dir = conv_dir.join("subagents");
    let tm_in_subagents = subagents_dir.join(format!("{}.jsonl", teammate_id.as_str()));
    assert!(
        !tm_in_subagents.exists(),
        "teammate transcript must NOT be in subagents/"
    );
}
