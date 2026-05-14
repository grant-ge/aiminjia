/**
 * Team view types — mirror Rust `runtime::team_view`.
 *
 * All field names are camelCase to match `#[serde(rename_all = "camelCase")]`
 * on the Rust side.
 */

export interface TeamAgent {
  agentId: string
  agentName: string
  spawnedAt: string
  isAsync: boolean
  hasTranscript: boolean
}

export type TeamEvent =
  | { kind: 'team_create'; ts: string; teamName: string | null }
  | { kind: 'team_delete'; ts: string }
  | { kind: 'agent_spawn'; ts: string; agentId: string; agentName: string }
  | { kind: 'agent_stop'; ts: string; agentName: string }
  | {
      kind: 'send_message'
      ts: string
      from: string
      to: string
      text: string
      isError: boolean
      toolCallId: string
    }
  | {
      kind: 'peer_message'
      ts: string
      from: string
      to: string
      text: string
      variant: string
    }

export interface TeamSession {
  teamId: string
  teamName: string | null
  createdAt: string
  deletedAt: string | null
  members: TeamAgent[]
  events: TeamEvent[]
}

export interface TeamOverview {
  conversationId: string
  teams: TeamSession[]
}
