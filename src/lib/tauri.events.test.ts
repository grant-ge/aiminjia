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
  approvePermissionRequest,
  denyPermissionRequest,
  onDiagnosticsEvent,
  onCompactCompleted,
  onPermissionAsk,
  onPermissionResolved,
  onStreamingNotice,
  onTurnCompleted,
  onTaskStatusChanged,
  type DiagnosticsEventPayload,
  type AgentIdlePayload,
  type StreamingNoticePayload,
  type TurnCompletedPayload,
  type CompactCompletedPayload,
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

  it('exposes PERMISSION_ASK event constant with correct value', () => {
    expect(TAURI_EVENTS.PERMISSION_ASK).toBe('permission:ask')
  })

  it('exposes PERMISSION_RESOLVED event constant with correct value', () => {
    expect(TAURI_EVENTS.PERMISSION_RESOLVED).toBe('permission:resolved')
  })

  it('onPermissionAsk registers listener with correct event name', async () => {
    const handler = vi.fn()

    await onPermissionAsk(handler)

    expect(tauriEventMock.listen).toHaveBeenCalledWith(
      'permission:ask',
      expect.any(Function),
    )
  })

  it('onPermissionResolved registers listener with correct event name', async () => {
    const handler = vi.fn()

    await onPermissionResolved(handler)

    expect(tauriEventMock.listen).toHaveBeenCalledWith(
      'permission:resolved',
      expect.any(Function),
    )
  })

  it('exposes diagnostics event constant with correct value', () => {
    expect(TAURI_EVENTS.DIAGNOSTICS_EVENT).toBe('diagnostics:event')
  })

  it('DiagnosticsEventPayload keeps frontend store fields stable', () => {
    const payload: DiagnosticsEventPayload = {
      ts: '2026-04-25T00:00:00.000Z',
      seq: 12,
      category: 'diagnostics',
      level: 'info',
      source: 'backend',
      event: 'turn.started',
      conversationId: 'conv-1',
      runId: 'run-1',
    }

    expect(payload.source).toBe('backend')
    expect(payload.category).toBe('diagnostics')
    expect(payload.seq).toBe(12)
  })

  it('onDiagnosticsEvent registers listener with correct event name', async () => {
    const handler = vi.fn()

    await onDiagnosticsEvent(handler)

    expect(tauriEventMock.listen).toHaveBeenCalledWith(
      'diagnostics:event',
      expect.any(Function),
    )
  })

  it('approvePermissionRequest calls invoke with correct command and params', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    const invokeMock = vi.mocked(invoke)
    invokeMock.mockResolvedValue(undefined)

    await approvePermissionRequest('tool-call-123', null)

    expect(invokeMock).toHaveBeenCalledWith('approve_permission_request', {
      toolCallId: 'tool-call-123',
      updatedInput: null,
    })
  })

  it('denyPermissionRequest calls invoke with correct command and params', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    const invokeMock = vi.mocked(invoke)
    invokeMock.mockResolvedValue(undefined)

    await denyPermissionRequest('tool-call-123', undefined)

    expect(invokeMock).toHaveBeenCalledWith('deny_permission_request', {
      toolCallId: 'tool-call-123',
      message: undefined,
    })
  })

  it('exposes TURN_COMPLETED event constant with correct value', () => {
    expect(TAURI_EVENTS.TURN_COMPLETED).toBe('turn:completed')
  })

  it('TurnCompletedPayload keeps frontend-facing outcome shape stable', () => {
    const payload: TurnCompletedPayload = {
      conversationId: 'conv-1',
      runId: 'run-1',
      outcome: 'BudgetExceeded',
      totalInputTokens: 123,
      totalOutputTokens: 45,
      totalCostUsd: 0.12,
      permissionDenialCount: 2,
      reason: 'Reached maximum budget ($0.10)',
    }

    expect(payload.outcome).toBe('BudgetExceeded')
    expect(payload.conversationId).toBe('conv-1')
    expect(payload.totalCostUsd).toBe(0.12)
  })

  it('onTurnCompleted registers listener with correct event name', async () => {
    const handler = vi.fn()

    await onTurnCompleted(handler)

    expect(tauriEventMock.listen).toHaveBeenCalledWith(
      'turn:completed',
      expect.any(Function),
    )
  })

  it('exposes STREAMING_NOTICE event constant with correct value', () => {
    expect(TAURI_EVENTS.STREAMING_NOTICE).toBe('streaming:notice')
  })

  it('StreamingNoticePayload keeps failover fields stable', () => {
    const payload: StreamingNoticePayload = {
      conversationId: 'conv-1',
      runId: 'run-1',
      level: 'info',
      code: 'auto_failed_over',
      message: 'switched to backup',
      fromRoute: { provider: 'anthropic' },
      toRoute: { provider: 'openai' },
    }

    expect(payload.code).toBe('auto_failed_over')
    expect(payload.fromRoute?.provider).toBe('anthropic')
    expect(payload.toRoute?.provider).toBe('openai')
  })

  it('onStreamingNotice registers listener with correct event name', async () => {
    const handler = vi.fn()

    await onStreamingNotice(handler)

    expect(tauriEventMock.listen).toHaveBeenCalledWith(
      'streaming:notice',
      expect.any(Function),
    )
  })

  it('exposes compact completed event and payload shape', async () => {
    const payload: CompactCompletedPayload = {
      conversationId: 'conv-1',
      runId: 'run-1',
      boundaryId: 'boundary-1',
      trigger: 'manual',
      createdAt: '2026-06-02T00:00:00.000Z',
      tailMessageId: 'tail-1',
      preTokens: 12000,
      postTokens: 4500,
      tokensSaved: 7500,
      messagesSummarized: 18,
    }
    expect(TAURI_EVENTS.COMPACT_COMPLETED).toBe('compact:completed')
    expect(payload.tokensSaved).toBe(7500)

    const handler = vi.fn()
    await onCompactCompleted(handler)

    expect(tauriEventMock.listen).toHaveBeenCalledWith(
      'compact:completed',
      expect.any(Function),
    )
  })
})
