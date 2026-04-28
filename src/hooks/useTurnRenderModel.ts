/**
 * Plan-D: 把扁平 messages + conversationStreamState.toolExecutions
 * 投影为 RenderTurn[]，供 MessageList 渲染。
 */
import { useMemo } from 'react'
import type { ReactNode } from 'react'

import {
  isFileActionEnabled,
  isPreviewableFileType,
} from '@/components/chat/generatedFileActions'
import { useChatStore } from '@/stores/chatStore'
import type { ToolExecution } from '@/stores/streamingStore'
import type { FileAction, GeneratedFile, Message, SkillCommandBreadcrumb } from '@/types/message'

export interface RenderAiSegment {
  id: string
  text: string
  message: Message
}

export interface RenderToolStep {
  index: number
  toolCallId: string
  name: string
  status: 'running' | 'done' | 'error'
  durationMs?: number
  inputJson?: string
  output?: ReactNode
}

export interface RenderToolGroup {
  status: 'running' | 'done'
  steps: RenderToolStep[]
  durationMs: number
}

export interface RenderGeneratedFile {
  id: string
  conversationId: string
  title: string
  fileName: string
  sub: string
  appName: string
  fileType?: string
  actions: FileAction[]
  canPreview: boolean
  canOpenExternal: boolean
  canReveal: boolean
  primaryAction: 'preview' | 'open'
}

export interface RenderTurn {
  userMessage?: { id: string; text: string; commandText?: string; skillCommand?: SkillCommandBreadcrumb }
  aiSegments: RenderAiSegment[]
  toolGroup?: RenderToolGroup
  generatedFiles: RenderGeneratedFile[]
  suggestions: string[]
}

function toolExecStatusToStep(s: ToolExecution['status']): RenderToolStep['status'] {
  return s === 'executing' ? 'running' : s === 'error' ? 'error' : 'done'
}

function formatFileSize(bytes: number | undefined): string | null {
  if (bytes == null || bytes <= 0) return null
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function displayFileType(fileType: string | undefined): string | null {
  const value = fileType?.trim().toUpperCase()
  if (!value) return null
  if (value === 'EXCEL') return 'XLS'
  return value
}

function displayFileCategory(category: string | undefined): string | null {
  switch (category) {
    case 'report': return '报告'
    case 'chart': return '图表'
    case 'data': return '数据'
    case 'analysis': return '分析'
    case 'script': return '脚本'
    case 'temp': return '临时'
    default: return category || null
  }
}

function buildGeneratedFileMeta(f: GeneratedFile, format?: string, subtitle?: string): string {
  if (subtitle) return subtitle
  if (f.isDegraded) {
    const actual = displayFileType(f.fileType) ?? format?.toUpperCase() ?? '文件'
    const requested = f.requestedFormat?.trim().toUpperCase()
    return requested ? `已降级为 ${actual} · 原请求 ${requested}` : `已降级为 ${actual}`
  }

  const parts = [
    formatFileSize(f.fileSize),
    displayFileCategory(f.category),
  ].filter(Boolean)
  return parts.join(' · ')
}

function normalizeGeneratedFile(f: GeneratedFile, conversationId: string): RenderGeneratedFile {
  const anyF = f as unknown as {
    id: string; title?: string; fileName?: string;
    subtitle?: string; appName?: string; format?: string;
    fileType?: string; actions?: FileAction[];
  }
  const title = anyF.title || anyF.fileName || '未命名文件'
  const fileName = anyF.fileName ?? title
  const fileType = anyF.fileType
  const actions = anyF.actions ?? []
  const canPreviewByType = isPreviewableFileType(fileType, fileName)
  const canPreview = canPreviewByType && isFileActionEnabled(actions, 'preview')
  const canOpenExternal = isFileActionEnabled(actions, 'open')
  const canReveal = isFileActionEnabled(actions, 'reveal')
  return {
    id: anyF.id,
    conversationId,
    title,
    fileName,
    sub: buildGeneratedFileMeta(f, anyF.format, anyF.subtitle),
    appName: anyF.appName || 'Open',
    fileType,
    actions,
    canPreview,
    canOpenExternal,
    canReveal,
    primaryAction: canPreview ? 'preview' : 'open',
  }
}

function ensureToolGroup(turn: RenderTurn, status: RenderToolGroup['status'] = 'running'): RenderToolGroup {
  if (!turn.toolGroup) {
    turn.toolGroup = { status, steps: [], durationMs: 0 }
  }
  return turn.toolGroup
}

function stringifyInput(input: unknown): string | undefined {
  if (input == null) return undefined
  try {
    return JSON.stringify(input, null, 2)
  } catch {
    return String(input)
  }
}

function truncateOutput(text: string, isError: boolean, maxLines = 20): string {
  const lines = text.split('\n')
  if (lines.length <= maxLines) return text
  if (isError) {
    return `... (total ${lines.length} lines, truncated)\n${lines.slice(-maxLines).join('\n')}`
  }
  return `${lines.slice(0, maxLines).join('\n')}\n... (total ${lines.length} lines, truncated)`
}

function recalcToolGroup(group: RenderToolGroup): void {
  group.steps.forEach((step, idx) => {
    step.index = idx + 1
  })
  group.durationMs = group.steps.reduce((acc, s) => acc + (s.durationMs ?? 0), 0)
  group.status = group.steps.some((s) => s.status === 'running') ? 'running' : 'done'
}


function normalizeUserMessageForRender(message: Message): NonNullable<RenderTurn['userMessage']> {
  const rawText = message.content.text ?? ''
  if (message.content.skillCommand || message.content.commandText) {
    return {
      id: message.id,
      text: rawText,
      commandText: message.content.commandText,
      skillCommand: message.content.skillCommand,
    }
  }

  const slashMatch = rawText.match(/^\/([A-Za-z0-9][A-Za-z0-9_-]*)(?:\s+([\s\S]*))?$/)
  if (!slashMatch) {
    return { id: message.id, text: rawText }
  }

  const skillId = slashMatch[1]
  const text = slashMatch[2]?.trimStart() ?? ''
  const command = `/${skillId}`
  return {
    id: message.id,
    text,
    commandText: rawText,
    skillCommand: { id: skillId, label: skillId, command },
  }
}

export function buildTurnsFromMessages(
  messages: Message[],
  toolExecutions: ToolExecution[],
): RenderTurn[] {
  const turns: RenderTurn[] = []
  let current: RenderTurn | null = null

  for (const m of messages) {
    if (m.role === 'user') {
      current = {
        userMessage: normalizeUserMessageForRender(m),
        aiSegments: [],
        toolGroup: undefined,
        generatedFiles: [],
        suggestions: [],
      }
      turns.push(current)
      continue
    }

    if (!current) {
      current = {
        userMessage: undefined,
        aiSegments: [],
        toolGroup: undefined,
        generatedFiles: [],
        suggestions: [],
      }
      turns.push(current)
    }

    if (m.role === 'assistant') {
      if (m.toolCalls?.length) {
        const group = ensureToolGroup(current)
        for (const tc of m.toolCalls) {
          if (!group.steps.some((s) => s.toolCallId === tc.id)) {
            group.steps.push({
              index: group.steps.length + 1,
              toolCallId: tc.id,
              name: tc.name,
              status: 'running',
              inputJson: stringifyInput(tc.arguments),
            })
          }
        }
      }
      if (m.content.text) {
        current.aiSegments.push({ id: m.id, text: m.content.text, message: m })
      }
      if (m.content.generatedFiles?.length) {
        for (const f of m.content.generatedFiles) {
          current.generatedFiles.push(normalizeGeneratedFile(f, m.conversationId))
        }
      }
      continue
    }

    if (m.role === 'tool' && m.toolResult) {
      const group = ensureToolGroup(current)
      const result = m.toolResult
      const existing = group.steps.find((s) => s.toolCallId === result.toolCallId)
      const output = result.content ? truncateOutput(result.content, result.isError) : undefined
      if (existing) {
        existing.status = result.isError ? 'error' : 'done'
        existing.output = output
        existing.durationMs = result.durationMs
      } else {
        group.steps.push({
          index: group.steps.length + 1,
          toolCallId: result.toolCallId,
          name: result.name,
          status: result.isError ? 'error' : 'done',
          output,
          durationMs: result.durationMs,
        })
      }
    }
  }

  if (toolExecutions.length > 0 && turns.length > 0) {
    const target = turns[turns.length - 1]
    const group = ensureToolGroup(target)
    for (const t of toolExecutions) {
      const existing = group.steps.find((s) => s.toolCallId === t.toolId)
      const output = t.output ? truncateOutput(t.output, t.status === 'error') : undefined
      if (existing) {
        existing.status = toolExecStatusToStep(t.status)
        if (t.durationMs != null) existing.durationMs = t.durationMs
        if (t.input != null && !existing.inputJson) existing.inputJson = stringifyInput(t.input)
        if (output && !existing.output) existing.output = output
      } else {
        group.steps.push({
          index: group.steps.length + 1,
          toolCallId: t.toolId,
          name: t.toolName,
          status: toolExecStatusToStep(t.status),
          durationMs: t.durationMs,
          inputJson: stringifyInput(t.input),
          output,
        })
      }
    }
  }

  for (const turn of turns) {
    if (turn.toolGroup) {
      recalcToolGroup(turn.toolGroup)
    }
  }

  return turns
}

const EMPTY_TOOL_EXECUTIONS: ToolExecution[] = []

export function useTurnRenderModel(): RenderTurn[] {
  const messages = useChatStore((s) => s.messages)
  const activeId = useChatStore((s) => s.activeConversationId)
  const toolExecutions = useChatStore((s) => {
    if (!activeId) return EMPTY_TOOL_EXECUTIONS
    return s.streamStates[activeId]?.toolExecutions ?? EMPTY_TOOL_EXECUTIONS
  })
  return useMemo(() => buildTurnsFromMessages(messages, toolExecutions), [messages, toolExecutions])
}
