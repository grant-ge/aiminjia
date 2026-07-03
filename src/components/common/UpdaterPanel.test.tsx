import '@testing-library/jest-dom'
import { act, fireEvent, render, screen } from '@testing-library/react'
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

const defaultActions = {
  startDownload: useUpdaterStore.getState().startDownload,
  retryDownload: useUpdaterStore.getState().retryDownload,
  installNow: useUpdaterStore.getState().installNow,
}

describe('UpdaterPanel', () => {
  afterEach(() => {
    act(() => useUpdaterStore.setState({
      phase: 'idle',
      version: null,
      notes: '',
      progress: null,
      panelOpen: false,
      _devPreview: false,
      _update: null,
      _cachedBytes: null,
      _bootstrapPromise: null,
      startDownload: defaultActions.startDownload,
      retryDownload: defaultActions.retryDownload,
      installNow: defaultActions.installNow,
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

    expect(screen.getByRole('dialog')).toHaveAttribute('data-aijia-updater-phase', 'downloading')
    // During downloading phase the install-and-restart button is not rendered —
    // this is how the new UI prevents triggering install before _cachedBytes is ready.
    expect(screen.queryByRole('button', { name: 'updater.installAndRestart' })).toBeNull()
  })

  it('routes the failed retry button through retryDownload', () => {
    const startDownload = vi.fn()
    const retryDownload = vi.fn()
    act(() => useUpdaterStore.setState({
      phase: 'failed',
      version: '0.5.32',
      notes: '',
      progress: null,
      panelOpen: true,
      error: 'download failed',
      _update: { install: vi.fn() } as never,
      _cachedBytes: null,
      startDownload,
      retryDownload,
    }))

    render(<UpdaterPanel />)
    const retryButton = screen.getByRole('button', { name: 'updater.retry' })
    expect(retryButton).toHaveAttribute('data-aijia-updater-action', 'retry')
    expect(screen.getByRole('dialog')).toHaveAttribute('data-aijia-updater-phase', 'failed')
    fireEvent.click(retryButton)

    expect(retryDownload).toHaveBeenCalledTimes(1)
    expect(startDownload).not.toHaveBeenCalled()
  })

  it.each([
    ['available', 'download', 'updater.updateNow'],
    ['ready', 'install', 'updater.installAndRestart'],
    ['failed', 'retry', 'updater.retry'],
  ] as const)(
    'keeps %s preview action local instead of calling real updater actions',
    (phase, action, label) => {
      const startDownload = vi.fn()
      const retryDownload = vi.fn()
      const installNow = vi.fn()
      act(() => useUpdaterStore.setState({
        phase,
        version: '0.5.99-preview',
        notes: '',
        progress: phase === 'ready' ? { downloaded: 10, total: 10 } : null,
        panelOpen: true,
        _devPreview: true,
        _update: null,
        _cachedBytes: null,
        startDownload,
        retryDownload,
        installNow,
      }))

      render(<UpdaterPanel />)

      const dialog = screen.getByRole('dialog')
      expect(dialog).toHaveAttribute('data-aijia-updater-dev-preview', 'true')
      const button = screen.getByRole('button', { name: label })
      expect(button).toHaveAttribute('data-aijia-updater-action', action)
      fireEvent.click(button)

      expect(useUpdaterStore.getState().panelOpen).toBe(false)
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
      expect(startDownload).not.toHaveBeenCalled()
      expect(retryDownload).not.toHaveBeenCalled()
      expect(installNow).not.toHaveBeenCalled()
    },
  )

  it('exposes stable updater action selectors for intent tests', () => {
    act(() => useUpdaterStore.setState({
      phase: 'ready',
      version: '0.5.35-test.1',
      notes: '',
      progress: { downloaded: 10, total: 10 },
      panelOpen: true,
      _update: { install: vi.fn() } as never,
      _cachedBytes: new Uint8Array([1]),
      installNow: vi.fn(),
    }))

    render(<UpdaterPanel />)

    expect(screen.getByRole('dialog')).toHaveAttribute('data-aijia-updater-panel')
    expect(screen.getByRole('dialog')).toHaveAttribute('data-aijia-updater-version', '0.5.35-test.1')
    expect(screen.getByRole('button', { name: 'updater.installAndRestart' }))
      .toHaveAttribute('data-aijia-updater-action', 'install')
  })

  it('renders staged install progress while installing', () => {
    act(() => useUpdaterStore.setState({
      phase: 'installing',
      version: '0.5.35-test.1',
      notes: '',
      progress: { downloaded: 10, total: 10 },
      installProgress: { stage: 'installing', current: 3, total: 4 },
      panelOpen: true,
      _update: { install: vi.fn() } as never,
      _cachedBytes: new Uint8Array([1]),
    } as never))

    render(<UpdaterPanel />)

    expect(screen.getByRole('dialog')).toHaveAttribute('data-aijia-updater-phase', 'installing')
    expect(screen.getByTestId('updater-install-progress')).toHaveAttribute('data-aijia-updater-install-percent', '75')
    expect(screen.getByText('updater.installStage.installing')).toBeInTheDocument()
  })

  it.each(['downloading', 'ready'] as const)('keeps %s updater content inset from the dialog edge', (phase) => {
    act(() => useUpdaterStore.setState({
      phase,
      version: '0.5.36-1',
      notes: '- 修复弹窗内容贴边',
      progress: { downloaded: 10, total: 10 },
      panelOpen: true,
      _update: { install: vi.fn() } as never,
      _cachedBytes: phase === 'ready' ? new Uint8Array([1]) : null,
    }))

    render(<UpdaterPanel />)

    expect(screen.getByTestId('updater-panel-body')).toHaveClass('px-6')
  })
})
