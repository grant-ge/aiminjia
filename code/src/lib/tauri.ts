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

import type { Message } from '@/types/message'
import type { Settings } from '@/types/settings'

// ---------------------------------------------------------------------------
// Tauri Event Constants
// ---------------------------------------------------------------------------

export const TAURI_EVENTS = {
  STREAMING_DELTA: 'streaming:delta',
  STREAMING_DONE: 'streaming:done',
  STREAMING_ERROR: 'streaming:error',
  MESSAGE_UPDATED: 'message:updated',
  ANALYSIS_STEP_CHANGED: 'analysis:step-changed',
  FILE_PARSED: 'file:parsed',
  FILE_GENERATED: 'file:generated',
  NOTIFICATION: 'notification',
  TOOL_EXECUTING: 'tool:executing',
  TOOL_COMPLETED: 'tool:completed',
  CONVERSATION_TITLE_UPDATED: 'conversation:title-updated',
  AGENT_IDLE: 'agent:idle',
  AGENT_PHASE: 'agent:phase',
  STREAMING_STEP_RESET: 'streaming:step-reset',
  AUTH_EXPIRED: 'auth:expired',
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
  messageId: string
}

export interface StreamingErrorPayload {
  conversationId: string
  error: string
}

export interface AgentIdlePayload {
  conversationId: string
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
}

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
export function sendMessage(conversationId: string, content: string, fileIds?: string[]): Promise<void> {
  return invoke<void>('send_message', {
    conversationId,
    content,
    fileIds: fileIds ?? [],
  })
}

/**
 * Abort the streaming response for a specific conversation.
 *
 * @param conversationId - The conversation whose streaming should be stopped
 */
export function stopStreaming(conversationId: string): Promise<void> {
  return invoke<void>('stop_streaming', { conversationId })
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
export function exportConversation(
  conversationId: string,
  format: 'pdf' | 'html',
): Promise<{ fileId: string; fileName: string; storedPath: string; fileSize: number }> {
  return invoke<{ fileId: string; fileName: string; storedPath: string; fileSize: number }>('export_conversation', {
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
  description: string
  source: string
  hasWorkflow: boolean
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
  tenant: { id: number; name: string; balance: string } | null
  models: CloudModel[]
}

/** Cloud model info from /v1/models. */
export interface CloudModel {
  id: string
  name: string
  modelType: string
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
  handler: (payload: ToolCompletedPayload) => void,
): Promise<() => void> {
  return listen<ToolCompletedPayload>(TAURI_EVENTS.TOOL_COMPLETED, (event) => {
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
