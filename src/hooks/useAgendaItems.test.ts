import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AgendaItem } from '@/lib/tauri'

const tauriMock = vi.hoisted(() => ({
  listAgendaItems: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import { useAgendaItems } from './useAgendaItems'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

function makeItem(id: string): AgendaItem {
  const now = new Date().toISOString()
  return {
    id,
    title: `Agenda ${id}`,
    prompt: 'Do work',
    organizerPersonaId: 'p1',
    participants: [{ personaId: 'p1', joinedAt: now }],
    startAt: now,
    timezone: 'Asia/Shanghai',
    rule: null,
    skipDates: [],
    nextFireAt: now,
    occurrenceCount: 0,
    status: 'active',
    overrideOf: null,
    workspacePath: null,
    createdAt: now,
    updatedAt: now,
  }
}

describe('useAgendaItems', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('reports loading during initial load and clears it after items arrive', async () => {
    const initial = deferred<AgendaItem[]>()
    tauriMock.listAgendaItems.mockReturnValueOnce(initial.promise)

    const { result } = renderHook(() => useAgendaItems())

    await waitFor(() => {
      expect(tauriMock.listAgendaItems).toHaveBeenCalledTimes(1)
      expect(result.current.loading).toBe(true)
    })

    const item = makeItem('latest')
    await act(async () => {
      initial.resolve([item])
      await initial.promise
    })

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
      expect(result.current.items).toEqual([item])
      expect(result.current.error).toBeNull()
    })
  })

  it('keeps the latest request result when refresh calls resolve out of order', async () => {
    const initial = deferred<AgendaItem[]>()
    const slowRefresh = deferred<AgendaItem[]>()
    const fastRefresh = deferred<AgendaItem[]>()
    tauriMock.listAgendaItems
      .mockReturnValueOnce(initial.promise)
      .mockReturnValueOnce(slowRefresh.promise)
      .mockReturnValueOnce(fastRefresh.promise)

    const { result } = renderHook(() => useAgendaItems())

    await act(async () => {
      initial.resolve([])
      await initial.promise
    })

    let slowDone!: Promise<void>
    let fastDone!: Promise<void>
    await act(async () => {
      slowDone = result.current.refresh()
      fastDone = result.current.refresh()
    })

    const latest = makeItem('latest')
    await act(async () => {
      fastRefresh.resolve([latest])
      await fastDone
    })

    await waitFor(() => {
      expect(result.current.items).toEqual([latest])
    })

    const stale = makeItem('stale')
    await act(async () => {
      slowRefresh.resolve([stale])
      await slowDone
    })

    expect(result.current.items).toEqual([latest])
  })
})
