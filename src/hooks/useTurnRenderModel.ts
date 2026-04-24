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
  message: Message
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
        userMessage: { id: m.id, text: m.content.text ?? '' },
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
      // assistant 有 toolCalls → 这是一次工具调用轮次，初始化 toolGroup
      if (m.toolCalls && m.toolCalls.length > 0) {
        if (!current.toolGroup) {
          current.toolGroup = { status: 'running', steps: [], durationMs: 0 }
        }
      }
      // 有文字内容才加 aiSegment
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
      // 确保 toolGroup 存在
      if (!current.toolGroup) {
        current.toolGroup = { status: 'done', steps: [], durationMs: 0 }
      }
      const result = m.toolResult
      current.toolGroup.steps.push({
        index: current.toolGroup.steps.length + 1,
        name: result.name,
        status: result.isError ? 'error' : 'done',
        durationMs: result.durationMs,
      })
      current.toolGroup.durationMs =
        current.toolGroup.steps.reduce((acc, s) => acc + (s.durationMs ?? 0), 0)
    }
  }

  // 最后一个 turn：只有当该 turn 尚无来自历史 role=tool 消息的步骤时，
  // 才用实时 toolExecutions 覆盖（表示 streaming 正在进行中）
  if (toolExecutions.length > 0 && turns.length > 0) {
    const target = turns[turns.length - 1]
    const hasHistoricalSteps = target.toolGroup != null && target.toolGroup.steps.length > 0
    if (!hasHistoricalSteps) {
      const steps: RenderToolStep[] = toolExecutions.map((t, i) => ({
        index: i + 1,
        name: t.toolName,
        status: toolExecStatusToStep(t.status),
        durationMs: t.durationMs,
      }))
      const running = steps.some((s) => s.status === 'running')
      target.toolGroup = {
        status: running ? 'running' : 'done',
        steps,
        durationMs: steps.reduce((acc, s) => acc + (s.durationMs ?? 0), 0),
      }
    }
  }

  // 整理所有 toolGroup 最终状态
  for (const turn of turns) {
    if (turn.toolGroup && turn.toolGroup.steps.length > 0) {
      const hasRunning = turn.toolGroup.steps.some((s) => s.status === 'running')
      turn.toolGroup.status = hasRunning ? 'running' : 'done'
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
