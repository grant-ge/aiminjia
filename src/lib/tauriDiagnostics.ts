import { invoke } from '@tauri-apps/api/core'

export type DiagnosticLevel = 'debug' | 'info' | 'warn' | 'error'

export interface FrontendDiagnosticPayload {
  event: string
  level?: DiagnosticLevel
  ok?: boolean
  conversationId?: string
  runId?: string
  messageId?: string
  clientMessageId?: string
  toolCallId?: string
  agentId?: string
  interactionId?: string
  taskId?: string
  command?: string
  durationMs?: number
  elapsedMs?: number
  error?: string
  payload?: unknown
}

export function recordFrontendDiagnostic(diagnostic: FrontendDiagnosticPayload): Promise<void> {
  return invoke<void>('record_frontend_diagnostic', { diagnostic })
}
