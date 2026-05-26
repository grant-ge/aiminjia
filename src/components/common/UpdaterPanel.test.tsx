import '@testing-library/jest-dom'
import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { UpdaterPanel } from './UpdaterPanel'
import { useUpdaterStore } from '@/lib/updaterStore'

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockReturnValue(new Promise<string>(() => {})),
}))

vi.mock('react-i18next', () => ({
  initReactI18next: {
    type: '3rdParty',
    init: vi.fn(),
  },
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) => {
      if (key === 'updater.panelTitle') return `Update ${values?.version}`
      return key
    },
  }),
}))

describe('UpdaterPanel', () => {
  afterEach(() => {
    act(() => useUpdaterStore.setState({
      phase: 'idle',
      version: null,
      notes: '',
      progress: null,
      panelOpen: false,
      _update: null,
      _cachedBytes: null,
      _bootstrapPromise: null,
    }))
  })

  it('disables install while a live update handle is still downloading', () => {
    act(() => useUpdaterStore.setState({
      phase: 'downloading',
      version: '0.5.21',
      notes: '',
      progress: { downloaded: 50, total: 100 },
      panelOpen: true,
      _update: { install: vi.fn() } as never,
      _cachedBytes: null,
    }))

    render(<UpdaterPanel />)

    // During downloading phase the install-and-restart button is not rendered —
    // this is how the new UI prevents triggering install before _cachedBytes is ready.
    expect(screen.queryByRole('button', { name: 'updater.installAndRestart' })).toBeNull()
  })
})
