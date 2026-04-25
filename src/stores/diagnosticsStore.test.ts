import { beforeEach, describe, expect, it } from 'vitest'
import { MAX_DIAGNOSTIC_EVENTS, useDiagnosticsStore } from './diagnosticsStore'
import type { DiagnosticEvent } from '@/lib/diagnostics'

function event(seq: number, extra: Partial<DiagnosticEvent> = {}): DiagnosticEvent {
  return {
    ts: '2026-04-25T00:00:00.000Z',
    seq,
    category: 'diagnostics',
    source: 'frontend',
    level: 'info',
    event: 'chat.submit.started',
    ...extra,
  }
}

describe('diagnosticsStore', () => {
  beforeEach(() => {
    useDiagnosticsStore.getState().clearDiagnostics()
  })

  it('keeps recent diagnostics in insertion order', () => {
    useDiagnosticsStore.getState().appendDiagnostic(event(1, {
      conversationId: 'conv_1',
      runId: 'run_1',
    }))

    expect(useDiagnosticsStore.getState().events).toHaveLength(1)
    expect(useDiagnosticsStore.getState().events[0].seq).toBe(1)
    expect(useDiagnosticsStore.getState().getByRunId('run_1')).toHaveLength(1)
    expect(useDiagnosticsStore.getState().getByConversationId('conv_1')).toHaveLength(1)
  })

  it('clears diagnostics', () => {
    useDiagnosticsStore.getState().appendDiagnostic(event(1))

    useDiagnosticsStore.getState().clearDiagnostics()

    expect(useDiagnosticsStore.getState().events).toEqual([])
  })

  it('keeps a bounded ring buffer of the latest events', () => {
    for (let seq = 1; seq <= MAX_DIAGNOSTIC_EVENTS + 2; seq += 1) {
      useDiagnosticsStore.getState().appendDiagnostic(event(seq))
    }

    const events = useDiagnosticsStore.getState().events
    expect(events).toHaveLength(MAX_DIAGNOSTIC_EVENTS)
    expect(events[0].seq).toBe(3)
    expect(events.at(-1)?.seq).toBe(MAX_DIAGNOSTIC_EVENTS + 2)
  })

  it('filters by run and conversation id', () => {
    useDiagnosticsStore.getState().appendDiagnostic(event(1, { runId: 'run_1', conversationId: 'conv_1' }))
    useDiagnosticsStore.getState().appendDiagnostic(event(2, { runId: 'run_2', conversationId: 'conv_1' }))
    useDiagnosticsStore.getState().appendDiagnostic(event(3, { runId: 'run_1', conversationId: 'conv_2' }))

    expect(useDiagnosticsStore.getState().getByRunId('run_1').map((item) => item.seq)).toEqual([1, 3])
    expect(useDiagnosticsStore.getState().getByConversationId('conv_1').map((item) => item.seq)).toEqual([1, 2])
  })
})
