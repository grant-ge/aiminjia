/**
 * Typed Tauri IPC wrappers.
 * Provides type-safe access to all Tauri backend commands and event listeners.
 *
 * Reference: tech-architecture.md §3.4 — Tauri IPC Layer
 *
 * Conventions:
 * - Tauri invoke uses snake_case for command names and parameter names.
 * - The Rust backend uses #[serde(rename_all = "camelCase")] so JSON
 *   responses are already camelCase — no client-side transformation needed.
 */

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

import { recordDiagnostic, recordDiagnosticError } from './diagnostics'
export type { DiagnosticLevel, FrontendDiagnosticPayload } from './tauriDiagnostics'
export { recordFrontendDiagnostic } from './tauriDiagnostics'

import type { Message, SubAgentTranscriptEntry } from '@/types/message'
import type { TeamOverview } from '@/types/team'
import type {
  PendingItem,
  PendingSnapshotPayload,
  PendingQueuedPayload,
  PendingDrainedPayload,
  PendingRemovedPayload,
} from '@/types/pending'
import type { Settings } from '@/types/settings'

// ---------------------------------------------------------------------------
// Tauri Event Constants
// ---------------------------------------------------------------------------

export const TAURI_EVENTS = {
  STREAMING_DELTA: 'streaming:delta',
  STREAMING_DONE: 'streaming:done',
  STREAMING_ERROR: 'streaming:error',
  STREAMING_RETRY_RESET: 'streaming:retry-reset',
  MESSAGE_UPDATED: 'message:updated',
  STOP_PREVENTED_CONTINUATION: 'stop:prevented-continuation',
  /** @deprecated 后端不发送此事件 */
  FILE_PARSED: 'file:parsed',
  FILE_GENERATED: 'file:generated',
  NOTIFICATION: 'notification',
  TOOL_EXECUTING: 'tool:executing',
  TOOL_COMPLETED: 'tool:completed',
  CONVERSATION_TITLE_UPDATED: 'conversation:title-updated',
  AGENT_IDLE: 'agent:idle',
  TASK_STATUS_CHANGED: 'task:status-changed',
  AUTH_EXPIRED: 'auth:expired',
  SKILL_FILE_CHANGED: 'skill-file-changed',
  PERMISSION_ASK: 'permission:ask',
  INTERACTION_REQUIRED: 'interaction:required',
  INTERACTION_RESOLVED: 'interaction:resolved',
  TURN_COMPLETED: 'turn:completed',
  DIAGNOSTICS_EVENT: 'diagnostics:event',
  CONVERSATION_CREATED: 'conversation:created',
  CHANNEL_PLATFORM_STATE: 'channel:platform-state',
  CHANNEL_MESSAGE: 'channel:message',
  /** LTR Path A: Lead finished a turn and has pending Teammate messages queued. */
  LEAD_HAS_PENDING_MESSAGES: 'lead:has-pending-messages',
  PENDING_SNAPSHOT: 'pending:snapshot',
  PENDING_QUEUED: 'pending:queued',
  PENDING_DRAINED: 'pending:drained',
  PENDING_REMOVED: 'pending:removed',
  /** Spec 2026-05-17 §4.1 — TurnStage transitions. */
  TURN_STAGE: 'turn:stage',
  /** Spec 2026-05-17 §4.1 — ~2s keep-alive while a turn is in progress. */
  TURN_HEARTBEAT: 'turn:heartbeat',
} as const

// ---------------------------------------------------------------------------
// Event Payload Types
// ---------------------------------------------------------------------------

export interface StreamingDeltaPayload {
  conversationId: string
  delta: string
}

export interface StreamingDonePayload {
  conversationId: string
}

export interface StreamingErrorPayload {
  conversationId: string
  error: string
  rawError?: string
}

export interface StreamingRetryResetPayload {
  conversationId: string
  runId?: string
}

export interface AgentIdlePayload {
  conversationId: string
  runId?: string
  agentId?: string
  scope?: 'primary' | 'child'
}

export interface ToolExecutingPayload {
  conversationId: string
  toolName: string
  toolId: string
  purpose?: string
  input?: unknown  // 完整入参 JSON 对象
  /**
   * 'child' 表示这是子 agent 内部的工具执行；前端在主对话工具轨迹里应过滤掉，
   * 这些事件留作"子 agent 详情"等未来用途。缺省（undefined / 'primary'）
   * 视为主 agent 自己的工具，正常渲染。
   */
  scope?: 'primary' | 'child'
}

/** @deprecated tool:completed 现在直接推完整 Message，保留此类型仅供旧引用过渡 */
export interface ToolCompletedPayload {
  conversationId: string
  toolName: string
  toolId: string
  success: boolean
  summary?: string
}

export interface ChatAttachmentPayload {
  id: string
  fileName: string
  filePath: string
  kind: 'file' | 'folder' | 'image'
  fileSize: number
  fileType: 'excel' | 'csv' | 'word' | 'pdf' | 'json' | 'folder' | 'image'
  mimeType?: string
}

export interface SavedClipboardAttachmentPayload {
  fileName: string
  path: string
  fileSize: number
  mimeType: string
}

export function readClipboardFilePaths(): Promise<string[]> {
  return invoke<string[]>('read_clipboard_file_paths')
}

export interface FileGeneratedPayload {
  conversationId: string
  fileId: string
  fileName: string
  requestedFormat: string
  actualFormat: string
  fileSize: number
  storedPath: string
  category: string
  isDegraded: boolean
  degradationNotice: string | null
}

export interface TaskStatusChangedPayload {
  conversationId: string
  taskId: string
  status: string
  runId: string
  subject: string
  description?: string
  activeForm?: string
  owner?: string
  blockedBy?: string[]
  createdAt?: string
}

export interface PermissionAskPayload {
  conversationId: string
  runId: string
  toolCallId: string
  toolName: string
  message: string
  suggestions: string[] | null
  mode: 'default' | 'plan' | 'dontAsk'
  rememberOptions: Array<'session' | 'workspace' | 'user'> | null
  defaultDestination: 'session' | 'workspace' | 'user' | null
}

export interface QuestionOption {
  label: string
  description: string
  preview?: string
}

export interface Question {
  question: string
  header: string
  options: QuestionOption[]
  multiSelect?: boolean
}

export interface InteractionRequiredPayload {
  conversationId: string
  runId: string
  interactionId: string
  toolCallId: string
  toolName: string
  kind: 'askUserQuestion'
  payload: {
    questions: Question[]
    metadata?: unknown
  }
}

export interface InteractionResolvedPayload {
  conversationId: string
  runId: string
  interactionId: string
}

export type TurnOutcome =
  | 'Success'
  | 'Cancelled'
  | 'MaxIterationsReached'
  | 'BudgetExceeded'
  | 'ExecutionError'

export interface TurnCompletedPayload {
  conversationId: string
  runId: string
  outcome: TurnOutcome
  totalInputTokens: number
  totalOutputTokens: number
  /** Anthropic-style prompt-cache write tokens accumulated this turn. */
  totalCacheCreationInputTokens?: number
  /** Anthropic-style prompt-cache read tokens accumulated this turn. */
  totalCacheReadInputTokens?: number
  totalCostUsd?: number | null
  permissionDenialCount: number
  iterations?: number
  reason?: string
  message?: string
}

// ---------------------------------------------------------------------------
// Turn-stage events (spec docs/superpowers/specs/2026-05-17-turn-stages.md)
// ---------------------------------------------------------------------------

export interface TurnRunningTool {
  toolName: string
  toolCallId: string
  startedAtMs: number
}

export type TurnStageKind =
  | { kind: 'submitted' }
  | { kind: 'waitingLlm';         iteration: number }
  | { kind: 'streaming';          iteration: number }
  | {
      kind: 'tools'
      iteration: number
      running: TurnRunningTool[]
      completedInBatch: number
    }
  | { kind: 'waitingPermission';  toolName: string; toolCallId: string }
  | {
      kind: 'waitingInteraction'
      interactionKind: string
      interactionId: string
    }
  | { kind: 'compacting' }
  | { kind: 'completing' }

export interface TurnStagePayload {
  conversationId: string
  runId: string
  stage: TurnStageKind
  stageStartedAtMs: number
}

export interface TurnHeartbeatPayload {
  conversationId: string
  runId: string
  stageElapsedMs: number
  turnElapsedMs: number
}

/** Mirror of backend `PersistedTurnStage` (turn_stage.json on disk). */
export interface PersistedTurnStage {
  schemaVersion: number
  conversationId: string
  runId: string
  stage: TurnStageKind
  stageStartedAtMs: number
  turnStartedAtMs: number
  lastHeartbeatAtMs: number
}

/** Mirror of backend `InterruptedTurnRecord` (interrupted_turn.json on disk). */
export interface InterruptedTurnRecord {
  conversationId: string
  runId: string
  lastStage: TurnStageKind
  interruptedAtMs: number
}

export interface DiagnosticsEventPayload {
  ts: string
  seq: number
  category: 'diagnostics'
  level: 'debug' | 'info' | 'warn' | 'error'
  source: 'frontend' | 'backend'
  event: string
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

export interface AgentInfo {
  name: string
  description: string
  source: 'builtin' | 'user'
}

// ---------------------------------------------------------------------------
// Chat Commands
// ---------------------------------------------------------------------------

/**
 * Send a user message to a conversation and trigger the AI response pipeline.
 *
 * @param conversationId - Target conversation ID
 * @param content - The user's message text
 * @param attachments - Optional list of structured attachments to attach
 */
export function sendMessage(
  conversationId: string,
  content: string,
  attachments?: ChatAttachmentPayload[],
  agentName?: string | null,
  clientMessageId?: string,
): Promise<void> {
  return invoke<void>('send_message', {
    conversationId,
    content,
    attachments: attachments ?? [],
    agentName: agentName ?? null,
    clientMessageId: clientMessageId ?? null,
  })
}

export function saveClipboardImageToTmp(
  bytes: number[],
  mimeType: string,
): Promise<SavedClipboardAttachmentPayload> {
  return invoke<SavedClipboardAttachmentPayload>('save_clipboard_image_to_tmp_dir', {
    bytes,
    mimeType,
  })
}

export function saveClipboardImageToWorkspaceStaging(
  bytes: number[],
  mimeType: string,
): Promise<SavedClipboardAttachmentPayload> {
  return invoke<SavedClipboardAttachmentPayload>('save_clipboard_image_to_workspace_staging', {
    bytes,
    mimeType,
  })
}

export function listAgents(): Promise<AgentInfo[]> {
  return invoke<AgentInfo[]>('list_agents')
}

/**
 * Abort the streaming response for a specific conversation.
 *
 * @param conversationId - The conversation whose streaming should be stopped
 */
export function stopStreaming(conversationId: string): Promise<void> {
  return invoke<void>('stop_streaming', { conversationId })
}

export function approvePermissionRequest(
  toolCallId: string,
  updatedInput: unknown,
  remember?: boolean,
  destination?: 'session' | 'workspace' | 'user',
): Promise<void> {
  return invoke<void>('approve_permission_request', {
    toolCallId,
    updatedInput,
    remember,
    destination,
  })
}

export function denyPermissionRequest(
  toolCallId: string,
  message?: string,
  remember?: boolean,
  destination?: 'session' | 'workspace' | 'user',
): Promise<void> {
  return invoke<void>('deny_permission_request', {
    toolCallId,
    message,
    remember,
    destination,
  })
}

export function cancelPermissionRequest(
  toolCallId: string,
  message?: string,
): Promise<void> {
  return invoke<void>('cancel_permission_request', { toolCallId, message })
}

export function submitUserInteraction(
  interactionId: string,
  value: { answers: Record<string, string>; annotations?: Record<string, unknown> },
): Promise<void> {
  return invoke<void>('submit_user_interaction', { interactionId, value })
}

export function cancelUserInteraction(
  interactionId: string,
  message?: string,
): Promise<void> {
  return invoke<void>('cancel_user_interaction', { interactionId, message })
}

/**
 * Retrieve all messages for a given conversation, ordered chronologically.
 *
 * @param conversationId - The conversation to fetch messages from
 * @returns Array of messages belonging to the conversation
 */
export function getMessages(conversationId: string): Promise<Message[]> {
  return invoke<Message[]>('get_messages', {
    conversationId,
  })
}

export function getTasks(
  conversationId: string,
): Promise<import('@/stores/streamingStore').ConversationTaskState[]> {
  return invoke('get_tasks', { conversationId })
}

export type ItemStatus = 'active' | 'paused' | 'completed' | 'orphaned' | 'cancelled'
export type OccurrenceStatus = 'running' | 'succeeded' | 'failed'
export type Freq = 'daily' | 'weekly' | 'monthly' | 'yearly'

export interface Participant {
  employeeId: string
  joinedAt: string
}

export interface RecurrenceRule {
  freq: Freq
  interval: number
  endCondition:
    | { kind: 'never' }
    | { kind: 'count'; n: number }
    | { kind: 'until'; at: string }
  byDay?: string[]
  byMonthDay?: number[]
}

export interface OverrideRef {
  seriesItemId: string
  originalAt: string
}

export interface AgendaItem {
  id: string
  title: string
  prompt: string
  organizerEmployeeId: string
  participants: Participant[]
  startAt: string
  timezone: string
  rule: RecurrenceRule | null
  skipDates: string[]
  nextFireAt: string | null
  occurrenceCount: number
  status: ItemStatus
  overrideOf: OverrideRef | null
  workspacePath: string | null
  createdAt: string
  updatedAt: string
}

export interface Occurrence {
  id: string
  agendaItemId: string
  firedAt: string
  plannedFireAt: string
  startedAt: string
  finishedAt: string | null
  primaryEmployeeId: string
  conversationId: string
  sessionId: string
  runId: string
  status: OccurrenceStatus
  errorSummary: string | null
  triggerSource: 'scheduled' | 'manual_run_now'
}

export interface ItemFilter {
  statusIn?: ItemStatus[]
  employeeId?: string
  search?: string
}

export interface CreateAgendaItemRequest {
  title: string
  prompt: string
  organizerEmployeeId: string
  startAt: string
  timezone?: string
  rule?: RecurrenceRule | null
  workspacePath?: string | null
}

export interface UpdateAgendaItemRequest {
  title?: string
  prompt?: string
  startAt?: string
  timezone?: string
  rule?: RecurrenceRule | null
  status?: ItemStatus
  workspacePath?: string | null
}

export function listAgendaItems(filter?: ItemFilter): Promise<AgendaItem[]> {
  return invoke<AgendaItem[]>('list_agenda_items', { filter })
}
export function getAgendaItem(id: string): Promise<AgendaItem> {
  return invoke<AgendaItem>('get_agenda_item', { id })
}
export function createAgendaItem(request: CreateAgendaItemRequest): Promise<AgendaItem> {
  return invoke<AgendaItem>('create_agenda_item', { request })
}
export function updateAgendaItem(
  id: string,
  request: UpdateAgendaItemRequest,
): Promise<AgendaItem> {
  return invoke<AgendaItem>('update_agenda_item', { id, request })
}
export function deleteAgendaItem(id: string): Promise<boolean> {
  return invoke<boolean>('delete_agenda_item', { id })
}
export function cancelAgendaItem(id: string): Promise<AgendaItem> {
  return invoke<AgendaItem>('cancel_agenda_item', { id })
}
export function restoreAgendaItem(id: string): Promise<AgendaItem> {
  return invoke<AgendaItem>('restore_agenda_item', { id })
}
export function runAgendaItemNow(id: string): Promise<string> {
  return invoke<string>('run_agenda_item_now', { id })
}
export function listAgendaOccurrences(itemId: string, limit?: number): Promise<Occurrence[]> {
  return invoke<Occurrence[]>('list_agenda_occurrences', { itemId, limit })
}
export function skipOccurrence(id: string, at: string): Promise<AgendaItem> {
  return invoke<AgendaItem>('skip_occurrence', { id, at })
}
export function unskipOccurrence(id: string, at: string): Promise<AgendaItem> {
  return invoke<AgendaItem>('unskip_occurrence', { id, at })
}

export function getSubagentTranscript(
  transcriptRef: string,
): Promise<SubAgentTranscriptEntry[]> {
  return invoke<SubAgentTranscriptEntry[]>('get_subagent_transcript', {
    transcriptRef,
  })
}

/**
 * Read-only snapshot of a conversation's team activity (TeamCreate → TeamDelete
 * windows, member roster, chronological event stream). Returns `{ teams: [] }`
 * for conversations that never had a team.
 */
export function getTeamOverview(conversationId: string): Promise<TeamOverview> {
  return invoke<TeamOverview>('get_team_overview', { conversationId })
}

/**
 * Read one teammate's complete on-disk transcript. Returns parsed jsonl
 * entries (each entry is `{role, content, tool_calls?, tool_call_id?, tool_name?}`).
 */
export function getTeammateTranscript(
  conversationId: string,
  agentId: string,
): Promise<unknown[]> {
  return invoke<unknown[]>('get_teammate_transcript', {
    conversationId,
    agentId,
  })
}

/**
 * PR9: one line out of `<conv>/teams/{team}/team-chat.jsonl`.  The shape
 * matches what the writer (SendMessage tool) puts on disk.
 */
export interface TeamChatMessage {
  ts: string
  from: string
  to: string
  text: string
  variant?: string
}

/**
 * PR9: read the (optionally filtered/limited) tail of a team's team-chat.jsonl.
 *
 * - `sinceTs` filters out lines with `ts <= sinceTs` (string comparison, RFC3339-safe).
 * - `limit` caps the result length.
 */
export function teamChatMessages(
  conversationId: string,
  teamName: string,
  sinceTs?: string,
  limit?: number,
): Promise<TeamChatMessage[]> {
  return invoke<TeamChatMessage[]>('team_chat_messages', {
    conversationId,
    teamName,
    sinceTs,
    limit,
  })
}

/**
 * Create a new empty conversation.
 *
 * @returns The ID of the newly created conversation
 */
export function createConversation(): Promise<string> {
  return invoke<string>('create_conversation')
}

/**
 * Get all conversations.
 *
 * @returns Array of conversation objects from the database
 */
export function getConversations(): Promise<Record<string, unknown>[]> {
  return invoke<Record<string, unknown>[]>('get_conversations')
}

/**
 * Delete a conversation and all its associated messages.
 *
 * @param conversationId - The conversation to delete
 */
export function deleteConversation(conversationId: string): Promise<void> {
  return invoke<void>('delete_conversation', {
    conversationId,
  })
}

/**
 * Rename a conversation.
 *
 * @param conversationId - The conversation to rename
 * @param newTitle - The new title
 */
export function renameConversation(conversationId: string, newTitle: string): Promise<void> {
  return invoke<void>('rename_conversation', {
    conversationId,
    newTitle,
  })
}

export function archiveConversation(conversationId: string): Promise<void> {
  return invoke<void>('archive_conversation', { conversationId })
}

// ---------------------------------------------------------------------------
// Channel types
// ---------------------------------------------------------------------------

export type ChannelPlatform = 'dingtalk' | 'feishu' | 'wechat' | 'wecom'

export type ChannelCapability = 'available' | 'comingSoon'

export type ChannelConnectionState =
  | 'unconfigured'
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'configError'

export type RobotCodeSource = 'registration' | 'appKeyFallback'

export interface ChannelConfigView {
  platform: ChannelPlatform
  appKey: string
  appSecretMasked: string
  robotCode: string
  robotCodeSource: RobotCodeSource
  source: 'OPEN_CLAW'
  createdAt: string
  updatedAt: string
}

export interface ChannelPlatformState {
  platform: ChannelPlatform
  capability: ChannelCapability
  configured: boolean
  enabled: boolean
  connection: ChannelConnectionState
  config?: ChannelConfigView | null
  lastConnectedAt?: string | null
  lastError?: string | null
}

export interface ChannelPlatformStatePayload {
  state: ChannelPlatformState
}

export interface ChannelMessagePayload {
  platform: ChannelPlatform
  sessionId: string
  senderNick: string
  textPreview: string
}

export interface ChannelConversation {
  sessionId: string
  platform: ChannelPlatform
  conversationType: 'group' | 'private'
  externalId: string
  displayName: string
  unreadCount: number
  robotCode: string
  isActiveRobot: boolean
}

export interface ChannelRegistrationBeginResult {
  deviceCode: string
  userCode: string
  verificationUriComplete: string
  verificationUri: string
  intervalSeconds: number
  expiresInSeconds: number
  source: string
}

export interface ChannelRegistrationPollResult {
  state: 'waiting' | 'success' | 'fail' | 'expired' | 'unknown'
  clientId?: string | null
  robotCode?: string | null
  config?: ChannelConfigView | null
  platformState?: ChannelPlatformState | null
  failReason?: string | null
}

// ---------------------------------------------------------------------------
// Channel IPC
// ---------------------------------------------------------------------------

export function channelGetPlatforms(): Promise<ChannelPlatformState[]> {
  return invoke<ChannelPlatformState[]>('channel_get_platforms')
}

export function channelGetPlatform(platform: ChannelPlatform): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_get_platform', { platform })
}

export function channelGetConversations(
  platform?: ChannelPlatform,
): Promise<ChannelConversation[]> {
  return invoke<ChannelConversation[]>('channel_get_conversations', { platform })
}

export function channelBeginRegistration(
  platform: ChannelPlatform,
): Promise<ChannelRegistrationBeginResult> {
  return invoke<ChannelRegistrationBeginResult>('channel_begin_registration', { platform })
}

export function channelPollRegistration(
  platform: ChannelPlatform,
  deviceCode: string,
): Promise<ChannelRegistrationPollResult> {
  return invoke<ChannelRegistrationPollResult>('channel_poll_registration', { platform, deviceCode })
}

export function channelSetEnabled(
  platform: ChannelPlatform,
  enabled: boolean,
): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_set_enabled', { platform, enabled })
}

export function channelRemovePlatform(platform: ChannelPlatform): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_remove_platform', { platform })
}

export function channelRevealSecret(platform: ChannelPlatform): Promise<string> {
  return invoke<string>('channel_reveal_secret', { platform })
}

export function onChannelPlatformState(
  handler: (payload: ChannelPlatformStatePayload) => void,
): Promise<() => void> {
  return listen<ChannelPlatformStatePayload>(
    TAURI_EVENTS.CHANNEL_PLATFORM_STATE,
    (e) => handler(e.payload),
  )
}

export function onChannelMessage(
  handler: (payload: ChannelMessagePayload) => void,
): Promise<() => void> {
  return listen<ChannelMessagePayload>(TAURI_EVENTS.CHANNEL_MESSAGE, (e) => handler(e.payload))
}

export function restoreConversation(conversationId: string): Promise<void> {
  return invoke<void>('restore_conversation', { conversationId })
}

export function getArchivedConversations(): Promise<Array<{ id: string; title: string; updatedAt: string; isArchived: boolean }>> {
  return invoke('get_archived_conversations')
}

/**
 * Check which conversations currently have active agent tasks.
 *
 * @returns Array of conversation IDs that are being processed
 */
export function isAgentBusy(): Promise<string[]> {
  return invoke<string[]>('is_agent_busy')
}

// ---------------------------------------------------------------------------
// File Commands
// ---------------------------------------------------------------------------

/**
 * Upload a file from the local filesystem to the workspace for analysis.
 *
 * @param filePath - Absolute path to the file on disk
 * @param conversationId - Conversation to associate the file with
 * @returns Upload result with file ID and file size in bytes
 */
export function uploadFile(filePath: string, conversationId: string): Promise<{ fileId: string; fileSize: number }> {
  return invoke<{ fileId: string; fileSize: number }>('upload_file', {
    filePath,
    conversationId,
  })
}

/**
 * Open a generated file using the system's default application.
 *
 * @param fileId - ID of the generated file to open
 * @param conversationId - Conversation that owns the file
 */
export function openGeneratedFile(fileId: string, conversationId: string): Promise<void> {
  return invoke<void>('open_generated_file', {
    fileId,
    conversationId,
  })
}

/**
 * Reveal a file in the OS file manager (Finder / Explorer).
 *
 * @param fileId - ID of the file to reveal
 * @param conversationId - Conversation that owns the file
 */
export function revealFileInFolder(fileId: string, conversationId: string): Promise<void> {
  return invoke<void>('reveal_file_in_folder', {
    fileId,
    conversationId,
  })
}

/**
 * Generate a preview (e.g. HTML string or base64 image) for a file.
 *
 * @param fileId - ID of the file to preview
 * @param conversationId - Conversation that owns the file
 * @returns Preview content as a string (HTML or data URI)
 */
export function previewFile(fileId: string, conversationId: string): Promise<string> {
  return invoke<string>('preview_file', {
    fileId,
    conversationId,
  })
}

export type FilePreview =
  | { kind: 'markdown' | 'text' | 'json' | 'csv'; fileName: string; mimeType: string; content: string }
  | { kind: 'html'; fileName: string; mimeType: 'text/html'; content: string; sandbox: true }
  | { kind: 'image'; fileName: string; mimeType: string; dataUrl: string }
  | { kind: 'unsupported'; fileName: string; reason: string }

export function getFilePreview(fileId: string, conversationId: string): Promise<FilePreview> {
  return invoke<FilePreview>('get_file_preview', { fileId, conversationId })
}

export function getLocalFilePreview(path: string): Promise<FilePreview> {
  return invoke<FilePreview>('get_local_file_preview', { path })
}

export function openLocalFile(path: string): Promise<void> {
  return invoke<void>('open_local_file', { path })
}

/**
 * Delete a generated or uploaded file from the workspace.
 *
 * @param fileId - ID of the file to delete
 * @param conversationId - Conversation that owns the file
 */
export function deleteFile(fileId: string, conversationId: string): Promise<void> {
  return invoke<void>('delete_file', {
    fileId,
    conversationId,
  })
}

/**
 * Open a file by its display name, searching across all workspace subdirectories.
 * Used for inline file name links in chat text.
 *
 * @param fileName - The file name to search for (e.g. "report.xlsx")
 */
export function openFileByName(fileName: string): Promise<void> {
  return invoke<void>('open_file_by_name', { fileName })
}

/**
 * Reveal a file in the OS file manager by its display name.
 *
 * @param fileName - The file name to search for
 */
export function revealFileByName(fileName: string): Promise<void> {
  return invoke<void>('reveal_file_by_name', { fileName })
}

// ---------------------------------------------------------------------------
// Settings Commands
// ---------------------------------------------------------------------------

/**
 * Retrieve the current application settings.
 *
 * @returns The full Settings object
 */
export function getSettings(): Promise<Settings> {
  return invoke<Settings>('get_settings')
}

/**
 * Persist updated application settings.
 *
 * @param settings - The complete Settings object to save
 */
export function updateSettings(settings: Settings): Promise<void> {
  return invoke<void>('update_settings', { settings })
}

/**
 * Validate an API key by making a lightweight test request to the provider.
 *
 * @param provider - The LLM provider identifier (e.g. 'deepseek-v3', 'openai')
 * @param apiKey - The API key to validate
 * @returns `true` if the key is valid, `false` otherwise
 */
export function validateApiKey(provider: string, apiKey: string): Promise<boolean> {
  return invoke<boolean>('validate_api_key', {
    provider,
    apiKey,
  })
}

/**
 * Get the list of providers that have a saved API key.
 *
 * @returns Array of provider identifiers (e.g. ['deepseek-v3', 'openai'])
 */
export function getConfiguredProviders(): Promise<string[]> {
  return invoke<string[]>('get_configured_providers')
}

/**
 * Switch the active provider. Loads the stored API key for the target provider
 * and updates primaryModel + primaryApiKey in the backend.
 *
 * @param provider - The provider to switch to
 */
export function switchProvider(provider: string): Promise<void> {
  return invoke<void>('switch_provider', { provider })
}

/**
 * Get all per-provider API keys (decrypted). Used by the settings modal
 * to populate key inputs for all provider tabs.
 *
 * @returns Map of provider identifier → plaintext API key
 */
export function getAllProviderKeys(): Promise<Record<string, string>> {
  return invoke<Record<string, string>>('get_all_provider_keys')
}

/**
 * Batch-save all provider API keys. Used by the settings modal to persist
 * all configured keys at once.
 *
 * @param keys - Map of provider identifier → plaintext API key
 */
export function updateAllProviderKeys(keys: Record<string, string>): Promise<void> {
  return invoke<void>('update_all_provider_keys', { keys })
}

// ---------------------------------------------------------------------------
// Workspace Commands
// ---------------------------------------------------------------------------

/**
 * Set the active workspace directory for file storage and analysis output.
 *
 * @param path - Absolute path to the workspace directory
 */
export function selectWorkspace(path: string): Promise<void> {
  return invoke<void>('select_workspace', { path })
}

/**
 * Get information about the current workspace (path, size, file count, etc.).
 *
 * @returns Workspace info as a serialized string
 */
export function getWorkspaceInfo(): Promise<string> {
  return invoke<string>('get_workspace_info')
}

// ---------------------------------------------------------------------------
// Authorized Workspace Commands (Phase W1)
// ---------------------------------------------------------------------------

/** Lightweight reference to an authorized local directory. */
export interface AuthorizedWorkspaceRef {
  id: string
  rootPath: string
  displayName: string
}

interface PickLocalDirectoryOptions {
  defaultPath?: string
  title?: string
}

/**
 * Open the native folder picker and return the selected directory path.
 *
 * @param options - Optional initial directory and custom title
 * @returns Absolute path string, or null when the user cancels
 */
export function pickLocalDirectory(
  options?: PickLocalDirectoryOptions,
): Promise<string | null> {
  return invoke<string | null>('pick_local_directory', {
    defaultPath: options?.defaultPath ?? null,
    title: options?.title ?? null,
  })
}

/**
 * Authorize a local directory for tool access within a session.
 * Replaces any previously authorized directory for the same session.
 *
 * @param path - Absolute path to the directory to authorize
 * @param sessionId - The session that will own this authorization
 * @returns A reference to the newly authorized workspace
 */
export function authorizeLocalDirectory(
  path: string,
  sessionId: string,
): Promise<AuthorizedWorkspaceRef> {
  return invoke<AuthorizedWorkspaceRef>('authorize_local_directory', { path, sessionId })
}

/**
 * Get the currently authorized workspace for a session.
 *
 * @param sessionId - The session to query
 * @returns The authorized workspace ref, or null if none is set
 */
export function getAuthorizedWorkspace(
  sessionId: string,
): Promise<AuthorizedWorkspaceRef | null> {
  return invoke<AuthorizedWorkspaceRef | null>('get_authorized_workspace', { sessionId })
}

/**
 * Get the default folder (~/.renlijia/defaultFolder) as a workspace ref.
 * Always returns a value; the directory is guaranteed to exist at startup.
 */
export function getDefaultFolder(): Promise<AuthorizedWorkspaceRef> {
  return invoke<AuthorizedWorkspaceRef>('get_default_folder')
}

/**
 * Revoke the authorized workspace for a session.
 *
 * @param sessionId - The session whose authorization should be cleared
 */
export function revokeAuthorizedWorkspace(sessionId: string): Promise<void> {
  return invoke<void>('revoke_authorized_workspace', { sessionId })
}

/**
 * Open the logs directory in the system file manager.
 */
export function openLogsDirectory(): Promise<void> {
  return invoke<void>('open_logs_directory')
}

export interface UploadDiagnosticsResult {
  session_id: string
  chunks_uploaded: number
  chunks_total: number
  events_uploaded: number
  app_log_lines_uploaded: number
  bad_metrics_lines: number
}

/**
 * Read local diagnostic logs (renlijia.log + metrics.jsonl) and upload them
 * in chunks to the gateway for support investigation. Returns a summary used
 * by the settings panel to show a confirmation toast.
 */
export function uploadDiagnosticLogs(): Promise<UploadDiagnosticsResult> {
  return invoke<UploadDiagnosticsResult>('upload_diagnostic_logs')
}

/**
 * Open the workspace root directory in the system file manager.
 */
export function openWorkspaceDirectory(): Promise<void> {
  return invoke<void>('open_workspace_directory')
}

/**
 * Export all metrics entries to a JSON file.
 *
 * @param destPath - Absolute path for the exported file (from save dialog)
 * @returns Export result with path, entry count, and file size
 */
export function exportMetrics(destPath: string): Promise<{ path: string; entryCount: number; fileSize: number }> {
  return invoke<{ path: string; entryCount: number; fileSize: number }>('export_metrics', { destPath })
}

/**
 * Clear all metrics JSONL files.
 *
 * @returns Number of deleted files
 */
export function clearMetrics(): Promise<{ deletedFiles: number }> {
  return invoke<{ deletedFiles: number }>('clear_metrics')
}

/**
 * Get metrics file info (entry count + total bytes).
 *
 * @returns Metrics info with entry count and total bytes
 */
export function getMetricsInfo(): Promise<{ entryCount: number; totalBytes: number }> {
  return invoke<{ entryCount: number; totalBytes: number }>('get_metrics_info')
}


// ---------------------------------------------------------------------------
// Plugin Commands
// ---------------------------------------------------------------------------

/** Info about a registered tool */
export interface ToolInfo {
  name: string
  description: string
  source: string // "builtin" | "plugin"
}

/** Info about a registered skill */
export interface SkillInfo {
  id: string
  displayName: string
  displayNameEn: string
  description: string
  source: string
  hasWorkflow: boolean
  icon: string
  shortDescription: string
  shortDescriptionEn: string
  triggerText: string
  category: string
  /**
   * 技能"更新时间"。后端返回 RFC 3339 UTC 字符串；读不到时为 null。
   * 当前实现：技能根目录 mtime（见 src-tauri/src/plugin/skill/updated_at.rs）。
   */
  updatedAt: string | null
  /**
   * SKILL.md frontmatter `version:` 字段。前端把它作为 chip 显示在
   * 技能卡片标题旁；技能没声明 version 时为 null。
   */
  version?: string | null
}

/** Combined plugin info (tools + skills) */
export interface PluginInfo {
  tools: ToolInfo[]
  skills: SkillInfo[]
}

/** List all registered tools. */
export function listTools(): Promise<ToolInfo[]> {
  return invoke<ToolInfo[]>('list_tools')
}

/** List all registered skills. */
export function listSkills(): Promise<SkillInfo[]> {
  return invoke<SkillInfo[]>('list_skills')
}

/** Get combined tool + skill info. */
export function getPluginInfo(): Promise<PluginInfo> {
  return invoke<PluginInfo>('get_plugin_info')
}

// ---------------------------------------------------------------------------
// Auth Commands
// ---------------------------------------------------------------------------

/** Cloud auth info returned from login/get_cloud_auth. */
export interface CloudAuthInfo {
  loggedIn: boolean
  user: { id: number; name: string; username: string } | null
  tenant: { id: number; name: string; balance: string; productName?: string; logoUrl?: string; accentColor?: string; primaryColor?: string; bgColor?: string; sidebarBgColor?: string; fontFamily?: string } | null
  models: CloudModel[]
}

/** Cloud model info from /v1/models. */
export interface CloudModel {
  id: string
  name: string
  modelType: string
}

/**
 * Persona summary for list API
 * @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。
 */
export interface PersonaSummary {
  id: string
  name: string
  nameEn: string
  icon: string
  description: string
  descriptionEn: string
  builtin: boolean
}

/**
 * Full persona definition
 * @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。
 */
export interface Persona {
  id: string
  version: number
  builtin: boolean
  name: string
  icon: string
  description: string
  identity: string
  expertise: string[]
  memoryHints: string[]
  linkedCategories: string[]
  createdAt: string
  updatedAt: string
}

/**
 * Login with username and password to Lotus cloud.
 *
 * @returns Auth info including user, tenant, and available models
 */
export function cloudLogin(username: string, password: string): Promise<CloudAuthInfo> {
  return invoke<CloudAuthInfo>('cloud_login', { username, password })
}

/** Logout from cloud mode. */
export function cloudLogout(): Promise<void> {
  return invoke<void>('cloud_logout')
}

/** Get current cloud auth state (for app init / restore). */
export function getCloudAuth(): Promise<CloudAuthInfo> {
  return invoke<CloudAuthInfo>('get_cloud_auth')
}

/** Fetch available cloud models. */
export function getCloudModels(): Promise<CloudModel[]> {
  return invoke<CloudModel[]>('get_cloud_models')
}

/**
 * Change password on the cloud server.
 * After success, the user is automatically logged out.
 */
export function cloudChangePassword(oldPassword: string, newPassword: string): Promise<void> {
  return invoke<void>('cloud_change_password', { oldPassword, newPassword })
}

/**
 * Cached brand snapshot persisted at `~/.renlijia/users/{scope}/brand.json`.
 * Used to re-apply the last tenant's branding on the login page after logout.
 */
export interface BrandSnapshot {
  productName?: string
  logoUrl?: string
  accentColor?: string
  primaryColor?: string
  bgColor?: string
  sidebarBgColor?: string
  fontFamily?: string
}

/** Read the cached brand snapshot for the last-active account on this machine. */
export function getLastBrand(): Promise<BrandSnapshot | null> {
  return invoke<BrandSnapshot | null>('get_last_brand')
}

/** Persist the brand snapshot for the currently-active account. */
export function saveLastBrand(brand: BrandSnapshot): Promise<void> {
  return invoke<void>('save_last_brand', { brand })
}

/** Send an SMS verification code for personal registration. */
export function cloudSendSmsCode(phone: string): Promise<void> {
  return invoke<void>('cloud_send_sms_code', { phone })
}

/** Send an email verification code for personal registration. */
export function cloudSendEmailCode(email: string): Promise<void> {
  return invoke<void>('cloud_send_email_code', { email })
}

/**
 * Register a personal account via phone or email.
 * On success the caller should immediately call `cloudLogin(identifier, password)`
 * — registration does not auto-establish a session.
 */
export function cloudRegister(args: {
  method: 'phone' | 'email'
  phone?: string
  email?: string
  code: string
  password: string
  name?: string
}): Promise<void> {
  return invoke<void>('cloud_register', args)
}

// ---------------------------------------------------------------------------
// Persona Commands
//
// @deprecated Persona 系统已进入退役流程（2026-05-10 决定）。
// 新方向是数字员工（Employee, `employee_*` commands）一统，PR-5 会把
// `AgendaItem.organizer_persona_id` 迁到 `organizer_employee_id`。
// 保留这些导出仅是为了 agenda runtime 切换前的兼容窗口；新代码不要再引用。
// ---------------------------------------------------------------------------

/** @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。 */
export function listPersonas(): Promise<PersonaSummary[]> {
  return invoke<PersonaSummary[]>('list_personas')
}

/** @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。 */
export function getPersona(id: string): Promise<Persona> {
  return invoke<Persona>('get_persona', { id })
}

/** @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。 */
export function savePersona(persona: Persona): Promise<void> {
  return invoke<void>('save_persona', { persona })
}

/** @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。 */
export function deletePersona(id: string): Promise<void> {
  return invoke<void>('delete_persona', { id })
}

/** @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。 */
export function setActivePersona(id: string): Promise<void> {
  return invoke<void>('set_active_persona', { id })
}

/** @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。 */
export function getActivePersona(): Promise<Persona> {
  return invoke<Persona>('get_active_persona')
}

/** @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。 */
export function exportPersonas(id: string): Promise<string> {
  return invoke<string>('export_personas', { id })
}

/** @deprecated Persona 系统将在 PR-5 退役，由 Employee 替代。 */
export function importPersonas(json: string): Promise<string> {
  return invoke<string>('import_personas', { json })
}

// ---------------------------------------------------------------------------
// Typed Event Listeners
// ---------------------------------------------------------------------------

interface TauriEventEnvelope<T> {
  payload: T
}

function getStringField(payload: unknown, key: string): string | undefined {
  if (!payload || typeof payload !== 'object' || !(key in payload)) return undefined
  const value = (payload as Record<string, unknown>)[key]
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

function getConversationIdFromPayload(payload: unknown): string | undefined {
  return getStringField(payload, 'conversationId')
}

function getRunIdFromPayload(payload: unknown): string | undefined {
  return getStringField(payload, 'runId')
}

export function createInstrumentedEventHandler<T>(
  eventName: string,
  handler: (event: TauriEventEnvelope<T>) => void | Promise<void>,
): (event: TauriEventEnvelope<T>) => Promise<void> {
  return async (event) => {
    const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now()
    const conversationId = getConversationIdFromPayload(event.payload)
    const runId = getRunIdFromPayload(event.payload)

    recordDiagnostic({
      event: 'event.received',
      conversationId,
      runId,
      payload: { eventName, payload: event.payload },
    })
    recordDiagnostic({
      event: 'event.handler.started',
      conversationId,
      runId,
      payload: { eventName },
    })

    try {
      await handler(event)
      const endedAt = typeof performance !== 'undefined' ? performance.now() : Date.now()
      recordDiagnostic({
        event: 'event.handler.completed',
        ok: true,
        conversationId,
        runId,
        durationMs: Math.round(endedAt - startedAt),
        payload: { eventName },
      })
    } catch (error) {
      const endedAt = typeof performance !== 'undefined' ? performance.now() : Date.now()
      recordDiagnosticError('event.handler.failed', error, {
        conversationId,
        runId,
        durationMs: Math.round(endedAt - startedAt),
        payload: { eventName },
      })
      throw error
    }
  }
}

/**
 * Listen for streaming text deltas as the AI model generates a response.
 *
 * @param handler - Callback receiving each text delta chunk with conversationId
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onStreamingDelta(
  handler: (payload: StreamingDeltaPayload) => void,
): Promise<() => void> {
  return listen<StreamingDeltaPayload>(TAURI_EVENTS.STREAMING_DELTA, createInstrumentedEventHandler(TAURI_EVENTS.STREAMING_DELTA, (event) => {
    handler(event.payload)
  }))
}

/**
 * Listen for the streaming completion event.
 *
 * @param handler - Callback receiving the conversationId and final message ID
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onStreamingDone(
  handler: (payload: StreamingDonePayload) => void,
): Promise<() => void> {
  return listen<StreamingDonePayload>(TAURI_EVENTS.STREAMING_DONE, createInstrumentedEventHandler(TAURI_EVENTS.STREAMING_DONE, (event) => {
    handler(event.payload)
  }))
}

/**
 * Listen for streaming error events (e.g. network failure, rate limit).
 *
 * @param handler - Callback receiving the conversationId and error description
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onStreamingError(
  handler: (payload: StreamingErrorPayload) => void,
): Promise<() => void> {
  return listen<StreamingErrorPayload>(TAURI_EVENTS.STREAMING_ERROR, createInstrumentedEventHandler(TAURI_EVENTS.STREAMING_ERROR, (event) => {
    handler(event.payload)
  }))
}

export function onStreamingRetryReset(
  handler: (payload: StreamingRetryResetPayload) => void,
): Promise<() => void> {
  return listen<StreamingRetryResetPayload>(TAURI_EVENTS.STREAMING_RETRY_RESET, createInstrumentedEventHandler(TAURI_EVENTS.STREAMING_RETRY_RESET, (event) => {
    handler(event.payload)
  }))
}

/**
 * Listen for message update events (e.g. when a message's content is enriched
 * with additional blocks like tables, code results, or generated files).
 *
 * @param handler - Callback receiving the updated Message object
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onMessageUpdated(
  handler: (payload: Message) => void,
): Promise<() => void> {
  return listen<Message>(TAURI_EVENTS.MESSAGE_UPDATED, createInstrumentedEventHandler(TAURI_EVENTS.MESSAGE_UPDATED, (event) => {
    handler(event.payload)
  }))
}

/**
 * Listen for application-level notification events (toast messages).
 *
 * @param handler - Callback receiving the notification level, title, and message
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onNotification(
  handler: (payload: { level: string; title: string; message: string }) => void,
): Promise<() => void> {
  return listen<{ level: string; title: string; message: string }>(TAURI_EVENTS.NOTIFICATION, createInstrumentedEventHandler(TAURI_EVENTS.NOTIFICATION, (event) => {
    handler(event.payload)
  }))
}

/**
 * Listen for tool execution start events.
 *
 * @param handler - Callback receiving the conversationId, tool name, unique tool ID, and optional purpose description
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onToolExecuting(
  handler: (payload: ToolExecutingPayload) => void,
): Promise<() => void> {
  return listen<ToolExecutingPayload>(TAURI_EVENTS.TOOL_EXECUTING, createInstrumentedEventHandler(TAURI_EVENTS.TOOL_EXECUTING, (event) => {
    handler(event.payload)
  }))
}

/**
 * Listen for tool execution completion events.
 *
 * @param handler - Callback receiving the conversationId, tool name, unique tool ID, success flag, and optional summary
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onToolCompleted(
  handler: (payload: Message) => void,
): Promise<() => void> {
  return listen<Message>(TAURI_EVENTS.TOOL_COMPLETED, createInstrumentedEventHandler(TAURI_EVENTS.TOOL_COMPLETED, (event) => {
    handler(event.payload)
  }))
}

/**
 * Listen for conversation title update events (auto-generated after first AI response).
 *
 * @param handler - Callback receiving the conversation ID and new title
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onConversationTitleUpdated(
  handler: (payload: { conversationId: string; title: string }) => void,
): Promise<() => void> {
  return listen<{ conversationId: string; title: string }>(TAURI_EVENTS.CONVERSATION_TITLE_UPDATED, createInstrumentedEventHandler(TAURI_EVENTS.CONVERSATION_TITLE_UPDATED, (event) => {
    handler(event.payload)
  }))
}

export interface ConversationCreatedPayload {
  conversationId: string
  source: 'user' | 'agenda' | 'employee' | 'schedule' | string
  title: string | null
}

/**
 * 监听后端创建新 conversation 事件。所有直接走后端 create_conversation
 * 的路径（agenda / employee / schedule_runner / user IPC）都会 emit。
 *
 * sidebar 收到后应当 reload 对话列表。**不应当切路由或换 activeConversationId**：
 * 用户可能正在其它对话里操作，sidebar 只是多出一行而已。
 */
export function onConversationCreated(
  handler: (payload: ConversationCreatedPayload) => void,
): Promise<() => void> {
  return listen<ConversationCreatedPayload>(
    TAURI_EVENTS.CONVERSATION_CREATED,
    createInstrumentedEventHandler(TAURI_EVENTS.CONVERSATION_CREATED, (event) => {
      handler(event.payload)
    }),
  )
}

/**
 * Listen for agent idle events (emitted when the agent loop finishes).
 *
 * @param handler - Callback receiving the conversationId of the finished agent
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onAgentIdle(
  handler: (payload: AgentIdlePayload) => void,
): Promise<() => void> {
  return listen<AgentIdlePayload>(TAURI_EVENTS.AGENT_IDLE, createInstrumentedEventHandler(TAURI_EVENTS.AGENT_IDLE, (event) => {
    handler(event.payload)
  }))
}



/**
 * Listen for file:generated events (emitted directly by the tool execution layer,
 * bypassing LLM). Used to show immediate file feedback and degradation warnings.
 *
 * @param handler - Callback receiving the file generation details
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onFileGenerated(
  handler: (payload: FileGeneratedPayload) => void,
): Promise<() => void> {
  return listen<FileGeneratedPayload>(TAURI_EVENTS.FILE_GENERATED, createInstrumentedEventHandler(TAURI_EVENTS.FILE_GENERATED, (event) => {
    handler(event.payload)
  }))
}

/**
 * Listen for task status change events (emitted by the runtime task system).
 *
 * @param handler - Callback receiving the task status change payload
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onTaskStatusChanged(
  handler: (payload: TaskStatusChangedPayload) => void,
): Promise<() => void> {
  return listen<TaskStatusChangedPayload>(TAURI_EVENTS.TASK_STATUS_CHANGED, createInstrumentedEventHandler(TAURI_EVENTS.TASK_STATUS_CHANGED, (event) => {
    handler(event.payload)
  }))
}

export function onPermissionAsk(
  handler: (payload: PermissionAskPayload) => void,
): Promise<() => void> {
  return listen<PermissionAskPayload>(TAURI_EVENTS.PERMISSION_ASK, createInstrumentedEventHandler(TAURI_EVENTS.PERMISSION_ASK, (event) => {
    handler(event.payload)
  }))
}

export function onInteractionRequired(
  handler: (payload: InteractionRequiredPayload) => void,
): Promise<() => void> {
  return listen<InteractionRequiredPayload>(TAURI_EVENTS.INTERACTION_REQUIRED, createInstrumentedEventHandler(TAURI_EVENTS.INTERACTION_REQUIRED, (event) => {
    handler(event.payload)
  }))
}

export function onInteractionResolved(
  handler: (payload: InteractionResolvedPayload) => void,
): Promise<() => void> {
  return listen<InteractionResolvedPayload>(TAURI_EVENTS.INTERACTION_RESOLVED, createInstrumentedEventHandler(TAURI_EVENTS.INTERACTION_RESOLVED, (event) => {
    handler(event.payload)
  }))
}

export function onTurnCompleted(
  handler: (payload: TurnCompletedPayload) => void,
): Promise<() => void> {
  return listen<TurnCompletedPayload>(TAURI_EVENTS.TURN_COMPLETED, createInstrumentedEventHandler(TAURI_EVENTS.TURN_COMPLETED, (event) => {
    handler(event.payload)
  }))
}

export function onTurnStage(
  handler: (payload: TurnStagePayload) => void,
): Promise<() => void> {
  return listen<TurnStagePayload>(TAURI_EVENTS.TURN_STAGE, createInstrumentedEventHandler(TAURI_EVENTS.TURN_STAGE, (event) => {
    handler(event.payload)
  }))
}

export function onTurnHeartbeat(
  handler: (payload: TurnHeartbeatPayload) => void,
): Promise<() => void> {
  return listen<TurnHeartbeatPayload>(TAURI_EVENTS.TURN_HEARTBEAT, createInstrumentedEventHandler(TAURI_EVENTS.TURN_HEARTBEAT, (event) => {
    handler(event.payload)
  }))
}

// ── LTR: lead:has-pending-messages ──────────────────────────────────────────

export interface LeadHasPendingMessagesPayload {
  conversationId: string
  agentId?: string
}

/**
 * Listen for `lead:has-pending-messages` events emitted by the backend when
 * the Lead finishes a turn but has pending Teammate messages in its inbox
 * (LTR Path A).  The frontend currently does not act on this event — the
 * backend's SessionRuntime handles the continuation automatically via Path C
 * wake.  This listener exists solely to record a diagnostic point so the
 * event can be correlated in metrics.jsonl.
 */
export function onLeadHasPendingMessages(
  handler?: (payload: LeadHasPendingMessagesPayload) => void,
): Promise<() => void> {
  return listen<LeadHasPendingMessagesPayload>(
    TAURI_EVENTS.LEAD_HAS_PENDING_MESSAGES,
    createInstrumentedEventHandler(TAURI_EVENTS.LEAD_HAS_PENDING_MESSAGES, (event) => {
      recordDiagnostic({
        event: 'event.received',
        conversationId: event.payload.conversationId,
        payload: { eventName: TAURI_EVENTS.LEAD_HAS_PENDING_MESSAGES, agentId: event.payload.agentId },
      })
      if (handler) handler(event.payload)
    }),
  )
}

export function onDiagnosticsEvent(
  handler: (payload: DiagnosticsEventPayload) => void,
): Promise<() => void> {
  return listen<DiagnosticsEventPayload>(TAURI_EVENTS.DIAGNOSTICS_EVENT, (event) => {
    handler(event.payload)
  })
}

export interface AuthExpiredPayload {
  message: string
}

/**
 * Listen for auth:expired events (emitted when cloud session expires and
 * the backend clears auth state). The frontend should clear its auth store
 * and prompt the user to re-login.
 */
export function onAuthExpired(
  handler: (payload: AuthExpiredPayload) => void,
): Promise<() => void> {
  return listen<AuthExpiredPayload>(TAURI_EVENTS.AUTH_EXPIRED, createInstrumentedEventHandler(TAURI_EVENTS.AUTH_EXPIRED, (event) => {
    handler(event.payload)
  }))
}

// ---------------------------------------------------------------------------
// Skill Management Commands
// ---------------------------------------------------------------------------

/** Info about a custom (user-installed) skill. */
export interface CustomSkillInfo {
  id: string
  name: string
  description: string
  path: string
  enabled: boolean
  version?: string | null
}

/** List all custom skills installed by the user. */
export function listCustomSkills(): Promise<CustomSkillInfo[]> {
  return invoke<CustomSkillInfo[]>('list_custom_skills')
}

/** Install a custom skill from a local directory. */
export async function installCustomSkill(
  sourcePath: string,
  force: boolean = false,
): Promise<string> {
  return invoke<string>('install_custom_skill', { sourcePath, force })
}

/** Uninstall a custom skill by ID. */
export function uninstallCustomSkill(skillId: string): Promise<string> {
  return invoke<string>('uninstall_custom_skill', { skillId })
}

/** Create a new skill template directory with scaffolding files. */
export async function initSkillTemplate(targetDir: string, skillId: string, skillName: string): Promise<string> {
  return invoke<string>('init_skill_template', { targetDir, skillId, skillName })
}

/** Export a skill's SKILL.md to a destination file path. */
export async function packSkill(skillDir: string, destPath: string): Promise<string> {
  return invoke<string>('pack_skill', { skillDir, destPath })
}

/** Reload a custom skill from disk (dev mode hot-reload). */
export function reloadSkill(skillPath: string): Promise<string> {
  return invoke<string>('reload_skill', { skillPath })
}

/** Start watching a skill directory for file changes (dev mode). */
export function startSkillWatch(skillPath: string): Promise<string> {
  return invoke<string>('start_skill_watch', { skillPath })
}

/** Stop watching the skill directory (dev mode). */
export function stopSkillWatch(): Promise<string> {
  return invoke<string>('stop_skill_watch')
}

// ---------------------------------------------------------------------------
// Marketplace Commands
// ---------------------------------------------------------------------------

/** A skill package from the cloud marketplace. */
export interface MarketplaceSkillItem {
  id: number
  pluginId: string
  name: string
  description: string
  category: string
  icon: string
  version: string
  scope: string
  status: string
  downloads: number
  featured: boolean
  packageSize: number
  tenantName: string
  createdAt: string
}

/** Paginated marketplace response. */
export interface MarketplaceResponse {
  items: MarketplaceSkillItem[]
  total: number
  page: number
  size: number
}

/** List skill packages from the cloud marketplace. */
export function listMarketplaceSkills(
  page: number,
  size: number,
  category?: string,
  search?: string,
): Promise<MarketplaceResponse> {
  return invoke<MarketplaceResponse>('list_marketplace_skills', {
    page,
    size,
    category: category || null,
    search: search || null,
  })
}

/** Download and install a skill package from the marketplace. */
export function installMarketplaceSkill(
  packageId: number,
  pluginId: string,
): Promise<string> {
  return invoke<string>('install_marketplace_skill', {
    packageId,
    pluginId,
  })
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// MCP Server Commands
// ---------------------------------------------------------------------------

export interface McpServerConfig {
  name: string
  transportType: string
  endpoint: string
  envVars?: Record<string, string>
}

export interface McpServerStatus {
  name: string
  transportType: string
  endpoint: string
  state: 'configured' | 'connecting' | 'ready' | 'failed' | 'disconnected'
  registeredToolIds: string[]
  lastError: string | null
}

export function listMcpServers(): Promise<McpServerStatus[]> {
  return invoke<McpServerStatus[]>('list_mcp_servers')
}

export function addMcpServer(config: McpServerConfig): Promise<void> {
  return invoke<void>('add_mcp_server', { config })
}

export function removeMcpServer(serverName: string): Promise<void> {
  return invoke<void>('remove_mcp_server', { serverName })
}

export function connectMcpServer(serverName: string): Promise<McpServerStatus> {
  return invoke<McpServerStatus>('connect_mcp_server', { serverName })
}

export function disconnectMcpServer(serverName: string): Promise<void> {
  return invoke<void>('disconnect_mcp_server', { serverName })
}

// ---------------------------------------------------------------------------
// Project Memory Commands
// ---------------------------------------------------------------------------

export interface ProjectMemoryEntryDraft {
  memoryType: 'user_preference' | 'project_constraint' | 'reference_info' | 'feedback'
  name: string
  description: string
  content: string
  source?: string
}

export function saveProjectMemory(
  workspacePath: string,
  memory: ProjectMemoryEntryDraft,
): Promise<string> {
  return invoke<string>('save_project_memory', { workspacePath, memory })
}

export function distillProjectMemory(workspacePath: string): Promise<number> {
  return invoke<number>('distill_project_memory', { workspacePath })
}

// ---------------------------------------------------------------------------
// Runtime Commands
// ---------------------------------------------------------------------------

export interface RuntimeToolHealth {
  version: string
  path: string
}

export interface RuntimeHealth {
  bundleVersion: string
  node: RuntimeToolHealth | null
  npm: RuntimeToolHealth | null
  npx: RuntimeToolHealth | null
  python: RuntimeToolHealth | null
  uv: RuntimeToolHealth | null
  uvx: RuntimeToolHealth | null
}


export type RuntimeOperationKind = 'ensure' | 'reinstall'
export type RuntimeOperationPhase =
  | 'manifest'
  | 'download'
  | 'checksum'
  | 'extract'
  | 'smokeTest'
  | 'promote'
  | 'health'
export type RuntimeOperationStatus = 'started' | 'progress' | 'retrying' | 'completed' | 'failed' | 'cancelled'

export interface RuntimeOperationProgressPayload {
  operationId: string
  kind: RuntimeOperationKind
  phase: RuntimeOperationPhase
  downloadedBytes?: number | null
  totalBytes?: number | null
  percent?: number | null
  attempt: number
  maxAttempts: number
  resumed: boolean
  status: RuntimeOperationStatus
  message?: string | null
  error?: string | null
}

export interface RuntimeCleanupResult {
  removedVersions: string[]
  keptVersions: string[]
}

export const RUNTIME_OPERATION_PROGRESS = 'runtime:operation-progress'

export async function onRuntimeOperationProgress(
  handler: (payload: RuntimeOperationProgressPayload) => void,
): Promise<() => void> {
  const { listen } = await import('@tauri-apps/api/event')
  return listen<RuntimeOperationProgressPayload>(RUNTIME_OPERATION_PROGRESS, (event) => {
    handler(event.payload)
  })
}

export function getRuntimeHealth(): Promise<RuntimeHealth> {
  return invoke<RuntimeHealth>('runtime_get_health')
}

export function ensureRuntime(): Promise<RuntimeHealth> {
  return invoke<RuntimeHealth>('runtime_ensure')
}

export function reinstallRuntime(): Promise<RuntimeHealth> {
  return invoke<RuntimeHealth>('runtime_reinstall')
}


export function cancelRuntimeOperation(operationId: string): Promise<boolean> {
  return invoke<boolean>('runtime_cancel_operation', { operationId })
}

export function cleanupOldRuntimeVersions(keepVersions: number): Promise<RuntimeCleanupResult> {
  return invoke<RuntimeCleanupResult>('runtime_cleanup_old_versions', { keepVersions })
}

export interface RuntimeDiagnostics {
  activeResolver: 'bundled' | 'installed' | 'none'
  bundledVersion: string | null
  installedVersion: string | null
  node: string
  python: string
  uv: string
}

export function runtimeDiagnostics(): Promise<RuntimeDiagnostics> {
  return invoke<RuntimeDiagnostics>('runtime_diagnostics')
}

// ---------------------------------------------------------------------------
// DingTalk Commands
// ---------------------------------------------------------------------------

export interface DingtalkStatusInfo {
  connected: boolean
  userName: string | null
  corpName: string | null
}

/** Start DingTalk OAuth login (opens system browser). */
export function dingtalkLogin(): Promise<DingtalkStatusInfo> {
  return invoke<DingtalkStatusInfo>('dingtalk_login')
}

/** Disconnect from DingTalk. */
export function dingtalkLogout(): Promise<void> {
  return invoke<void>('dingtalk_logout')
}

/** Get current DingTalk connection status (no network call). */
export function dingtalkStatus(): Promise<DingtalkStatusInfo> {
  return invoke<DingtalkStatusInfo>('dingtalk_status')
}

/** Refresh DingTalk auth status from dws (network call). */
export function dingtalkRefreshStatus(): Promise<DingtalkStatusInfo> {
  return invoke<DingtalkStatusInfo>('dingtalk_refresh_status')
}

export interface SyncBuiltinSkillsResult {
  installed: string[]
  skipped: string[]
}

export async function syncBuiltinSkills(): Promise<SyncBuiltinSkillsResult> {
  return invoke<SyncBuiltinSkillsResult>('sync_builtin_skills')
}

// ---------------------------------------------------------------------------
// Employee Commands
// ---------------------------------------------------------------------------

export interface EmployeeRecord {
  id: string
  name: string
  role: string
  description: string
  avatar: string
  templateId: string | null
  toolWhitelist: string[]
  cron: string | null
  timezone: string
  lifecycle: 'active' | 'paused' | 'archived'
  cronEnabled: boolean
  resourceConfig: Record<string, unknown>
  systemPromptExtra: string | null
  defaultSkillId: string | null
  /**
   * Pointer to the template snapshot this instance was hired from. Present
   * on records hired/refreshed after PR3 (2026-05-10); older records have
   * `templateRef === null` until the backend stamps them on next read.
   */
  templateRef: EmployeeTemplateRef | null
  createdAt: string
  updatedAt: string
  lastRunAt: string | null
  nextRunAt: string | null
}

/** Identifies which template snapshot an employee instance was hired from. */
export interface EmployeeTemplateRef {
  templateId: string
  version: string
  sha256: string
  /** `"bootstrap"` (embedded) or `"ops:<url>"` (downloaded). */
  source: string
}

/**
 * On-disk shape of the template snapshot at
 * `<instance>/template/template.json`. Mirrors the OPS-side
 * `employee_templates` row. Returned by `employeeTemplateCatalog()`.
 */
export interface EmployeeTemplateSnapshot {
  templateId: string
  version: string
  name: string
  avatar: string
  role: string
  description: string
  badge: string
  systemPromptExtra: string
  toolWhitelist: string[]
  cron: string
  defaultSkillId: string
  requiresDingtalk: boolean
  requiresAttachment: { accept: string; min: number; max: number } | null
  resourceConfigSchema: Record<string, unknown> | null
  resourceConfigUI: Record<string, unknown> | null
}

export interface CreateEmployeeRequest {
  name: string
  role: string
  description: string
  avatar: string
  templateId?: string
  toolWhitelist?: string[]
  cron?: string
  timezone?: string
  lifecycle?: 'active' | 'archived'
  cronEnabled?: boolean
  resourceConfig?: Record<string, unknown>
  systemPromptExtra?: string
  defaultSkillId?: string | null
}

export interface UpdateEmployeeRequest {
  name?: string
  role?: string
  description?: string
  avatar?: string
  toolWhitelist?: string[]
  /** Pass null explicitly to clear cron; omit to leave unchanged. */
  cron?: string | null
  timezone?: string
  lifecycle?: 'active' | 'archived'
  cronEnabled?: boolean
  resourceConfig?: Record<string, unknown>
  systemPromptExtra?: string | null
  defaultSkillId?: string | null
}

export function employeeList(): Promise<EmployeeRecord[]> {
  return invoke<EmployeeRecord[]>('employee_list')
}

export function employeeGet(id: string): Promise<EmployeeRecord> {
  return invoke<EmployeeRecord>('employee_get', { id })
}

export function employeeCreate(request: CreateEmployeeRequest): Promise<EmployeeRecord> {
  return invoke<EmployeeRecord>('employee_create', { request })
}

export function employeeUpdate(id: string, request: UpdateEmployeeRequest): Promise<EmployeeRecord> {
  return invoke<EmployeeRecord>('employee_update', { id, request })
}

export function employeeDelete(id: string): Promise<boolean> {
  return invoke<boolean>('employee_delete', { id })
}

// ─── PR-12: manual template upgrade ──────────────────────────────────────────

export interface TemplateUpgradeCheck {
  currentVersion: string | null
  latestVersion: string | null
  hasUpgrade: boolean
  changedFields: string[]
}

/**
 * Check whether the employee's frozen template snapshot has a newer
 * version available locally (in bootstrap or the global cache).
 * Returns metadata for the drawer to surface the "升级模板" button.
 */
export function employeeTemplateCheckUpgrade(id: string): Promise<TemplateUpgradeCheck> {
  return invoke<TemplateUpgradeCheck>('employee_template_check_upgrade', { id })
}

/**
 * Rewrite the employee's snapshot to the latest available version and
 * rebuild derived record fields (role / description / avatar /
 * systemPromptExtra / defaultSkillId / skillIds). Preserves user-tuned
 * fields (name / cron / cronEnabled / resourceConfig / lifecycle).
 */
export function employeeTemplateUpgrade(id: string): Promise<EmployeeRecord> {
  return invoke<EmployeeRecord>('employee_template_upgrade', { id })
}

export function employeeTrigger(
  id: string,
  promptOverride?: string,
  attachments?: ChatAttachmentPayload[],
): Promise<string> {
  return invoke<string>('employee_trigger', {
    id,
    promptOverride: promptOverride ?? null,
    attachments: attachments ?? [],
  })
}

export interface EmployeeActiveRunInfo {
  employeeId: string
  conversationId: string
  startedAt: string
  triggerKind: 'on_demand' | 'cron'
}

/**
 * Stop an employee's currently running dispatch (delegates to stop_streaming
 * via the conversation_id tracked in EmployeeActiveRuns). Returns true if a
 * run was found and cancellation was requested, false if no active run.
 */
export function employeeStopRun(id: string): Promise<boolean> {
  return invoke<boolean>('employee_stop_run', { id })
}

/**
 * Returns the live ActiveRun info for an employee, or null if none.
 * Polled by useEmployees to drive UI state.
 */
export function employeeActiveRun(id: string): Promise<EmployeeActiveRunInfo | null> {
  return invoke<EmployeeActiveRunInfo | null>('employee_active_run', { id })
}

/**
 * Returns the catalog of templates the new-hire wizard should display.
 *
 * Sources merged in the backend (last write wins on `template_id`, by
 * version string):
 *   1. Embedded bootstrap registry (always available)
 *   2. `~/.renlijia/employee-templates-cache/` (downloaded via
 *      `employeeTemplateRefresh()`)
 *
 * Never hits the network. Call `employeeTemplateRefresh()` to update the
 * cache from lotus ops-portal.
 */
export function employeeTemplateCatalog(): Promise<EmployeeTemplateSnapshot[]> {
  return invoke<EmployeeTemplateSnapshot[]>('employee_template_catalog')
}

/**
 * Sync the local template cache from lotus ops-portal.
 *
 * Fetches `/api/public/employee-templates` (latest published per
 * `template_id`, `tenant_scope=global`), then for each entry whose version
 * is newer than the cache (or missing) downloads the snapshot, verifies
 * sha256 against the manifest, and writes it to disk.
 *
 * Returns the number of templates downloaded this call. Failures on
 * individual templates are logged and skipped — a partial refresh is
 * better than a hard failure.
 */
export function employeeTemplateRefresh(): Promise<number> {
  return invoke<number>('employee_template_refresh')
}

// ---------------------------------------------------------------------------
// Knowledge Indexing Commands
// ---------------------------------------------------------------------------

export interface PendingKnowledgeSource {
  path: string
  originalName: string
  size: number
}

export async function employeeIndexKnowledgeAsync(
  employeeId: string,
  sources: PendingKnowledgeSource[],
): Promise<void> {
  await invoke('employee_index_knowledge_async', {
    args: {
      employee_id: employeeId,
      sources: sources.map((s) => [s.path, s.originalName] as [string, string]),
    },
  })
}

// ---------------------------------------------------------------------------
// Inbox Commands
// ---------------------------------------------------------------------------

export type InboxKind = 'report' | 'signal' | 'running' | 'error'

export interface InboxEntry {
  id: string
  employeeId: string
  kind: InboxKind
  title: string
  summary: string | null
  reportPath: string | null
  conversationId: string | null
  read: boolean
  catchupInfo: string | null
  createdAt: string
}

export function inboxList(employeeId?: string, limit?: number): Promise<InboxEntry[]> {
  return invoke<InboxEntry[]>('inbox_list', {
    employeeId: employeeId ?? null,
    limit: limit ?? null,
  })
}

export function inboxMarkRead(employeeId: string, entryId: string): Promise<boolean> {
  return invoke<boolean>('inbox_mark_read', { employeeId, entryId })
}

export function inboxMarkAllRead(employeeId: string): Promise<number> {
  return invoke<number>('inbox_mark_all_read', { employeeId })
}

export function inboxUnreadCount(employeeId?: string): Promise<number> {
  return invoke<number>('inbox_unread_count', { employeeId: employeeId ?? null })
}

// ---------------------------------------------------------------------------
// Pending Queue IPC + Listeners
// ---------------------------------------------------------------------------

export async function pendingSnapshotForSession(sessionId: string): Promise<PendingItem[]> {
  return invoke<PendingItem[]>('pending_snapshot_for_session', { sessionId })
}

export async function pendingRemoveItem(sessionId: string, itemId: string): Promise<boolean> {
  return invoke<boolean>('pending_remove_item', { sessionId, itemId })
}

// ── Turn-stage persistence (spec docs/superpowers/specs/2026-05-17-turn-stages.md §5) ─

/** Read the active turn-stage snapshot for a conversation.  Returns null
 *  when no turn is mid-flight (file does not exist). */
export async function getActiveTurnStage(
  conversationId: string,
): Promise<PersistedTurnStage | null> {
  const result = await invoke<PersistedTurnStage | null>('get_active_turn_stage', {
    conversationId,
  })
  return result
}

/** Read the crash-recovery sentinel for a conversation.  Returns null when
 *  the previous process didn't die mid-turn for this conversation. */
export async function getInterruptedTurn(
  conversationId: string,
): Promise<InterruptedTurnRecord | null> {
  const result = await invoke<InterruptedTurnRecord | null>('get_interrupted_turn', {
    conversationId,
  })
  return result
}

/** Delete the interrupted-turn sentinel after the user dismisses (or
 *  resends) — so the banner doesn't keep showing on subsequent opens. */
export async function dismissInterruptedTurn(conversationId: string): Promise<void> {
  await invoke<void>('dismiss_interrupted_turn', { conversationId })
}

export function listenPendingSnapshot(
  handler: (payload: PendingSnapshotPayload) => void,
): Promise<() => void> {
  return listen<PendingSnapshotPayload>(TAURI_EVENTS.PENDING_SNAPSHOT, (event) =>
    handler(event.payload),
  )
}

export function listenPendingQueued(
  handler: (payload: PendingQueuedPayload) => void,
): Promise<() => void> {
  return listen<PendingQueuedPayload>(TAURI_EVENTS.PENDING_QUEUED, (event) =>
    handler(event.payload),
  )
}

export function listenPendingDrained(
  handler: (payload: PendingDrainedPayload) => void,
): Promise<() => void> {
  return listen<PendingDrainedPayload>(TAURI_EVENTS.PENDING_DRAINED, (event) =>
    handler(event.payload),
  )
}

export function listenPendingRemoved(
  handler: (payload: PendingRemovedPayload) => void,
): Promise<() => void> {
  return listen<PendingRemovedPayload>(TAURI_EVENTS.PENDING_REMOVED, (event) =>
    handler(event.payload),
  )
}
