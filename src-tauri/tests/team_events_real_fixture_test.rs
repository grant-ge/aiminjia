//! 集成测试：用真实对话 fixture `840ba884` 跑 team_events 解析器，
//! 验证端到端事件流的正确性。
//!
//! 该 fixture 是从 `~/.renlijia/users/t_3__u_23/conversations/840ba884-.../`
//! 拷贝的真实辩论赛对话（已删去 teammate jsonl 仅保留 meta.json）。
//!
//! 这层测试确保解析器对真实生产数据 schema 的鲁棒性——单测里的合成数据
//! 可能跟实际字段顺序/嵌套形态不完全一致。

use app_lib::runtime::team_events;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("team_view_840ba884")
}

#[test]
fn parses_real_debate_conversation() {
    let dir = fixture_dir();
    let view = team_events::parse_team_view(&dir).expect("parse should succeed");

    // ─ Roster 检查 ─
    assert_eq!(view.roster.team_name.as_deref(), Some("debate-team"));
    assert!(view.roster.description.is_some());
    assert_eq!(view.roster.members.len(), 4, "应有 4 个 teammate");

    let member_names: Vec<&str> = view.roster.members.iter().map(|m| m.name.as_str()).collect();
    for expected in &["affirmative", "negative", "judge", "moderator"] {
        assert!(
            member_names.contains(expected),
            "缺少 teammate {}: 实际 {:?}",
            expected,
            member_names
        );
    }

    // ─ 事件类型分布 ─
    let mut team_created = 0;
    let mut member_joined = 0;
    let mut send_lead_to_member = 0;
    let mut send_member_to_lead = 0;
    let mut other = 0;
    for ev in &view.events {
        match ev {
            team_events::TeamEvent::TeamCreated { .. } => team_created += 1,
            team_events::TeamEvent::MemberJoined { .. } => member_joined += 1,
            team_events::TeamEvent::MessageSent { sender, to, .. } => {
                if sender == "team-lead" {
                    send_lead_to_member += 1;
                } else if to == "team-lead" {
                    send_member_to_lead += 1;
                } else {
                    other += 1;
                }
            }
            _ => other += 1,
        }
    }

    assert_eq!(team_created, 1, "应恰有 1 条 TeamCreated");
    assert_eq!(member_joined, 4, "应恰有 4 条 MemberJoined");
    assert!(send_lead_to_member > 0, "应至少有 1 条 lead → member SendMessage");
    assert!(send_member_to_lead > 0, "应至少有 1 条 member → lead peer-message");

    // ─ 时间顺序 ─
    let timestamps: Vec<_> = view.events.iter().map(|e| e.ts()).collect();
    for w in timestamps.windows(2) {
        assert!(w[0] <= w[1], "事件未按 ts 升序：{:?} vs {:?}", w[0], w[1]);
    }

    // ─ TeamCreated 应是最早的事件 ─
    if let team_events::TeamEvent::TeamCreated { team_name, .. } = &view.events[0] {
        assert_eq!(team_name, "debate-team");
    } else {
        panic!("第一条事件应是 TeamCreated，实际：{:?}", view.events[0]);
    }

    // ─ 输出诊断信息（手动 cargo test -- --nocapture 时看） ─
    eprintln!("============ 解析结果摘要 ============");
    eprintln!("team_name:      {:?}", view.roster.team_name);
    eprintln!("description:    {:?}", view.roster.description);
    eprintln!("created_at:     {:?}", view.roster.created_at);
    eprintln!("members ({}):", view.roster.members.len());
    for m in &view.roster.members {
        eprintln!("  - {}  agent_id={}  spawned_at={:?}", m.name, m.agent_id, m.spawned_at);
    }
    eprintln!("task progress:  {}/{}", view.roster.task_count_completed, view.roster.task_count_total);
    eprintln!("event totals:");
    eprintln!("  TeamCreated:           {}", team_created);
    eprintln!("  MemberJoined:          {}", member_joined);
    eprintln!("  SendMessage lead→mem:  {}", send_lead_to_member);
    eprintln!("  SendMessage mem→lead:  {}", send_member_to_lead);
    eprintln!("  other:                 {}", other);
    eprintln!("total events:            {}", view.events.len());
    eprintln!();
    eprintln!("=== 前 10 条事件按时序 ===");
    for (i, ev) in view.events.iter().take(10).enumerate() {
        let summary = match ev {
            team_events::TeamEvent::TeamCreated { team_name, .. } => {
                format!("TeamCreated team={}", team_name)
            }
            team_events::TeamEvent::MemberJoined { name, agent_id, .. } => {
                format!("MemberJoined {} ({})", name, &agent_id[..16.min(agent_id.len())])
            }
            team_events::TeamEvent::TaskCreated { subject, .. } => {
                format!("TaskCreated {}", &subject[..40.min(subject.len())])
            }
            team_events::TeamEvent::TaskUpdated { task_id, status, owner, .. } => {
                format!("TaskUpdated #{} status={:?} owner={:?}", task_id, status, owner)
            }
            team_events::TeamEvent::MessageSent { sender, to, content, .. } => {
                let preview = content.chars().take(40).collect::<String>();
                format!("MessageSent {} → {}: {}", sender, to, preview)
            }
        };
        eprintln!("  #{:02} [{}] {}", i, ev.ts().format("%H:%M:%S"), summary);
    }
}
