import { renderHook, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/lib/tauri', () => ({
  expertTeamTemplateCatalog: vi.fn(async () => {
    throw new Error('ipc unavailable')
  }),
}))

import { useExpertTeamCatalog } from '../useExpertTeamCatalog'

describe('useExpertTeamCatalog', () => {
  it('falls back to builtin expert teams when IPC fails', async () => {
    const { result } = renderHook(() => useExpertTeamCatalog())
    await waitFor(() => expect(result.current.isLoading).toBe(false))
    expect(result.current.teams.length).toBeGreaterThan(0)
    expect(result.current.source).toBe('bootstrap')
  })
})
