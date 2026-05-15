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
import { useNotificationStore } from '@/stores/notificationStore'
import i18n from '@/i18n'

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
  /** Internal: true only after this session's Update.download() has resolved. */
  _downloaded: boolean
  /** Internal: de-dupes React StrictMode/settings-triggered bootstraps. */
  _bootstrapPromise: Promise<void> | null

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
  // Currently unused (bootstrap clears unconditionally), kept for diagnostics
  // and future re-enablement. eslint-disable-next-line @typescript-eslint/no-unused-vars
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
void readPending

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

// Module-level flag: register online/offline listeners only once (#7).
let networkListenersInstalled = false

export const useUpdaterStore = create<UpdaterState>()((set, get) => ({
  phase: 'idle',
  version: null,
  notes: '',
  progress: null,
  panelOpen: false,
  online: typeof navigator !== 'undefined' ? navigator.onLine : true,
  _update: null,
  _downloaded: false,
  _bootstrapPromise: null,

  async bootstrap() {
    // Synchronous dedup check — must happen before the async IIFE starts (#2).
    const inFlight = get()._bootstrapPromise
    if (inFlight) {
      return inFlight
    }

    // Allocate the promise placeholder synchronously so concurrent callers
    // that arrive before the first `await` inside the IIFE see a non-null
    // `_bootstrapPromise` and de-dup.  We assign the actual promise to it
    // right after the IIFE expression (still in the same synchronous frame).
    let resolveHolder!: () => void
    const holder = new Promise<void>((r) => { resolveHolder = r })
    set({ _bootstrapPromise: holder })

    const run = (async () => {
      if (typeof navigator !== 'undefined' && !networkListenersInstalled) {
        networkListenersInstalled = true
        window.addEventListener('online', () => set({ online: true }))
        window.addEventListener('offline', () => set({ online: false }))
      }

      // pending.json from a prior session is unreliable: the Tauri Update handle
      // can't be persisted across launches, so even a "ready" status would mean
      // we'd have to re-download anyway. Worse, surfacing an old version (e.g.
      // 0.5.18 cached while the server is now on 0.5.20) confuses users and the
      // install button silently no-ops without an Update handle. Always start
      // from a clean slate and trust this session's check() result.
      await clearPending()

      let update: Update | null = null
      try {
        update = await check()
      } catch (e) {
        console.warn('[updater] check failed:', e)
        return
      }

      if (!update) {
        set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _downloaded: false })
        return
      }

      const currentVersion = await getVersion()
      if (update.version === currentVersion) {
        set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _downloaded: false })
        return
      }

      set({
        _update: update,
        version: update.version,
        notes: update.body ?? '',
        phase: 'downloading',
        progress: { downloaded: 0, total: 0 },
        _downloaded: false,
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
          _downloaded: true,
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
        set({ phase: 'failed', _downloaded: false })
        useNotificationStore.getState().push({
          context: 'toast',
          level: 'error',
          title: i18n.t('updater.downloadFailed'),
          message: i18n.t('updater.downloadFailedDesc'),
          actions: [],
          dismissible: true,
          autoHide: 8,
        })
      }
    })()

    try {
      await run
    } finally {
      resolveHolder()
      if (get()._bootstrapPromise === holder) {
        set({ _bootstrapPromise: null })
      }
    }
  },

  openPanel() {
    set({ panelOpen: true })
  },

  closePanel() {
    set({ panelOpen: false })
  },

  async installNow() {
    const { _update, _downloaded, phase, online } = get()
    if (!_update || !_downloaded || phase !== 'ready') {
      useNotificationStore.getState().push({
        context: 'toast',
        level: 'error',
        title: i18n.t('updater.installFailedTitle'),
        message: i18n.t('updater.notReadyMessage'),
        actions: [],
        dismissible: true,
        autoHide: 6,
      })
      return
    }
    // Guard: network may be required for signature verification (#5).
    if (!online) {
      useNotificationStore.getState().push({
        context: 'toast',
        level: 'error',
        title: i18n.t('updater.installFailedTitle'),
        message: i18n.t('updater.offlineHint'),
        actions: [],
        dismissible: true,
        autoHide: 6,
      })
      return
    }
    set({ phase: 'installing' })
    let installed = false
    try {
      try {
        await _update.install()
      } catch (e) {
        const msg = String((e as Error)?.message ?? e)
        if (msg.includes('called before Update.download')) {
          await _update.download((event) => {
            if (event.event === 'Progress') {
              const p = get().progress ?? { downloaded: 0, total: 0 }
              set({ progress: { downloaded: p.downloaded + event.data.chunkLength, total: p.total } })
            }
          })
          await _update.install()
        } else {
          throw e
        }
      }
      installed = true
      await clearPending()
      set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _downloaded: false })
      await relaunch()
    } catch (e) {
      console.error('[updater] install failed:', e)
      if (installed) {
        // Install succeeded but relaunch failed — tell user to restart manually (#6).
        useNotificationStore.getState().push({
          context: 'toast',
          level: 'info',
          title: i18n.t('updater.installSuccessTitle'),
          message: i18n.t('updater.relaunchFailedHint'),
          actions: [],
          dismissible: true,
          autoHide: 10,
        })
        set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _downloaded: false })
      } else {
        useNotificationStore.getState().push({
          context: 'toast',
          level: 'error',
          title: i18n.t('updater.installFailedTitle'),
          message: String((e as Error)?.message ?? e),
          actions: [],
          dismissible: true,
          autoHide: 8,
        })
        set({ phase: 'ready' })
      }
    }
  },
}))
