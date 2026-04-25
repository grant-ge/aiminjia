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

import type { Message, SubAgentTranscriptEntry } from '@/types/message'
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
  ANALYSIS_STEP_CHANGED: 'analysis:step-changed',
  STOP_PREVENTED_CONTINUATION: 'stop:prevented-continuation',
  /** @deprecated 后端不发送此事件 */
  FILE_PARSED: 'file:parsed',
  FILE_GENERATED: 'file:generated',
  NOTIFICATION: 'notification',
  TOOL_EXECUTING: 'tool:executing',
  TOOL_COMPLETED: 'tool:completed',
  CONVERSATION_TITLE_UPDATED: 'conversation:title-updated',
  AGENT_IDLE: 'agent:idle',
  AGENT_PHASE: 'agent:phase',
  TASK_STATUS_CHANGED: 'task:status-changed',
  STREAMING_STEP_RESET: 'streaming:step-reset',
  AUTH_EXPIRED: 'auth:expired',
  SKILL_FILE_CHANGED: 'skill-file-changed',
  PERMISSION_ASK: 'permission:ask',
  INTERACTION_REQUIRED: 'interaction:required',
  INTERACTION_RESOLVED: 'interaction:resolved',
  TURN_COMPLETED: 'turn:completed',
  DIAGNOSTICS_EVENT: 'diagnostics:event',
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

export interface AgentPhasePayload {
  conversationId: string
  iteration: number
  phase: 'think' | 'act' | 'observe'
  prevPhaseDurationMs: number
  toolNames: string[]
  maxIterations: number
}

export interface StreamingStepResetPayload {
  conversationId: string
  step: number
}

export interface ToolExecutingPayload {
  conversationId: string
  toolName: string
  toolId: string
  purpose?: string
  input?: unknown  // 完整入参 JSON 对象
}

/** @deprecated tool:completed 现在直接推完整 Message，保留此类型仅供旧引用过渡 */
export interface ToolCompletedPayload {
  conversationId: string
  toolName: string
  toolId: string
  success: boolean
  summary?: string
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
  totalCostUsd?: number | null
  permissionDenialCount: number
  iterations?: number
  reason?: string
  message?: string
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
 * @param fileIds - Optional list of uploaded file IDs to attach
 */
export function sendMessage(
  conversationId: string,
  content: string,
  fileIds?: string[],
  agentName?: string | null,
  clientMessageId?: string,
  selectedSkillId?: string | null,
  selectedSkillLabel?: string | null,
): Promise<void> {
  return invoke<void>('send_message', {
    conversationId,
    content,
    fileIds: fileIds ?? [],
    agentName: agentName ?? null,
    clientMessageId: clientMessageId ?? null,
    selectedSkillId: selectedSkillId ?? null,
    selectedSkillLabel: selectedSkillLabel ?? null,
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

export type ScheduleStatus = 'enabled' | 'disabled'

export interface ScheduleRecord {
  id: string
  title: string
  prompt: string
  cron: string
  humanSchedule: string
  status: ScheduleStatus
  nextRunAt?: string | null
  timezone: string
  createdAt: string
  updatedAt: string
}

export interface CreateScheduleRequest {
  title: string
  prompt: string
  cron: string
  timezone?: string
  enabled?: boolean
}

export function listSchedules(): Promise<ScheduleRecord[]> {
  return invoke<ScheduleRecord[]>('list_schedules')
}

export function createSchedule(request: CreateScheduleRequest): Promise<ScheduleRecord> {
  return invoke<ScheduleRecord>('create_schedule', { request })
}

export function deleteSchedule(id: string): Promise<boolean> {
  return invoke<boolean>('delete_schedule', { id })
}

export function getSubagentTranscript(
  transcriptRef: string,
): Promise<SubAgentTranscriptEntry[]> {
  return invoke<SubAgentTranscriptEntry[]>('get_subagent_transcript', {
    transcriptRef,
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

export function getConversationModelOverride(conversationId: string): Promise<string | null> {
  return invoke<string | null>('get_conversation_model_override', {
    conversationId,
  })
}

export function setConversationModelOverride(conversationId: string, model: string | null): Promise<void> {
  return invoke<void>('set_conversation_model_override', {
    conversationId,
    model,
  })
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

/**
 * Export a conversation to PDF or HTML format.
 *
 * @param conversationId - The conversation to export
 * @param format - Export format: 'pdf' or 'html'
 * @returns File info for the generated export
 */
export interface ExportConversationResult {
  fileId: string
  fileName: string
  storedPath: string
  fileSize: number
  /** Format the user asked for (e.g. "pdf"). */
  requestedFormat: string
  /** Format actually produced (may differ from requested on fallback). */
  actualFormat: string
  /** True when the requested format could not be generated and a fallback was produced. */
  wasFallback: boolean
  /** Human-readable explanation of why the fallback happened. */
  fallbackMessage?: string | null
}

export function exportConversation(
  conversationId: string,
  format: 'pdf' | 'html',
): Promise<ExportConversationResult> {
  return invoke<ExportConversationResult>('export_conversation', {
    conversationId,
    format,
  })
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

/** Branding info from persisted auth (no network, instant). */
export interface BrandingInfo {
  productName?: string
  logoUrl?: string
  accentColor?: string
  primaryColor?: string
  bgColor?: string
  sidebarBgColor?: string
  fontFamily?: string
}

/** Cloud model info from /v1/models. */
export interface CloudModel {
  id: string
  name: string
  modelType: string
}

/** Persona summary for list API */
export interface PersonaSummary {
  id: string
  name: string
  nameEn: string
  icon: string
  description: string
  descriptionEn: string
  builtin: boolean
}

/** Full persona definition */
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

/** Get branding from persisted auth state (no network, instant). */
export function getBranding(): Promise<BrandingInfo> {
  return invoke<BrandingInfo>('get_branding')
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

// ---------------------------------------------------------------------------
// Persona Commands
// ---------------------------------------------------------------------------

/** @deprecated 前端 Skill-First 改版后不再引用，仅为后端兼容保留。 */
export function listPersonas(): Promise<PersonaSummary[]> {
  return invoke<PersonaSummary[]>('list_personas')
}

/** @deprecated 前端 Skill-First 改版后不再引用，仅为后端兼容保留。 */
export function getPersona(id: string): Promise<Persona> {
  return invoke<Persona>('get_persona', { id })
}

/** @deprecated 前端 Skill-First 改版后不再引用，仅为后端兼容保留。 */
export function savePersona(persona: Persona): Promise<void> {
  return invoke<void>('save_persona', { persona })
}

/** @deprecated 前端 Skill-First 改版后不再引用，仅为后端兼容保留。 */
export function deletePersona(id: string): Promise<void> {
  return invoke<void>('delete_persona', { id })
}

/** @deprecated 前端 Skill-First 改版后不再引用，仅为后端兼容保留。 */
export function setActivePersona(id: string): Promise<void> {
  return invoke<void>('set_active_persona', { id })
}

/** @deprecated 前端 Skill-First 改版后不再引用，仅为后端兼容保留。 */
export function getActivePersona(): Promise<Persona> {
  return invoke<Persona>('get_active_persona')
}

/** Export a persona to JSON. */
export function exportPersonas(id: string): Promise<string> {
  return invoke<string>('export_personas', { id })
}

/** Import a persona from JSON. Returns the new persona ID. */
export function importPersonas(json: string): Promise<string> {
  return invoke<string>('import_personas', { json })
}

// ---------------------------------------------------------------------------
// Typed Event Listeners
// ---------------------------------------------------------------------------

/**
 * Listen for streaming text deltas as the AI model generates a response.
 *
 * @param handler - Callback receiving each text delta chunk with conversationId
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onStreamingDelta(
  handler: (payload: StreamingDeltaPayload) => void,
): Promise<() => void> {
  return listen<StreamingDeltaPayload>(TAURI_EVENTS.STREAMING_DELTA, (event) => {
    handler(event.payload)
  })
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
  return listen<StreamingDonePayload>(TAURI_EVENTS.STREAMING_DONE, (event) => {
    handler(event.payload)
  })
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
  return listen<StreamingErrorPayload>(TAURI_EVENTS.STREAMING_ERROR, (event) => {
    handler(event.payload)
  })
}

export function onStreamingRetryReset(
  handler: (payload: StreamingRetryResetPayload) => void,
): Promise<() => void> {
  return listen<StreamingRetryResetPayload>(TAURI_EVENTS.STREAMING_RETRY_RESET, (event) => {
    handler(event.payload)
  })
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
  return listen<Message>(TAURI_EVENTS.MESSAGE_UPDATED, (event) => {
    handler(event.payload)
  })
}

/**
 * Listen for analysis pipeline step transitions.
 *
 * @param handler - Callback receiving the current step index and its status
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onAnalysisStepChanged(
  handler: (payload: { step: number; status: string }) => void,
): Promise<() => void> {
  return listen<{ step: number; status: string }>(TAURI_EVENTS.ANALYSIS_STEP_CHANGED, (event) => {
    handler(event.payload)
  })
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
  return listen<{ level: string; title: string; message: string }>(TAURI_EVENTS.NOTIFICATION, (event) => {
    handler(event.payload)
  })
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
  return listen<ToolExecutingPayload>(TAURI_EVENTS.TOOL_EXECUTING, (event) => {
    handler(event.payload)
  })
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
  return listen<Message>(TAURI_EVENTS.TOOL_COMPLETED, (event) => {
    handler(event.payload)
  })
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
  return listen<{ conversationId: string; title: string }>(TAURI_EVENTS.CONVERSATION_TITLE_UPDATED, (event) => {
    handler(event.payload)
  })
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
  return listen<AgentIdlePayload>(TAURI_EVENTS.AGENT_IDLE, (event) => {
    handler(event.payload)
  })
}

/**
 * Listen for agent TAOR phase transitions (Think → Act → Observe).
 *
 * @param handler - Callback receiving the phase event payload
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onAgentPhase(
  handler: (payload: AgentPhasePayload) => void,
): Promise<() => void> {
  return listen<AgentPhasePayload>(TAURI_EVENTS.AGENT_PHASE, (event) => {
    handler(event.payload)
  })
}

/**
 * Listen for streaming step-reset events during auto-advance between analysis steps.
 *
 * When the backend auto-advances from step N to step N+1, it emits this event
 * so the frontend clears the previous step's streaming content and tool executions
 * while keeping isStreaming=true (the next step's deltas are about to start).
 */
export function onStreamingStepReset(
  handler: (payload: StreamingStepResetPayload) => void,
): Promise<() => void> {
  return listen<StreamingStepResetPayload>(TAURI_EVENTS.STREAMING_STEP_RESET, (event) => {
    handler(event.payload)
  })
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
  return listen<FileGeneratedPayload>(TAURI_EVENTS.FILE_GENERATED, (event) => {
    handler(event.payload)
  })
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
  return listen<TaskStatusChangedPayload>(TAURI_EVENTS.TASK_STATUS_CHANGED, (event) => {
    handler(event.payload)
  })
}

export function onPermissionAsk(
  handler: (payload: PermissionAskPayload) => void,
): Promise<() => void> {
  return listen<PermissionAskPayload>(TAURI_EVENTS.PERMISSION_ASK, (event) => {
    handler(event.payload)
  })
}

export function onInteractionRequired(
  handler: (payload: InteractionRequiredPayload) => void,
): Promise<() => void> {
  return listen<InteractionRequiredPayload>(TAURI_EVENTS.INTERACTION_REQUIRED, (event) => {
    handler(event.payload)
  })
}

export function onInteractionResolved(
  handler: (payload: InteractionResolvedPayload) => void,
): Promise<() => void> {
  return listen<InteractionResolvedPayload>(TAURI_EVENTS.INTERACTION_RESOLVED, (event) => {
    handler(event.payload)
  })
}

export function onTurnCompleted(
  handler: (payload: TurnCompletedPayload) => void,
): Promise<() => void> {
  return listen<TurnCompletedPayload>(TAURI_EVENTS.TURN_COMPLETED, (event) => {
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
  return listen<AuthExpiredPayload>(TAURI_EVENTS.AUTH_EXPIRED, (event) => {
    handler(event.payload)
  })
}

// ---------------------------------------------------------------------------
// Browser Events (WebView → Frontend)
// ---------------------------------------------------------------------------

/** Show the CDP browser window (bring active tab to front). */
export function showBrowseView(): Promise<void> {
  return invoke<void>('show_browse_view')
}

export interface BrowserNavigatingPayload {
  appId?: number
  url: string
}

export interface BrowserPageReadyPayload {
  appId?: number
  url: string
  title: string
}

export interface BrowserClosedPayload {
  appId?: number
}

/** Listen for browser navigating events. */
export function onBrowserNavigating(
  handler: (payload: BrowserNavigatingPayload) => void,
): Promise<() => void> {
  return listen<BrowserNavigatingPayload>('browser:navigating', (event) => handler(event.payload))
}

/** Listen for browser page-ready events. */
export function onBrowserPageReady(
  handler: (payload: BrowserPageReadyPayload) => void,
): Promise<() => void> {
  return listen<BrowserPageReadyPayload>('browser:page-ready', (event) => handler(event.payload))
}

/** Listen for browser closed events. */
export function onBrowserClosed(
  handler: (payload: BrowserClosedPayload) => void,
): Promise<() => void> {
  return listen<BrowserClosedPayload>('browser:closed', (event) => handler(event.payload))
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
}

/** List all custom skills installed by the user. */
export function listCustomSkills(): Promise<CustomSkillInfo[]> {
  return invoke<CustomSkillInfo[]>('list_custom_skills')
}

/** Install a custom skill from a local directory. */
export function installCustomSkill(sourcePath: string): Promise<string> {
  return invoke<string>('install_custom_skill', { sourcePath })
}

/** Uninstall a custom skill by ID. */
export function uninstallCustomSkill(skillId: string): Promise<string> {
  return invoke<string>('uninstall_custom_skill', { skillId })
}

/** Create a new skill template directory with scaffolding files. */
export async function initSkillTemplate(targetDir: string, skillId: string, skillName: string): Promise<string> {
  return invoke<string>('init_skill_template', { targetDir, skillId, skillName })
}

/** Pack a skill directory into a .aijia-skill zip file. */
export async function packSkill(skillDir: string): Promise<string> {
  return invoke<string>('pack_skill', { skillDir })
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
// Skill-Smith — conversational skill creation draft file system (T2)
// ---------------------------------------------------------------------------

/** A file inside a skill-smith draft. Paths are forward-slash normalized. */
export interface DraftFile {
  relativePath: string
  size: number
  modifiedTs: number
}

/** Summary of a skill-smith draft (for resume banner, list UI, etc). */
export interface DraftSummary {
  draftId: string
  createdAt: number
  lastModified: number
  /** One of: "intent" | "manifest" | "workflow" | "prompts" */
  stage: string
  skillName: string | null
}

/** Create a new empty draft. Returns the 12-char hex draft_id. */
export function createSkillDraft(): Promise<string> {
  return invoke<string>('create_skill_draft')
}

/** Write `content` to `{draft}/{relativePath}`, creating parent dirs. */
export function writeSkillDraftFile(
  draftId: string,
  relativePath: string,
  content: string,
): Promise<void> {
  return invoke('write_skill_draft_file', { draftId, relativePath, content })
}

/** Read a file from a draft. Throws if not found. */
export function readSkillDraftFile(
  draftId: string,
  relativePath: string,
): Promise<string> {
  return invoke<string>('read_skill_draft_file', { draftId, relativePath })
}

/** List all files in a draft (recursive). */
export function listSkillDraftFiles(draftId: string): Promise<DraftFile[]> {
  return invoke<DraftFile[]>('list_skill_draft_files', { draftId })
}

/** List all drafts (for the "resume your draft" banner). */
export function listSkillDrafts(): Promise<DraftSummary[]> {
  return invoke<DraftSummary[]>('list_skill_drafts')
}

/** Delete a draft entirely. */
export function discardSkillDraft(draftId: string): Promise<void> {
  return invoke('discard_skill_draft', { draftId })
}

/** Remove drafts untouched for >7 days. Returns count removed. */
export function cleanupExpiredDrafts(): Promise<number> {
  return invoke<number>('cleanup_expired_drafts')
}

/** A structured schema error or warning from validateSkillDraft. */
export interface DraftValidationIssue {
  /** e.g. `"plugin.toml"` / `"prompts/step0.md"` */
  file: string
  /** e.g. `"trigger.keywords[0]"` */
  path: string
  /** e.g. `"length: 2-30"` / `"enum"` / `"reserved"` */
  rule: string
  /** What we actually saw (stringified). */
  actual: string
  /** Chinese primary message, may include English in parentheses. */
  message: string
  /** Optional hint a downstream LLM can use to fix the field. */
  fixHint: string | null
}

/** Validation report returned by `validateSkillDraft`. */
export interface DraftValidationReport {
  valid: boolean
  errors: DraftValidationIssue[]
  warnings: DraftValidationIssue[]
  summary: string
}

/** Validate a draft against the skill schema. Errors block commit; warnings don't. */
export function validateSkillDraft(draftId: string): Promise<DraftValidationReport> {
  return invoke<DraftValidationReport>('validate_skill_draft', { draftId })
}

/** Outcome of a skill-smith commit attempt. */
export interface SkillCommitResult {
  skillId: string
  installedPath: string
  /** `true` iff a skill with the same id already exists AND we did NOT overwrite.
   *  Caller should prompt the user to rename, force-overwrite, or cancel. */
  conflict: boolean
}

/**
 * Commit a validated draft to `~/.renlijia/skills/`. Returns `{ conflict: true }`
 * without making changes if a skill with the same `plugin.id` already exists —
 * caller should then invoke `commitSkillDraftForce` or prompt the user to
 * rename the skill. Errors if validation fails.
 */
export function commitSkillDraft(draftId: string): Promise<SkillCommitResult> {
  return invoke<SkillCommitResult>('commit_skill_draft', { draftId })
}

/** Same as `commitSkillDraft` but overwrites any existing skill with the same id. */
export function commitSkillDraftForce(draftId: string): Promise<SkillCommitResult> {
  return invoke<SkillCommitResult>('commit_skill_draft_force', { draftId })
}

/**
 * Package a validated draft as a `.aijia-skill` zip and place it in `outputDir`.
 * The draft is preserved (unlike commit). Returns the full path to the zip.
 */
export function exportSkillDraft(draftId: string, outputDir: string): Promise<string> {
  return invoke<string>('export_skill_draft', { draftId, outputDir })
}

/** Status of an individual dry-run check. */
export type DryRunCheckStatus = 'pass' | 'fail' | 'skip' | 'warn'

/** One of the six dry-run checks (schema / prompts-reference / prompts-content / python-scripts / knowledge / loadable). */
export interface DryRunCheck {
  /** Stable machine id — safe to match on. */
  name: string
  status: DryRunCheckStatus
  detail: string
}

/** Full dry-run report returned by `dryRunSkillDraft`. */
export interface DryRunReport {
  /** `true` iff no check has status "fail". Warnings and skips don't block. */
  pass: boolean
  checks: DryRunCheck[]
  summary: string
}

/**
 * Static dry-run — verifies the draft's plumbing (schema, prompts, JSON syntax,
 * loadability) without invoking any LLM or running Python. Safe to call as
 * often as needed; no side effects.
 */
export function dryRunSkillDraft(draftId: string): Promise<DryRunReport> {
  return invoke<DryRunReport>('dry_run_skill_draft', { draftId })
}

/**
 * Listen for skill file change events (dev mode hot-reload).
 * The payload is the skill directory path that changed.
 */
export function onSkillFileChanged(
  handler: (skillPath: string) => void,
): Promise<() => void> {
  return listen<string>(TAURI_EVENTS.SKILL_FILE_CHANGED, (event) => {
    handler(event.payload)
  })
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
