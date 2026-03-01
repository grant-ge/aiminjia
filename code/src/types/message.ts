/**
 * Message types for the chat system.
 * Based on tech-architecture.md §3.3
 */

export type MessageRole = 'user' | 'assistant' | 'system'

export interface Message {
  id: string
  conversationId: string
  role: MessageRole
  createdAt: string
  content: MessageContent
}

export interface Conversation {
  id: string
  title: string
  createdAt: string
  updatedAt: string
  isArchived: boolean
}

/**
 * MessageContent supports multiple rich content types mixed together.
 *
 * Rendering order (resolved ambiguity):
 * progress → text → codeBlocks → codeResults → tables → metrics →
 * options → anomalies → insights → rootCauses → generatedFiles →
 * reports → searchSources → execSummary → confirmations
 */
export interface MessageContent {
  text?: string
  files?: FileAttachment[]
  codeBlocks?: CodeBlock[]
  codeResults?: CodeResult[]
  tables?: DataTable[]
  metrics?: MetricCard[]
  options?: OptionGroup[]
  anomalies?: AnomalyItem[]
  insights?: InsightBlock[]
  rootCauses?: RootCauseBlock[]
  confirmations?: ConfirmBlock[]
  progress?: ProgressState
  searchSources?: SearchSource[]
  execSummary?: ExecSummary
  reports?: ReportCard[]
  generatedFiles?: GeneratedFile[]
}

/** The fixed rendering order for MessageContent fields */
export const MESSAGE_CONTENT_RENDER_ORDER: (keyof MessageContent)[] = [
  'progress',
  'text',
  'codeBlocks',
  'codeResults',
  'tables',
  'metrics',
  'options',
  'anomalies',
  'insights',
  'rootCauses',
  'generatedFiles',
  'reports',
  'searchSources',
  'execSummary',
  'confirmations',
]

// --- File Attachment ---

export interface FileAttachment {
  id: string
  fileName: string
  fileSize: number
  fileType: 'excel' | 'word' | 'pdf' | 'csv' | 'json'
  status: 'uploading' | 'uploaded' | 'parsing' | 'parsed' | 'error'
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

// --- Option Cards ---

export interface OptionGroup {
  id: string
  options: OptionCard[]
  selectedId?: string
  columns?: 2 | 3
}

export interface OptionCard {
  id: string
  tag?: string
  tagColor?: string
  title: string
  description: string
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

// --- Confirm Block ---

export interface ConfirmBlock {
  id: string
  title: string
  primaryLabel: string
  primaryAction: string
  secondaryLabel?: string
  secondaryAction?: string
  status: 'pending' | 'confirmed' | 'rejected'
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

// --- Search Source ---

export interface SearchSource {
  id: string
  title: string
  items: SearchSourceItem[]
  warning?: string
}

export interface SearchSourceItem {
  source: string
  snippet: string
  url?: string
}

// --- Exec Summary ---

export interface ExecSummary {
  title: string
  boxes: ExecSummaryBox[]
}

export interface ExecSummaryBox {
  label: string
  value: string
  subtitle?: string
  variant?: 'danger' | 'money' | 'good' | 'neutral'
}

// --- Report Card ---

export interface ReportCard {
  id: string
  title: string
  description: string
  fileType: 'html' | 'excel' | 'pdf'
}

// --- Generated File ---

export interface GeneratedFile {
  id: string
  fileName: string
  filePath: string
  fileType: 'excel' | 'html' | 'pdf' | 'csv' | 'json' | 'png' | 'py'
  fileSize: number
  category: 'report' | 'analysis' | 'script' | 'temp'
  version: number
  isLatest: boolean
  supersededBy?: string
  createdAt: string
  createdByStep?: number
  description: string
  actions: FileAction[]
}

export interface FileAction {
  type: 'open' | 'preview' | 'download' | 'delete' | 'reveal'
  label: string
  enabled: boolean
}
