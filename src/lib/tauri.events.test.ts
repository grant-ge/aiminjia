import { describe, expect, it, vi } from 'vitest'

const tauriEventMock = vi.hoisted(() => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: tauriEventMock.listen,
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

import {
  TAURI_EVENTS,
  onTaskStatusChanged,
  type AgentIdlePayload,
} from './tauri'

describe('tauri event contract', () => {
  it('exposes task status changed event constant', () => {
    expect(TAURI_EVENTS.TASK_STATUS_CHANGED).toBe('task:status-changed')
  })

  it('agent idle payload keeps scope fields for child or primary discrimination', () => {
    const payload: AgentIdlePayload = {
      conversationId: 'conv-1',
      runId: 'run-1',
      agentId: 'agent-1',
      scope: 'child',
    }

    expect(payload.scope).toBe('child')
    expect(payload.runId).toBe('run-1')
    expect(payload.agentId).toBe('agent-1')
  })

  it('registers task status changed listener with the correct event name', async () => {
    const handler = vi.fn()

    await onTaskStatusChanged(handler)

    expect(tauriEventMock.listen).toHaveBeenCalledWith(
      'task:status-changed',
      expect.any(Function),
    )
  })
})
