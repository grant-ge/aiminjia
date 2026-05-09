import { create } from 'zustand'
import type { Update } from '@tauri-apps/plugin-updater'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { getVersion } from '@tauri-apps/api/app'
import { appCacheDir, join } from '@tauri-apps/api/path'
import {
  exists,
  readTextFile,
  writeTextFile,
  remove,
  mkdir,
} from '@tauri-apps/plugin-fs'

type Phase = 'idle' | 'downloading' | 'ready' | 'failed' | 'installing'

interface PendingMeta {
  version: string
  totalBytes: number
  downloadedBytes: number
  status: 'downloading' | 'ready' | 'failed'
  checkedAt: string
  readyAt?: string
}

interface UpdaterState {
  phase: Phase
  version: string | null
  notes: string
  progress: { downloaded: number; total: number } | null
  panelOpen: boolean
  online: boolean
  /** Internal: live Update handle. Not persisted. */
  _update: Update | null

  bootstrap(): Promise<void>
  openPanel(): void
  closePanel(): void
  installNow(): Promise<void>
}

const PENDING_DIRNAME = 'updater'
const PENDING_FILENAME = 'pending.json'

async function pendingPath(): Promise<{ dir: string; file: string }> {
  const cache = await appCacheDir()
  const dir = await join(cache, PENDING_DIRNAME)
  const file = await join(dir, PENDING_FILENAME)
  return { dir, file }
}

async function readPending(): Promise<PendingMeta | null> {
  try {
    const { file } = await pendingPath()
    if (!(await exists(file))) return null
    const text = await readTextFile(file)
    const meta = JSON.parse(text) as PendingMeta
    if (!meta.version || !meta.status) return null
    return meta
  } catch {
    return null
  }
}

async function writePending(meta: PendingMeta): Promise<void> {
  const { dir, file } = await pendingPath()
  try {
    await mkdir(dir, { recursive: true })
  } catch {
    /* dir may already exist */
  }
  await writeTextFile(file, JSON.stringify(meta, null, 2))
}

async function clearPending(): Promise<void> {
  try {
    const { file } = await pendingPath()
    if (await exists(file)) {
      await remove(file)
    }
  } catch {
    /* best-effort */
  }
}

export const useUpdaterStore = create<UpdaterState>()((set, get) => ({
  phase: 'idle',
  version: null,
  notes: '',
  progress: null,
  panelOpen: false,
  online: typeof navigator !== 'undefined' ? navigator.onLine : true,
  _update: null,

  async bootstrap() {
    if (typeof navigator !== 'undefined') {
      window.addEventListener('online', () => set({ online: true }))
      window.addEventListener('offline', () => set({ online: false }))
    }

    const prior = await readPending()
    if (prior && prior.status !== 'ready') {
      // downloading or failed → discard
      await clearPending()
    } else if (prior && prior.status === 'ready') {
      // surface the ready link immediately so the user sees it on launch
      set({
        phase: 'ready',
        version: prior.version,
        progress: { downloaded: prior.downloadedBytes, total: prior.totalBytes },
      })
    }

    let update: Update | null = null
    try {
      update = await check()
    } catch (e) {
      console.warn('[updater] check failed:', e)
      // Offline + prior ready → keep ready phase, but install will fail without an Update handle
      return
    }

    if (!update) {
      if (prior) await clearPending()
      set({ phase: 'idle', version: null, notes: '', progress: null, _update: null })
      return
    }

    const currentVersion = await getVersion()
    if (update.version === currentVersion) {
      if (prior) await clearPending()
      set({ phase: 'idle', version: null, notes: '', progress: null, _update: null })
      return
    }

    if (prior && prior.status === 'ready' && prior.version !== update.version) {
      // remote moved on; discard old pending and re-download new version
      await clearPending()
      set({ phase: 'idle', version: null, notes: '', progress: null })
    }

    set({ _update: update, version: update.version, notes: update.body ?? '' })

    // start download (or re-fill in-process bytes if prior was ready)
    set({
      phase: 'downloading',
      progress: { downloaded: 0, total: 0 },
    })
    await writePending({
      version: update.version,
      totalBytes: 0,
      downloadedBytes: 0,
      status: 'downloading',
      checkedAt: new Date().toISOString(),
    })

    let total = 0
    let downloaded = 0
    try {
      await update.download((event) => {
        if (event.event === 'Started') {
          total = event.data.contentLength ?? 0
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength
          set({ progress: { downloaded, total } })
        }
      })
      const readyAt = new Date().toISOString()
      await writePending({
        version: update.version,
        totalBytes: total,
        downloadedBytes: downloaded,
        status: 'ready',
        checkedAt: new Date().toISOString(),
        readyAt,
      })
      set({
        phase: 'ready',
        progress: { downloaded, total },
      })
    } catch (e) {
      console.warn('[updater] download failed:', e)
      await writePending({
        version: update.version,
        totalBytes: total,
        downloadedBytes: downloaded,
        status: 'failed',
        checkedAt: new Date().toISOString(),
      })
      set({ phase: 'failed' })
    }
  },

  openPanel() {
    set({ panelOpen: true })
  },

  closePanel() {
    set({ panelOpen: false })
  },

  async installNow() {
    const { _update } = get()
    if (!_update) {
      console.warn('[updater] installNow called without an Update handle (likely offline)')
      return
    }
    set({ phase: 'installing' })
    try {
      await _update.install()
      await clearPending()
      await relaunch()
    } catch (e) {
      console.error('[updater] install failed:', e)
      set({ phase: 'ready' })
    }
  },
}))
