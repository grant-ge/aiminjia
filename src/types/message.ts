/**
 * Message types for the chat system.
 * Based on tech-architecture.md §3.3
 */

export type MessageRole = 'user' | 'assistant' | 'system' | 'tool'

export interface Message {
  id: string
  conversationId: string
  role: MessageRole
  createdAt: string
  content: MessageContent
  /** Sender information (only present for user messages) */
  sender?: MessageSender
  /** assistant 消息专用：工具调用入参列表，来自磁盘 toolCalls 字段 */
  toolCalls?: AssistantToolCall[]
  /** tool 消息关联的运行 ID（实时事件携带，历史消息可能没有） */
  runId?: string
  /** tool 消息专用：工具执行结果 */
  toolResult?: ToolResultContent
  /** 后端 echo 回的 optimistic id，仅出现在 message:updated role=user 时 */
  clientMessageId?: string
}

/** Information about the message sender (for user messages) */
export interface MessageSender {
  /** Display name of the sender */
  name: string
  /** Whether the sender was logged in when sending the message */
  isLoggedIn: boolean
}

export interface Conversation {
  id: string
  title: string
  createdAt: string
  updatedAt: string
  isArchived: boolean
  workspaceName?: string
  /**
   * Conversation source kind. Mirrored from `ConversationIndexEntry.kind`
   * in `index.json` so the sidebar can render groupings without fan-out.
   */
  kind?: 'user' | 'employee' | 'expertTeam' | 'im'
  /**
   * Human-readable source label. LLM 改 title 时本字段不变；侧边栏用它显示稳定的来源标签。
   */
  sourceLabel?: string
}

/**
 * MessageContent supports multiple rich content types mixed together.
 *
 * Rendering order (resolved ambiguity):
 * text → codeBlocks → codeResults → tables → metrics →
 * anomalies → insights → rootCauses → subagentEnvelope
 */
export interface SkillCommandBreadcrumb {
  id: string
  label: string
  command: string
}

export interface MessageContent {
  text?: string
  commandText?: string
  skillCommand?: SkillCommandBreadcrumb
  files?: FileAttachment[]
  codeBlocks?: CodeBlock[]
  codeResults?: CodeResult[]
  tables?: DataTable[]
  metrics?: MetricCard[]
  anomalies?: AnomalyItem[]
  insights?: InsightBlock[]
  rootCauses?: RootCauseBlock[]
  progress?: ProgressState
  generatedFiles?: GeneratedFile[]
  subagentEnvelope?: SubAgentEnvelopeContent
}

/** The fixed rendering order for MessageContent fields */
export const MESSAGE_CONTENT_RENDER_ORDER: (keyof MessageContent)[] = [
  'text',
  'codeBlocks',
  'codeResults',
  'tables',
  'metrics',
  'anomalies',
  'insights',
  'rootCauses',
  'subagentEnvelope',
]

// --- File Attachment ---

export interface FileAttachment {
  id: string
  fileName: string
  filePath?: string
  kind?: 'file' | 'folder' | 'image'
  fileSize: number
  fileType: 'excel' | 'word' | 'pdf' | 'csv' | 'json' | 'folder' | 'image'
  status: 'uploading' | 'uploaded' | 'parsing' | 'parsed' | 'error'
  mimeType?: string
  errorMessage?: string
}

// --- Code Block ---

export interface CodeBlock {
  id: string
  language: string
  code: string
  purpose?: string
  status: 'pending' | 'running' | 'success' | 'error'
}

export interface CodeResult {
  id: string
  codeBlockId: string
  output: string
  isError: boolean
}

// --- Data Table ---

export interface DataTable {
  id: string
  title?: string
  badge?: { text: string; variant: 'green' | 'orange' | 'red' | 'blue' }
  columns: TableColumn[]
  rows: TableRow[]
}

export interface TableColumn {
  key: string
  label: string
  align?: 'left' | 'center' | 'right'
}

export type TableRow = Record<string, TableCellValue>

export interface TableCellValue {
  text: string
  color?: 'green' | 'orange' | 'red' | 'blue' | 'accent'
  bold?: boolean
}

// --- Metric Card ---

export interface MetricCard {
  id: string
  label: string
  value: string
  subtitle?: string
  state: 'good' | 'warn' | 'bad' | 'neutral'
}

// --- Anomaly List ---

export interface AnomalyItem {
  id: string
  priority: 'high' | 'medium' | 'low'
  title: string
  description: string
}

// --- Insight Block ---

export interface InsightBlock {
  id: string
  title: string
  content: string
}

// --- Root Cause Block ---

export interface RootCauseBlock {
  id: string
  title: string
  items: RootCauseItem[]
}

export interface RootCauseItem {
  count: number
  label: string
  detail: string
  action: string
}

// --- Progress State ---

export interface ProgressState {
  title: string
  currentStep: number
  steps: ProgressStep[]
}

export interface ProgressStep {
  label: string
  status: 'done' | 'active' | 'pending'
}

// --- Generated File ---

export type GeneratedFileType =
  | 'excel'
  | 'xlsx'
  | 'html'
  | 'pdf'
  | 'csv'
  | 'json'
  | 'png'
  | 'jpg'
  | 'jpeg'
  | 'py'
  | 'markdown'
  | 'md'
  | 'text'
  | 'txt'
  | string

export interface GeneratedFile {
  id: string
  title?: string
  fileName: string
  filePath: string
  fileType?: GeneratedFileType
  fileSize: number
  category: 'report' | 'chart' | 'data' | 'analysis' | 'script' | 'temp' | string
  version: number
  isLatest: boolean
  supersededBy?: string
  createdAt: string
  createdByStep?: number
  description: string
  actions?: FileAction[]
  isDegraded?: boolean
  degradationNotice?: string | null
  requestedFormat?: string
}

export interface FileAction {
  type: 'open' | 'preview' | 'download' | 'delete' | 'reveal'
  label: string
  enabled: boolean
}

export interface SubAgentEnvelopeContent {
  schemaVersion: number
  output: string
  iterationsUsed: number
  generatedFiles: string[]
  transcriptRef?: string
}

export interface SubAgentTranscriptEntry {
  role: string
  content: string
  toolName?: string
}

// --- Tool Call ---

/** assistant 消息里的工具调用入参（来自磁盘 toolCalls 字段） */
export interface AssistantToolCall {
  id: string          // tool_call_id，与 tool 消息的 toolCallId 对应
  name: string        // 工具名，如 "browse_navigate"
  arguments: unknown  // 完整入参 JSON 对象
}

/** role: 'tool' 消息的工具结果内容 */
export interface ToolResultContent {
  toolCallId: string   // 与 AssistantToolCall.id 对应
  name: string         // 工具名
  content: string      // 完整工具输出文本
  isError: boolean     // 是否执行失败
  durationMs?: number  // 执行耗时（ms）
}
