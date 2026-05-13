//! Tauri 命令：群聊视图查询。
//!
//! 数据源（v0.4）：
//! - `<conv_dir>/team.json` — Team 元数据（roster + TeamCreated/MemberJoined）
//! - `<conv_dir>/team-chat.jsonl` — 群聊消息一等公民存储；由
//!   `runtime::agent::team_chat::record_turn` 在每次 LLM turn 结束时追加
//!
//! 不再反扫 messages.jsonl / teammates/*.jsonl —— v0.3 反扫路径已下线。

use std::path::Path;
use std::sync::Arc;
use tauri::State;

use crate::runtime::agent::team_chat::{self, EntrySource};
use crate::runtime::agent::{MemberStatus, TeamSnapshot};
use crate::runtime::team_events::{TeamEvent, TeamRoster, TeamView, MemberInfo};
use crate::transport::tauri_commands::chat::TauriChatCommandAdapter;

/// 拉取一个 conversation 的最新群聊视图。
///
/// 没有 team.json 时返回 `events == []` + `roster.team_name == None`，
/// 前端按 "team_name 是否 None" 决定要不要展示 inline TeamCard / 抽屉。
#[tauri::command]
pub async fn team_view_for_conversation(
    adapter: State<'_, Arc<TauriChatCommandAdapter>>,
    conversation_id: String,
) -> Result<TeamView, String> {
    let storage = adapter.services_db_for_team_view();
    let conv_dir = storage.base_dir().join("conversations").join(&conversation_id);
    if !conv_dir.exists() {
        return Ok(empty_view());
    }
    Ok(load_view_from_team_json(&conv_dir))
}

/// 从 `<conv_dir>/team.json` 加载视图。文件不存在 / 损坏 → 空视图。
///
/// 没有走"v0.2 从 messages.jsonl 派生" fallback——v0.3 决策 #10 明确：
/// 老对话不渲染群即可，简化前端逻辑。
fn load_view_from_team_json(conv_dir: &Path) -> TeamView {
    let path = conv_dir.join("team.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return empty_view();
    };
    let snap: TeamSnapshot = match serde_json::from_slice(&bytes) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "team.json corrupt at {}: {} — treating as no team",
                path.display(),
                e
            );
            return empty_view();
        }
    };

    // Derive a TeamRoster from the snapshot. v0.3 keeps stopped/cancelled
    // members in the list (decision #4); frontend renders them greyed-out.
    let members: Vec<MemberInfo> = snap
        .teammates
        .iter()
        .map(|m| MemberInfo {
            agent_id: m.agent_id.as_str().to_string(),
            name: m.name.clone(),
            spawned_at: Some(m.created_at),
            employee_id: m.employee_id.clone(),
        })
        .collect();

    let task_count_total = 0;
    let task_count_completed = 0;

    let roster = TeamRoster {
        team_name: Some(snap.team_name.clone()),
        description: snap.description.clone(),
        created_at: Some(snap.created_at),
        members,
        task_count_total,
        task_count_completed,
    };

    // Derive events from snapshot — TeamCreated + MemberJoined per teammate.
    let mut events: Vec<TeamEvent> = Vec::new();
    events.push(TeamEvent::TeamCreated {
        ts: snap.created_at,
        team_name: snap.team_name.clone(),
        description: snap.description.clone(),
    });
    for m in &snap.teammates {
        events.push(TeamEvent::MemberJoined {
            ts: m.created_at,
            agent_id: m.agent_id.as_str().to_string(),
            name: m.name.clone(),
            subagent_type: None,
            description: None,
            employee_id: m.employee_id.clone(),
        });
        if matches!(m.status, MemberStatus::Stopped | MemberStatus::Cancelled) {
            // Re-use TaskUpdated as a generic status event for now.
            if let Some(ts) = m.stopped_at {
                let status_str = match m.status {
                    MemberStatus::Stopped => "stopped".to_string(),
                    MemberStatus::Cancelled => "cancelled".to_string(),
                    _ => "active".to_string(),
                };
                events.push(TeamEvent::TaskUpdated {
                    ts,
                    task_id: m.name.clone(),
                    owner: m.stopped_reason.clone(),
                    status: Some(status_str),
                });
            }
        }
    }

    // Append message timeline from team-chat.jsonl (v0.4 single source).
    let mut msg_events = read_team_chat_events(conv_dir);
    events.append(&mut msg_events);
    events.sort_by(|a, b| a.ts().cmp(&b.ts()));

    TeamView { events, roster }
}

/// Read `team-chat.jsonl` and map each entry to a `TeamEvent::MessageSent`.
/// Returns empty when the file is missing — legacy convs simply show no
/// timeline beyond TeamCreated/MemberJoined system pills.
fn read_team_chat_events(conv_dir: &Path) -> Vec<TeamEvent> {
    let entries = match team_chat::read_all(conv_dir) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "[team_view] read team-chat.jsonl failed at {}: {}",
                conv_dir.display(),
                e
            );
            return Vec::new();
        }
    };
    entries
        .into_iter()
        .map(|e| TeamEvent::MessageSent {
            ts: e.ts,
            sender: e.sender,
            to: e.to,
            content: e.content,
            anchor_message_id: match e.source {
                // Repurpose anchor_message_id as a source hint so the front-end
                // can style assistant_text vs send_message differently if
                // desired. Cheap, additive, no schema change.
                EntrySource::SendMessage => Some("source:send_message".to_string()),
                EntrySource::AssistantText => Some("source:assistant_text".to_string()),
                EntrySource::LeadReply => Some("source:lead_reply".to_string()),
            },
        })
        .collect()
}

fn empty_view() -> TeamView {
    TeamView {
        events: vec![],
        roster: TeamRoster {
            team_name: None,
            description: None,
            created_at: None,
            members: vec![],
            task_count_total: 0,
            task_count_completed: 0,
        },
    }
}
