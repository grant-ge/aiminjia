/**
 * Plan-D: 把扁平 messages + conversationStreamState.toolExecutions
 * 投影为 RenderTurn[]，供 MessageList 渲染。
 */
import { useMemo } from 'react'
import type { ReactNode } from 'react'

import { useChatStore } from '@/stores/chatStore'
import type { ToolExecution } from '@/stores/streamingStore'
import type { GeneratedFile, Message, SkillCommandBreadcrumb } from '@/types/message'

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
  title: string
  sub: string
  appName: string
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

function normalizeGeneratedFile(f: GeneratedFile): RenderGeneratedFile {
  const anyF = f as unknown as {
    id: string; title?: string; fileName?: string;
    subtitle?: string; appName?: string; format?: string;
  }
  return {
    id: anyF.id,
    title: anyF.title || anyF.fileName || '未命名文件',
    sub: anyF.subtitle || anyF.format || '',
    appName: anyF.appName || 'Open',
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
          current.generatedFiles.push(normalizeGeneratedFile(f))
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
