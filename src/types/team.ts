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

/**
 * StructuredMessage 的 type discriminator（snake_case）。后端定义在
 * `src-tauri/src/runtime/messaging/structured.rs`，落盘到 team-chat.jsonl
 * 的 `variant` 字段。
 */
export type SendMessageVariant =
  | 'text'
  | 'shutdown_request'
  | 'shutdown_response'
  | 'plan_approval_request'
  | 'plan_approval_response'

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
      variant: SendMessageVariant
      /** ShutdownResponse / PlanApprovalResponse 才有。 */
      approve?: boolean
      /** ShutdownRequest / ShutdownResponse 可选 reason。 */
      reason?: string
      /** PlanApprovalResponse 可选 feedback。 */
      feedback?: string
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
