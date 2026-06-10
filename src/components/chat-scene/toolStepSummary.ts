import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

export type ToolBucket =
  | 'command'
  | 'file_read'
  | 'file_edit'
  | 'interaction'
  | 'search'
  | 'mcp'
  | 'other'

export interface BucketCount {
  key: ToolBucket
  count: number
}

export interface ToolStepSummary {
  buckets: BucketCount[]
  runningCount: number
  errorCount: number
}

export function isUserInteractionTool(name: string): boolean {
  const lower = name.trim().toLowerCase()
  return lower === 'askuserquestion' || lower === 'ask_user_question' || lower === 'request_user_input'
}

export function classifyToolBucket(name: string): ToolBucket {
  const n = name.trim()
  if (isUserInteractionTool(n)) return 'interaction'
  if (n.startsWith('mcp__')) return 'mcp'
  const lower = n.toLowerCase()
  if (lower === 'bash' || lower === 'shell' || lower === 'shell_run') return 'command'
  if (lower === 'read' || lower === 'read_file') return 'file_read'
  if (
    lower === 'write' ||
    lower === 'edit' ||
    lower === 'multiedit' ||
    lower === 'write_file' ||
    lower === 'edit_file'
  )
    return 'file_edit'
  if (lower === 'grep' || lower === 'glob') return 'search'
  return 'other'
}

export function summarizeToolSteps(steps: readonly RenderToolStep[]): ToolStepSummary {
  const buckets: BucketCount[] = []
  let runningCount = 0
  let errorCount = 0
  for (const s of steps) {
    if (s.status === 'running') runningCount++
    if (s.status === 'error') errorCount++
    const key = classifyToolBucket(s.name)
    const count = key === 'interaction' ? countQuestions(s.inputJson) : 1
    const existing = buckets.find((b) => b.key === key)
    if (existing) existing.count += count
    else buckets.push({ key, count })
  }
  return { buckets, runningCount, errorCount }
}

function countQuestions(inputJson?: string): number {
  if (!inputJson) return 1
  try {
    const parsed = JSON.parse(inputJson) as { questions?: unknown }
    return Array.isArray(parsed.questions) && parsed.questions.length > 0
      ? parsed.questions.length
      : 1
  } catch {
    return 1
  }
}
