import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useDiagnosticsStore } from '@/stores/diagnosticsStore'
import { createInstrumentedEventHandler } from './tauri'

const invokeMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

describe('tauri diagnostics helpers', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    invokeMock.mockResolvedValue(undefined)
    useDiagnosticsStore.getState().clearDiagnostics()
  })

  it('records event handler success around callbacks', async () => {
    const callback = vi.fn()
    const handler = createInstrumentedEventHandler('streaming:done', callback)

    await handler({ payload: { conversationId: 'conv_1', runId: 'run_1' } })

    const events = useDiagnosticsStore.getState().events
    expect(callback).toHaveBeenCalledWith({ payload: { conversationId: 'conv_1', runId: 'run_1' } })
    expect(events.map((event) => event.event)).toEqual([
      'event.received',
      'event.handler.started',
      'event.handler.completed',
    ])
    expect(events[0]).toMatchObject({
      conversationId: 'conv_1',
      runId: 'run_1',
      payload: { eventName: 'streaming:done', payload: { conversationId: 'conv_1', runId: 'run_1' } },
    })
    expect(events[2]).toMatchObject({
      ok: true,
      conversationId: 'conv_1',
      runId: 'run_1',
      payload: { eventName: 'streaming:done' },
    })
  })

  it('does not record routine diagnostics for high-frequency event handlers', async () => {
    const callback = vi.fn()
    const handler = createInstrumentedEventHandler('turn:heartbeat', callback)

    await handler({ payload: { conversationId: 'conv_1', runId: 'run_1' } })

    expect(callback).toHaveBeenCalledWith({ payload: { conversationId: 'conv_1', runId: 'run_1' } })
    expect(useDiagnosticsStore.getState().events).toEqual([])
  })

  it('records event handler failure before rethrowing', async () => {
    const handler = createInstrumentedEventHandler('streaming:error', () => {
      throw new Error('boom')
    })

    await expect(handler({ payload: { conversationId: 'conv_1' } })).rejects.toThrow('boom')

    const events = useDiagnosticsStore.getState().events
    const failed = events.find((event) => event.event === 'event.handler.failed')
    expect(events.map((event) => event.event)).toEqual([
      'event.received',
      'event.handler.started',
      'event.handler.failed',
    ])
    expect(failed).toMatchObject({
      level: 'error',
      ok: false,
      conversationId: 'conv_1',
      error: 'boom',
      payload: { eventName: 'streaming:error', stack: expect.any(String) },
    })
  })
})
