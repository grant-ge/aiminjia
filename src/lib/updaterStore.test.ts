import { afterEach, describe, expect, it, vi } from 'vitest'

// Mock Tauri APIs that the store imports at module load time.
const checkMock = vi.fn()
const relaunchMock = vi.fn()
const getVersionMock = vi.fn()
const existsMock = vi.fn().mockResolvedValue(false)
const readTextFileMock = vi.fn()
const writeTextFileMock = vi.fn().mockResolvedValue(undefined)
const removeMock = vi.fn().mockResolvedValue(undefined)
const mkdirMock = vi.fn().mockResolvedValue(undefined)
const appCacheDirMock = vi.fn().mockResolvedValue('/tmp/cache')
const joinMock = vi.fn(async (...parts: string[]) => parts.join('/'))

vi.mock('@tauri-apps/plugin-updater', () => ({ check: (...a: unknown[]) => checkMock(...a) }))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: (...a: unknown[]) => relaunchMock(...a) }))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: (...a: unknown[]) => getVersionMock(...a) }))
vi.mock('@tauri-apps/api/path', () => ({
  appCacheDir: (...a: unknown[]) => appCacheDirMock(...a),
  join: (...a: unknown[]) => joinMock(...(a as string[])),
}))
vi.mock('@tauri-apps/plugin-fs', () => ({
  exists: (...a: unknown[]) => existsMock(...a),
  readTextFile: (...a: unknown[]) => readTextFileMock(...a),
  writeTextFile: (...a: unknown[]) => writeTextFileMock(...a),
  remove: (...a: unknown[]) => removeMock(...a),
  mkdir: (...a: unknown[]) => mkdirMock(...a),
}))

// Reset modules so each test imports a fresh store with reset zustand state.
async function loadModules() {
  vi.resetModules()
  const storeMod = await import('./updaterStore')
  const notifMod = await import('@/stores/notificationStore')
  return { useUpdaterStore: storeMod.useUpdaterStore, useNotificationStore: notifMod.useNotificationStore }
}

afterEach(() => {
  vi.clearAllMocks()
  existsMock.mockResolvedValue(false)
})

describe('updaterStore.bootstrap', () => {
  it('does not surface stale prior-ready cache without a live Update handle', async () => {
    // Simulate prior session that left "ready" cache for old version 0.5.18
    existsMock.mockResolvedValue(true)
    readTextFileMock.mockResolvedValue(JSON.stringify({
      version: '0.5.18',
      totalBytes: 1000,
      downloadedBytes: 1000,
      status: 'ready',
      checkedAt: '2026-05-09T00:00:00Z',
      readyAt: '2026-05-09T00:00:00Z',
    }))
    // No live update available now
    checkMock.mockResolvedValue(null)
    getVersionMock.mockResolvedValue('0.5.20')

    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()
    const s = useStore.getState()

    expect(s.phase).toBe('idle')
    expect(s.version).toBeNull()
    // The stale pending.json must have been cleared, not surfaced
    expect(removeMock).toHaveBeenCalled()
  })

  it('downloads and exposes the latest server version, not the stale cache', async () => {
    existsMock.mockResolvedValue(true)
    readTextFileMock.mockResolvedValue(JSON.stringify({
      version: '0.5.18',
      totalBytes: 100, downloadedBytes: 100, status: 'ready',
      checkedAt: '2026-05-09T00:00:00Z',
    }))
    const fakeUpdate = {
      version: '0.5.21',
      body: 'Fixes',
      download: vi.fn(async (cb: (e: { event: string; data: { contentLength?: number; chunkLength?: number } }) => void) => {
        cb({ event: 'Started', data: { contentLength: 200 } })
        cb({ event: 'Progress', data: { chunkLength: 200 } })
      }),
      install: vi.fn(),
    }
    checkMock.mockResolvedValue(fakeUpdate)
    getVersionMock.mockResolvedValue('0.5.18')

    const { useUpdaterStore: useStore } = await loadModules()
    await useStore.getState().bootstrap()
    const s = useStore.getState()

    expect(s.version).toBe('0.5.21')
    expect(s.phase).toBe('ready')
  })
})

describe('updaterStore.installNow', () => {
  it('pushes a user-visible toast when no Update handle is present', async () => {
    const { useUpdaterStore: useStore, useNotificationStore } = await loadModules()
    useNotificationStore.getState().dismissAll()
    // No bootstrap → no _update handle
    await useStore.getState().installNow()

    const notes = useNotificationStore.getState().notifications
    expect(notes.length).toBe(1)
    expect(notes[0].level).toBe('error')
    expect(notes[0].context).toBe('toast')
  })

  it('pushes a toast and reverts to ready when install() throws', async () => {
    const fakeUpdate = {
      version: '0.5.21',
      body: '',
      download: vi.fn(async (cb: (e: { event: string; data: { contentLength?: number; chunkLength?: number } }) => void) => {
        cb({ event: 'Started', data: { contentLength: 1 } })
        cb({ event: 'Progress', data: { chunkLength: 1 } })
      }),
      install: vi.fn().mockRejectedValue(new Error('disk full')),
    }
    checkMock.mockResolvedValue(fakeUpdate)
    getVersionMock.mockResolvedValue('0.5.18')

    const { useUpdaterStore: useStore, useNotificationStore } = await loadModules()
    useNotificationStore.getState().dismissAll()
    await useStore.getState().bootstrap()
    await useStore.getState().installNow()

    const notes = useNotificationStore.getState().notifications
    expect(notes.length).toBeGreaterThanOrEqual(1)
    expect(notes[notes.length - 1].level).toBe('error')
    expect(useStore.getState().phase).toBe('ready')
  })
})
