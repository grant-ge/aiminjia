import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { useTauriEvent } from './useTauriEvent'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('useTauriEvent', () => {
  it('does not throw when setup resolves normally', () => {
    const unlisten = vi.fn()
    const setup = vi.fn().mockResolvedValue(unlisten)

    expect(() => renderHook(() => useTauriEvent(setup))).not.toThrow()
  })

  it('calls unlisten on unmount after listener registration completes', async () => {
    const unlisten = vi.fn()
    const setup = vi.fn().mockResolvedValue(unlisten)
    const { unmount } = renderHook(() => useTauriEvent(setup))

    await waitFor(() => {
      expect(setup).toHaveBeenCalledTimes(1)
    })

    unmount()

    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('logs error and does not throw when setup rejects', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {})
    const setup = vi.fn().mockRejectedValue(new Error('Tauri listen failed'))

    expect(() => renderHook(() => useTauriEvent(setup))).not.toThrow()

    await waitFor(() => {
      expect(consoleError).toHaveBeenCalledWith(
        expect.stringContaining('[useTauriEvent]'),
        expect.any(Error),
      )
    })
  })
})
