import { describe, expect, it } from 'vitest'

import { buildTurnsFromMessages } from '../useTurnRenderModel'
import type { Message } from '@/types/message'
import type { ToolExecution } from '@/stores/streamingStore'

function userMsg(id: string, text: string): Message {
  return { id, conversationId: 'c1', role: 'user', createdAt: new Date().toISOString(), content: { text } }
}

function aiMsg(id: string, text: string): Message {
  return { id, conversationId: 'c1', role: 'assistant', createdAt: new Date().toISOString(), content: { text } }
}

describe('buildTurnsFromMessages', () => {
  it('groups messages into turns starting at each user message', () => {
    const msgs = [userMsg('u1', 'hi'), aiMsg('a1', 'hello'), userMsg('u2', 'again'), aiMsg('a2', 'hi!')]
    const turns = buildTurnsFromMessages(msgs, [])
    expect(turns.map((t) => t.userMessage?.id)).toEqual(['u1', 'u2'])
    expect(turns[0].aiSegments.map((s) => s.id)).toEqual(['a1'])
    expect(turns[1].aiSegments.map((s) => s.id)).toEqual(['a2'])
  })

  it('attaches tool executions to the last turn as a single ToolGroup', () => {
    const msgs = [userMsg('u1', 'x'), aiMsg('a1', 'done')]
    const tools: ToolExecution[] = [
      { toolId: 't1', toolName: 'fetch_feedback', status: 'completed' },
      { toolId: 't2', toolName: 'cluster_topics', status: 'completed' },
    ]
    const turns = buildTurnsFromMessages(msgs, tools)
    expect(turns[0].toolGroup).toBeDefined()
    expect(turns[0].toolGroup?.steps.map((s) => s.name)).toEqual(['fetch_feedback', 'cluster_topics'])
    expect(turns[0].toolGroup?.status).toBe('done')
  })

  it('marks toolGroup as running when any tool is executing', () => {
    const tools: ToolExecution[] = [
      { toolId: 't1', toolName: 'fetch', status: 'completed' },
      { toolId: 't2', toolName: 'run', status: 'executing' },
    ]
    const turns = buildTurnsFromMessages([userMsg('u1', 'x')], tools)
    expect(turns[0].toolGroup?.status).toBe('running')
  })
})
