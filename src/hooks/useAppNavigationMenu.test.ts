import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { TAURI_EVENTS } from '@/lib/tauri'
import { useUiStore } from '@/stores/uiStore'

const listenMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/event', () => ({
  listen: listenMock,
}))

import { useAppNavigationMenu } from './useAppNavigationMenu'

describe('useAppNavigationMenu', () => {
  let handler: ((event: { payload: string }) => void) | null = null
  let unlisten: ReturnType<typeof vi.fn>

  beforeEach(() => {
    localStorage.clear()
    handler = null
    unlisten = vi.fn()
    listenMock.mockReset()
    listenMock.mockImplementation(async (_event: string, nextHandler: (event: { payload: string }) => void) => {
      handler = nextHandler
      return unlisten
    })
    useUiStore.setState({
      route: { kind: 'skill-detail', skillId: 'sales-followup' },
      backStack: [{ kind: 'chat', conversationId: 'conv-1' }],
      forwardStack: [],
    })
  })

  it('listens to native navigation menu commands', async () => {
    renderHook(() => useAppNavigationMenu())

    await waitFor(() => {
      expect(listenMock).toHaveBeenCalledWith(
        TAURI_EVENTS.NAVIGATION_MENU_COMMAND,
        expect.any(Function),
      )
    })
  })

  it('routes backward and forward from native menu commands', async () => {
    renderHook(() => useAppNavigationMenu())
    await waitFor(() => expect(handler).not.toBeNull())

    handler?.({ payload: 'back' })
    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'conv-1' })

    handler?.({ payload: 'forward' })
    expect(useUiStore.getState().route).toEqual({
      kind: 'skill-detail',
      skillId: 'sales-followup',
    })
  })

  it('ignores unknown native menu commands', async () => {
    renderHook(() => useAppNavigationMenu())
    await waitFor(() => expect(handler).not.toBeNull())

    handler?.({ payload: 'open-devtools' })

    expect(useUiStore.getState().route).toEqual({
      kind: 'skill-detail',
      skillId: 'sales-followup',
    })
  })

  it('unsubscribes on unmount', async () => {
    const { unmount } = renderHook(() => useAppNavigationMenu())
    await waitFor(() => expect(handler).not.toBeNull())

    unmount()

    expect(unlisten).toHaveBeenCalled()
  })
})
