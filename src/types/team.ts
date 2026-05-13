// 群聊事件流类型 —— 与 Rust `runtime::team_events::TeamEvent` 对应。
// 通过 Tauri 命令 `team_view_for_conversation(conversation_id)` 拉取。

export type TeamEvent =
  | { kind: 'team_created'; ts: string; team_name: string; description: string | null }
  | {
      kind: 'member_joined'
      ts: string
      agent_id: string
      name: string
      subagent_type: string | null
      description: string | null
      employee_id: string | null
    }
  | { kind: 'task_created'; ts: string; task_id: string; subject: string }
  | {
      kind: 'task_updated'
      ts: string
      task_id: string
      owner: string | null
      status: string | null
    }
  | {
      kind: 'message_sent'
      ts: string
      sender: string
      to: string
      content: string
      anchor_message_id: string | null
    }

export interface MemberInfo {
  agent_id: string
  name: string
  spawned_at: string | null
  employee_id: string | null
}

export interface TeamRoster {
  team_name: string | null
  description: string | null
  created_at: string | null
  members: MemberInfo[]
  task_count_total: number
  task_count_completed: number
}

export interface TeamView {
  events: TeamEvent[]
  roster: TeamRoster
}

/**
 * 群是否实际存在（有人调过 TeamCreate）。
 * 用作 inline TeamCard / 角标 / 抽屉触发器的判断条件。
 */
export function hasTeam(view: TeamView | null | undefined): boolean {
  return !!view && view.roster.team_name !== null
}

/**
 * Batch B: 群消息事件载荷（来自 Tauri 事件 `team:message`，对应后端
 * `RuntimeEventKind::TeamMessage`）。
 *
 * 历史消息由 `team_view_for_conversation` 通过反扫 transcript 派生为
 * `message_sent` 事件；本类型只描述实时增量。
 */
export interface TeamMessage {
  conversationId: string
  ts: string
  from: string
  to: string
  body: string
}
