import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const checkMock = vi.fn()
const relaunchMock = vi.fn()
const getVersionMock = vi.fn()
const eventHandlers = new Map<string, (event: { payload: unknown }) => void>()
const listenMock = vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
  eventHandlers.set(event, handler)
  return () => {}
})
const invokeMock = vi.fn()

vi.mock('@tauri-apps/plugin-updater', () => ({ check: (...a: unknown[]) => checkMock(...a) }))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: (...a: unknown[]) => relaunchMock(...a) }))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: (...a: unknown[]) => getVersionMock(...a) }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: (event: string, handler: (event: { payload: unknown }) => void) => listenMock(event, handler),
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }))

async function loadModules() {
  vi.resetModules()
  const storeMod = await import('./updaterStore')
  const notifMod = await import('@/stores/notificationStore')
  return {
    useUpdaterStore: storeMod.useUpdaterStore,
    useNotificationStore: notifMod.useNotificationStore,
  }
}

function setupCommandMocks(opts: {
  cacheStatus?: 'complete' | 'partial' | 'none'
  cachedBytes?: number[]
  downloadResolve?: boolean
  platformKey?: string
}) {
  invokeMock.mockImplementation(async (cmd: string) => {
    if (cmd === 'updater_platform_key') return opts.platformKey ?? 'darwin-aarch64'
    if (cmd === 'updater_check_cache') {
      return { status: opts.cacheStatus ?? 'none', downloaded_size: 0 }
    }
    if (cmd === 'updater_read_cached_bytes') {
      return opts.cachedBytes ?? [1, 2, 3]
    }
    if (cmd === 'updater_download') {
      if (opts.downloadResolve === false) throw new Error('network timeout')
      return undefined
    }
    if (cmd === 'updater_clear_cache') return undefined
    if (cmd === 'updater_install_cached') return undefined
    throw new Error(`unexpected command: ${cmd}`)
  })
}

function fakeUpdate(version = '0.5.30', body = 'notes') {
  return {
    version,
    body,
    rawJson: { platforms: { 'darwin-aarch64': { url: 'https://example/pkg.tar.gz' } } },
    install: vi.fn().mockResolvedValue(undefined),
  }
}

function fakeMultiPlatformUpdate(version = '0.5.30') {
  return {
    version,
    body: 'notes',
    rawJson: {
      platforms: {
        'darwin-aarch64': { url: 'https://example/AIjia.app.tar.gz' },
        'darwin-x86_64': { url: 'https://example/AIjia_x64.app.tar.gz' },
        'windows-x86_64': { url: 'https://example/AIjia_x64-setup.exe' },
      },
    },
    install: vi.fn().mockResolvedValue(undefined),
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  eventHandlers.clear()
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('updaterStore.bootstrap', () => {
  it('tracks staged install progress events', async () => {
    checkMock.mockResolvedValue(null)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    useUpdaterStore.setState({ version: '0.5.34' })

    eventHandlers.get('updater:install-progress')?.({
      payload: {
        version: '0.5.34',
        stage: 'installing',
        current: 3,
        total: 4,
      },
    })

    expect((useUpdaterStore.getState() as unknown as {
      installProgress: { stage: string; current: number; total: number } | null
    }).installProgress).toEqual({
      stage: 'installing',
      current: 3,
      total: 4,
    })
  })

  it('stays idle when no update available', async () => {
    checkMock.mockResolvedValue(null)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('idle')
  })

  it('stays idle when server version equals current', async () => {
    checkMock.mockResolvedValue(fakeUpdate('0.5.29'))
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('idle')
  })

  it('always auto-starts download when no cache (no autoDownload toggle anymore)', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'none', cachedBytes: [9, 9] })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    // bootstrap kicks off startDownload via void (not awaited).
    // Wait for the phase to settle on 'ready'.
    await vi.waitFor(() => {
      expect(useUpdaterStore.getState().phase).toBe('ready')
    })
  })

  it('manual trigger: stays in available when no cache (waits for user confirm)', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'none' })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap({ triggeredBy: 'manual' })
    expect(useUpdaterStore.getState().phase).toBe('available')
  })

  it('cache complete: jumps straight to ready', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'complete', cachedBytes: [1, 2, 3] })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('ready')
  })

  it('auto poll refreshes a stale ready candidate and downloads the newer version', async () => {
    checkMock
      .mockResolvedValueOnce(fakeUpdate('0.5.32'))
      .mockResolvedValueOnce(fakeUpdate('0.5.33'))
    getVersionMock.mockResolvedValue('0.5.31')
    const downloadedVersions: string[] = []
    invokeMock.mockImplementation(async (cmd: string, args?: { version?: string }) => {
      if (cmd === 'updater_platform_key') return 'darwin-aarch64'
      if (cmd === 'updater_check_cache') return { status: 'none', downloaded_size: 0 }
      if (cmd === 'updater_read_cached_bytes') return [1, 2, 3]
      if (cmd === 'updater_download') {
        downloadedVersions.push(args?.version ?? '')
        return undefined
      }
      if (cmd === 'updater_clear_cache') return undefined
      throw new Error('unexpected: ' + cmd)
    })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    await vi.waitFor(() => {
      expect(useUpdaterStore.getState().phase).toBe('ready')
      expect(useUpdaterStore.getState().version).toBe('0.5.32')
    })

    await useUpdaterStore.getState().bootstrap()
    await vi.waitFor(() => {
      expect(useUpdaterStore.getState().phase).toBe('ready')
      expect(useUpdaterStore.getState().version).toBe('0.5.33')
    })

    expect(downloadedVersions).toEqual(['0.5.32', '0.5.33'])
  })
})

describe('updaterStore.startDownload', () => {
  it('transitions to ready on successful download', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'none', cachedBytes: [7, 8, 9] })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()

    await vi.waitFor(() => {
      expect(useUpdaterStore.getState().phase).toBe('ready')
    })
  })

  it('transitions to failed on download error', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'none', downloadResolve: false })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    await vi.waitFor(() => {
      expect(useUpdaterStore.getState().phase).toBe('failed')
    })
  })

  it('refreshes stale failed candidate before direct startDownload calls', async () => {
    checkMock
      .mockResolvedValueOnce(fakeUpdate('0.5.32'))
      .mockResolvedValueOnce(fakeUpdate('0.5.33'))
    getVersionMock.mockResolvedValue('0.5.31')
    const downloadedVersions: string[] = []
    invokeMock.mockImplementation(async (cmd: string, args?: { version?: string }) => {
      if (cmd === 'updater_platform_key') return 'darwin-aarch64'
      if (cmd === 'updater_check_cache') return { status: 'none', downloaded_size: 0 }
      if (cmd === 'updater_read_cached_bytes') return [1, 2, 3]
      if (cmd === 'updater_download') {
        downloadedVersions.push(args?.version ?? '')
        if (args?.version === '0.5.32') throw new Error('first attempt failed')
        return undefined
      }
      if (cmd === 'updater_clear_cache') return undefined
      throw new Error('unexpected: ' + cmd)
    })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    await vi.waitFor(() => {
      expect(useUpdaterStore.getState().phase).toBe('failed')
    })

    await useUpdaterStore.getState().startDownload()

    expect(useUpdaterStore.getState().phase).toBe('ready')
    expect(useUpdaterStore.getState().version).toBe('0.5.33')
    expect(downloadedVersions).toEqual(['0.5.32', '0.5.33'])
  })
})

describe('updaterStore.retryDownload', () => {
  it('only runs from failed state', async () => {
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().retryDownload()
    expect(useUpdaterStore.getState().phase).toBe('idle')
  })

  it('retries from failed state', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    let downloadAttempts = 0
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'updater_platform_key') return 'darwin-aarch64'
      if (cmd === 'updater_check_cache') return { status: 'none', downloaded_size: 0 }
      if (cmd === 'updater_read_cached_bytes') return [1, 2, 3]
      if (cmd === 'updater_download') {
        downloadAttempts++
        if (downloadAttempts === 1) throw new Error('first attempt failed')
        return undefined
      }
      if (cmd === 'updater_clear_cache') return undefined
      throw new Error('unexpected: ' + cmd)
    })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    await vi.waitFor(() => {
      expect(useUpdaterStore.getState().phase).toBe('failed')
    })

    await useUpdaterStore.getState().retryDownload()
    expect(useUpdaterStore.getState().phase).toBe('ready')
    expect(downloadAttempts).toBe(2)
  })

  it('refreshes stale failed candidate before retrying download', async () => {
    checkMock
      .mockResolvedValueOnce(fakeUpdate('0.5.32'))
      .mockResolvedValueOnce(fakeUpdate('0.5.33'))
    getVersionMock.mockResolvedValue('0.5.31')
    const downloadedVersions: string[] = []
    invokeMock.mockImplementation(async (cmd: string, args?: { version?: string }) => {
      if (cmd === 'updater_platform_key') return 'darwin-aarch64'
      if (cmd === 'updater_check_cache') return { status: 'none', downloaded_size: 0 }
      if (cmd === 'updater_read_cached_bytes') return [1, 2, 3]
      if (cmd === 'updater_download') {
        downloadedVersions.push(args?.version ?? '')
        if (args?.version === '0.5.32') throw new Error('first attempt failed')
        return undefined
      }
      if (cmd === 'updater_clear_cache') return undefined
      throw new Error('unexpected: ' + cmd)
    })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    await vi.waitFor(() => {
      expect(useUpdaterStore.getState().phase).toBe('failed')
    })

    await useUpdaterStore.getState().retryDownload()

    expect(useUpdaterStore.getState().phase).toBe('ready')
    expect(useUpdaterStore.getState().version).toBe('0.5.33')
    expect(downloadedVersions).toEqual(['0.5.32', '0.5.33'])
  })
})

describe('updaterStore.installNow', () => {
  it('invokes updater_install_cached command with the version', async () => {
    const upd = fakeUpdate()
    checkMock.mockResolvedValue(upd)
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'complete', cachedBytes: [1, 2, 3] })
    relaunchMock.mockResolvedValue(undefined)
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('ready')

    await useUpdaterStore.getState().installNow()
    // install goes through our custom Tauri command (not the JS Update.install)
    const installCalls = invokeMock.mock.calls.filter((c) => c[0] === 'updater_install_cached')
    expect(installCalls.length).toBe(1)
    expect(installCalls[0][1]).toEqual({ version: '0.5.30' })
    expect(relaunchMock).toHaveBeenCalled()
  })

  it('refreshes stale ready candidate before installing', async () => {
    checkMock
      .mockResolvedValueOnce(fakeUpdate('0.5.32'))
      .mockResolvedValueOnce(fakeUpdate('0.5.33'))
    getVersionMock.mockResolvedValue('0.5.31')
    const downloadedVersions: string[] = []
    invokeMock.mockImplementation(async (cmd: string, args?: { version?: string }) => {
      if (cmd === 'updater_platform_key') return 'darwin-aarch64'
      if (cmd === 'updater_check_cache') {
        return {
          status: args?.version === '0.5.32' ? 'complete' : 'none',
          downloaded_size: args?.version === '0.5.32' ? 3 : 0,
        }
      }
      if (cmd === 'updater_read_cached_bytes') return [1, 2, 3]
      if (cmd === 'updater_download') {
        downloadedVersions.push(args?.version ?? '')
        return undefined
      }
      if (cmd === 'updater_clear_cache') return undefined
      if (cmd === 'updater_install_cached') return undefined
      throw new Error('unexpected: ' + cmd)
    })
    relaunchMock.mockResolvedValue(undefined)
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('ready')

    await useUpdaterStore.getState().installNow()

    const installCalls = invokeMock.mock.calls.filter((c) => c[0] === 'updater_install_cached')
    expect(installCalls).toEqual([])
    expect(downloadedVersions).toEqual(['0.5.33'])
    expect(useUpdaterStore.getState().version).toBe('0.5.33')
    expect(useUpdaterStore.getState().phase).toBe('ready')
    expect(relaunchMock).not.toHaveBeenCalled()
  })

  it('shows error toast when not ready', async () => {
    const { useUpdaterStore, useNotificationStore } = await loadModules()
    useNotificationStore.getState().dismissAll()
    await useUpdaterStore.getState().installNow()
    const notes = useNotificationStore.getState().notifications
    expect(notes.length).toBe(1)
    expect(notes[0].level).toBe('error')
  })
})

describe('updaterStore.bootstrap platform-key URL selection', () => {
  it('intel mac picks darwin-x86_64 url, not darwin-aarch64', async () => {
    checkMock.mockResolvedValue(fakeMultiPlatformUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'updater_platform_key') return 'darwin-x86_64'
      if (cmd === 'updater_check_cache') return { status: 'none', downloaded_size: 0 }
      if (cmd === 'updater_read_cached_bytes') return [1, 2, 3]
      if (cmd === 'updater_download') return undefined
      if (cmd === 'updater_clear_cache') return undefined
      throw new Error('unexpected: ' + cmd)
    })
    const { useUpdaterStore } = await loadModules()
    // Use manual trigger so bootstrap stops in 'available' without firing
    // startDownload — keeps the assertion focused on platform-key resolution.
    await useUpdaterStore.getState().bootstrap({ triggeredBy: 'manual' })

    const state = useUpdaterStore.getState() as unknown as { _downloadUrl: string }
    expect(state._downloadUrl).toBe('https://example/AIjia_x64.app.tar.gz')
  })

  it('apple silicon mac picks darwin-aarch64 url', async () => {
    checkMock.mockResolvedValue(fakeMultiPlatformUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'updater_platform_key') return 'darwin-aarch64'
      if (cmd === 'updater_check_cache') return { status: 'none', downloaded_size: 0 }
      throw new Error('unexpected: ' + cmd)
    })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap({ triggeredBy: 'manual' })

    const state = useUpdaterStore.getState() as unknown as { _downloadUrl: string }
    expect(state._downloadUrl).toBe('https://example/AIjia.app.tar.gz')
  })

  it('windows picks windows-x86_64 url', async () => {
    checkMock.mockResolvedValue(fakeMultiPlatformUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'updater_platform_key') return 'windows-x86_64'
      if (cmd === 'updater_check_cache') return { status: 'none', downloaded_size: 0 }
      throw new Error('unexpected: ' + cmd)
    })
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap({ triggeredBy: 'manual' })

    const state = useUpdaterStore.getState() as unknown as { _downloadUrl: string }
    expect(state._downloadUrl).toBe('https://example/AIjia_x64-setup.exe')
  })
})
