import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  buildDiagnosticEvent,
  recordDiagnostic,
  recordDiagnosticError,
  redactDiagnosticPayload,
  summarizePayload,
  withDiagnosticSpan,
} from './diagnostics'
import { useDiagnosticsStore } from '@/stores/diagnosticsStore'

const invokeMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

describe('diagnostics', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-25T12:34:56.789Z'))
    invokeMock.mockReset()
    invokeMock.mockResolvedValue(undefined)
    useDiagnosticsStore.getState().clearDiagnostics()
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('builds flat queryable diagnostic events', () => {
    const event = buildDiagnosticEvent({
      event: 'chat.submit.started',
      level: 'info',
      conversationId: 'conv_1',
      runId: 'run_1',
      payload: { messageLength: 12 },
    })

    expect(event.ts).toBe('2026-04-25T12:34:56.789Z')
    expect(event.category).toBe('diagnostics')
    expect(event.source).toBe('frontend')
    expect(event.event).toBe('chat.submit.started')
    expect(event.conversationId).toBe('conv_1')
    expect(event.runId).toBe('run_1')
    expect(event.payload).toEqual({ messageLength: 12 })
    expect(event.seq).toBeGreaterThan(0)
    expect(event.elapsedMs).toBeGreaterThanOrEqual(0)
  })

  it('redacts secret payload keys recursively', () => {
    const redacted = redactDiagnosticPayload({
      authorization: 'Bearer abc',
      apiKey: 'sk-test',
      nested: { password: 'pw', safe: 'ok' },
      list: [{ accessToken: 'tok', safe: 'value' }],
    })

    expect(JSON.stringify(redacted)).not.toContain('Bearer abc')
    expect(JSON.stringify(redacted)).not.toContain('sk-test')
    expect(JSON.stringify(redacted)).not.toContain('pw')
    expect(JSON.stringify(redacted)).not.toContain('tok')
    expect(redacted).toMatchObject({
      authorization: '[REDACTED]',
      apiKey: '[REDACTED]',
      nested: { password: '[REDACTED]', safe: 'ok' },
      list: [{ accessToken: '[REDACTED]', safe: 'value' }],
    })
  })

  it('redacts bearer tokens, query tokens, and sk-prefixed secrets in string values', () => {
    const redacted = redactDiagnosticPayload({
      header: 'Authorization: Bearer nested-secret',
      url: 'https://example.test?access_token=query-secret&ok=1',
      key: 'sk-live-secret',
    })

    const raw = JSON.stringify(redacted)
    expect(raw).not.toContain('nested-secret')
    expect(raw).not.toContain('query-secret')
    expect(raw).not.toContain('sk-live-secret')
    expect(raw).toContain('[REDACTED]')
  })

  it('summarizes large payloads without dropping query fields', () => {
    const summary = summarizePayload({
      text: 'x'.repeat(300),
      list: Array.from({ length: 20 }, (_, i) => i),
      runId: 'run_1',
    })

    expect(summary).toMatchObject({
      text: expect.stringContaining('[truncated'),
      list: expect.arrayContaining([0, 1, 2]),
      runId: 'run_1',
    })
    expect((summary as { list: unknown[] }).list).toHaveLength(11)
  })

  it('records diagnostics to the local store and forwards sanitized payloads', async () => {
    const event = recordDiagnostic({
      event: 'ipc.invoke.completed',
      level: 'debug',
      ok: true,
      command: 'send_message',
      payload: { authorization: 'Bearer secret', header: 'Authorization: Bearer nested-secret', text: 'x'.repeat(260) },
    })

    expect(useDiagnosticsStore.getState().events).toEqual([event])
    expect(invokeMock).toHaveBeenCalledWith('record_frontend_diagnostic', {
      diagnostic: event,
    })
    expect(JSON.stringify(event.payload)).not.toContain('Bearer secret')
    expect(JSON.stringify(event.payload)).not.toContain('nested-secret')
    expect(JSON.stringify(event.payload)).toContain('[truncated')
  })

  it('records diagnostic errors with message and stack details', () => {
    const error = new Error('boom')

    const event = recordDiagnosticError('event.handler.failed', error, {
      conversationId: 'conv_1',
      payload: { safe: true },
    })

    expect(event.level).toBe('error')
    expect(event.ok).toBe(false)
    expect(event.error).toBe('boom')
    expect(event.conversationId).toBe('conv_1')
    expect(event.payload).toMatchObject({ safe: true, stack: expect.any(String) })
  })

  it('wraps async work with started and completed span diagnostics', async () => {
    vi.spyOn(performance, 'now')
      .mockReturnValueOnce(100)
      .mockReturnValueOnce(100)
      .mockReturnValueOnce(125)
      .mockReturnValueOnce(125)

    const result = await withDiagnosticSpan(
      { event: 'ipc.invoke', command: 'send_message' },
      async () => 'ok',
    )

    const events = useDiagnosticsStore.getState().events
    expect(result).toBe('ok')
    expect(events.map((event) => event.event)).toEqual([
      'ipc.invoke.started',
      'ipc.invoke.completed',
    ])
    expect(events[1]).toMatchObject({ ok: true, durationMs: 25 })
  })

  it('wraps async failures with failed span diagnostics and rethrows', async () => {
    vi.spyOn(performance, 'now')
      .mockReturnValueOnce(200)
      .mockReturnValueOnce(200)
      .mockReturnValueOnce(240)
      .mockReturnValueOnce(240)

    await expect(
      withDiagnosticSpan(
        { event: 'ipc.invoke', command: 'send_message' },
        async () => {
          throw new Error('nope')
        },
      ),
    ).rejects.toThrow('nope')

    const events = useDiagnosticsStore.getState().events
    expect(events.map((event) => event.event)).toEqual([
      'ipc.invoke.started',
      'ipc.invoke.failed',
    ])
    expect(events[1]).toMatchObject({ ok: false, level: 'error', durationMs: 40, error: 'nope' })
  })
})
