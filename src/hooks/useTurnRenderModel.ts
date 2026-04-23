/**
 * Plan-D: 把扁平 messages + conversationStreamState.toolExecutions
 * 投影为 RenderTurn[]，供 MessageList 渲染。
 *
 * 原则：
 * 1. 每个 user message 开一个新 turn
 * 2. assistant message 并入同一个 turn
 * 3. toolExecutions 归入最后一个 turn
 * 4. AI 消息的 generatedFiles 展平到所在 turn
 */
import { useMemo } from 'react'
import type { ReactNode } from 'react'

import { useChatStore } from '@/stores/chatStore'
import type { ToolExecution } from '@/stores/streamingStore'
import type { GeneratedFile, Message } from '@/types/message'

export interface RenderAiSegment {
  id: string
  text: string
}

export interface RenderToolStep {
  index: number
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
  userMessage?: { id: string; text: string }
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

export function buildTurnsFromMessages(
  messages: Message[],
  toolExecutions: ToolExecution[],
): RenderTurn[] {
  const turns: RenderTurn[] = []
  let current: RenderTurn | null = null

  for (const m of messages) {
    if (m.role === 'user') {
      current = {
        userMessage: { id: m.id, text: m.content.text || '' },
        aiSegments: [],
        toolGroup: undefined,
        generatedFiles: [],
        suggestions: [],
      }
      turns.push(current)
      continue
    }
    if (m.role === 'assistant') {
      if (!current) {
        current = { userMessage: undefined, aiSegments: [], toolGroup: undefined, generatedFiles: [], suggestions: [] }
        turns.push(current)
      }
      if (m.content.text) {
        current.aiSegments.push({ id: m.id, text: m.content.text })
      }
      if (m.content.generatedFiles?.length) {
        for (const f of m.content.generatedFiles) {
          current.generatedFiles.push(normalizeGeneratedFile(f))
        }
      }
    }
  }

  if (toolExecutions.length > 0 && turns.length > 0) {
    const target = turns[turns.length - 1]
    const steps: RenderToolStep[] = toolExecutions.map((t, i) => ({
      index: i + 1,
      name: t.toolName,
      status: toolExecStatusToStep(t.status),
    }))
    const running = steps.some((s) => s.status === 'running')
    target.toolGroup = { status: running ? 'running' : 'done', steps, durationMs: 0 }
  }

  return turns
}

export function useTurnRenderModel(): RenderTurn[] {
  const messages = useChatStore((s) => s.messages)
  const activeId = useChatStore((s) => s.activeConversationId)
  const toolExecutions = useChatStore((s) => {
    if (!activeId) return [] as ToolExecution[]
    return s.streamStates[activeId]?.toolExecutions ?? []
  })
  return useMemo(() => buildTurnsFromMessages(messages, toolExecutions), [messages, toolExecutions])
}
