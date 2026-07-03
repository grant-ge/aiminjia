import { create } from 'zustand'
import type { Update } from '@tauri-apps/plugin-updater'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { getVersion } from '@tauri-apps/api/app'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  updaterCheckCache,
  updaterDownload,
  updaterReadCachedBytes,
  updaterClearCache,
  updaterInstallCached,
  updaterPlatformKey,
} from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import i18n from '@/i18n'

type Phase = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'failed' | 'installing'
type InstallStage = 'preparing' | 'verifying' | 'installing' | 'finishing'

interface InstallProgress {
  stage: InstallStage
  current: number
  total: number
}

interface UpdaterState {
  phase: Phase
  version: string | null
  notes: string
  progress: { downloaded: number; total: number } | null
  installProgress: InstallProgress | null
  error: string | null
  panelOpen: boolean
  online: boolean
  _update: Update | null
  _cachedBytes: Uint8Array | null
  _expectedSize: number
  _downloadUrl: string
  _etag: string
  _devPreview: boolean
  _bootstrapPromise: Promise<void> | null
  _downloadInFlight: Promise<void> | null
  _progressUnlisten: UnlistenFn | null
  _failedUnlisten: UnlistenFn | null
  _installProgressUnlisten: UnlistenFn | null

  bootstrap(opts?: { triggeredBy?: 'auto' | 'manual' }): Promise<void>
  startDownload(): Promise<void>
  retryDownload(): Promise<void>
  openPanel(): void
  closePanel(): void
  installNow(): Promise<void>
}

let networkListenersInstalled = false

async function setupEventListeners(set: (partial: Partial<UpdaterState>) => void, get: () => UpdaterState) {
  if (get()._progressUnlisten) return
  let lastDownloaded = 0
  const progressUnlisten = await listen<{ version: string; downloaded: number; total: number }>(
    'updater:download-progress',
    (e) => {
      const { downloaded, total } = e.payload
      if (downloaded < lastDownloaded) {
        console.warn('[updater] progress went backwards:', lastDownloaded, '→', downloaded, 'phase=', get().phase)
      }
      lastDownloaded = downloaded
      set({ progress: { downloaded, total } })
    },
  )
  const failedUnlisten = await listen<{ version: string; error: string }>(
    'updater:download-failed',
    (e) => {
      console.warn('[updater] download-failed event:', e.payload.error)
      set({ phase: 'failed', error: e.payload.error })
    },
  )
  const installProgressUnlisten = await listen<{
    version: string
    stage: InstallStage
    current: number
    total: number
  }>(
    'updater:install-progress',
    (e) => {
      const { version, stage, current, total } = e.payload
      if (version !== get().version) return
      set({ installProgress: { stage, current, total } })
    },
  )
  set({
    _progressUnlisten: progressUnlisten,
    _failedUnlisten: failedUnlisten,
    _installProgressUnlisten: installProgressUnlisten,
  })
}

function extractUpdateMeta(
  update: Update,
  platformKey: string,
): { url: string; size: number; etag: string } {
  // Tauri Update doesn't expose the platform-resolved download URL directly —
  // only the raw update.json contents via `rawJson`. Pick the entry that
  // matches our compile-time {os}-{arch} (resolved Rust-side via cfg!), so
  // Intel mac builds get `darwin-x86_64`, not whatever entry is listed first.
  // navigator.userAgent is unreliable here: Apple Silicon's webview UA hides
  // arch, and Rosetta-translated processes lie about it. Trust the Rust side.
  const raw = (update as unknown as { rawJson?: { platforms?: Record<string, { url?: string }> } })?.rawJson
  let url = ''
  if (raw && raw.platforms) {
    const matched = raw.platforms[platformKey]?.url
    if (matched) {
      url = matched
    } else {
      console.warn('[updater] no manifest entry for', platformKey, '— falling back to first available')
      for (const platform of Object.values(raw.platforms)) {
        if (platform?.url) { url = platform.url; break }
      }
    }
  }
  return { url, size: 0, etag: '' }
}

type StoreSet = (partial: Partial<UpdaterState>) => void
type StoreGet = () => UpdaterState

async function refreshLatestCandidate(
  set: StoreSet,
  get: StoreGet,
): Promise<{ hasUpdate: boolean; changed: boolean }> {
  const previous = get()
  let update: Update | null = null
  try {
    update = await check()
  } catch (e) {
    console.warn('[updater] refresh check failed:', e)
    return { hasUpdate: Boolean(previous._update), changed: false }
  }

  if (!update) {
    set({
      phase: 'idle',
      version: null,
      notes: '',
      progress: null,
      installProgress: null,
      error: null,
      _update: null,
      _cachedBytes: null,
      _downloadUrl: '',
      _expectedSize: 0,
      _etag: '',
    })
    return { hasUpdate: false, changed: Boolean(previous._update) }
  }

  const current = await getVersion()
  if (update.version === current) {
    set({
      phase: 'idle',
      version: null,
      notes: '',
      progress: null,
      installProgress: null,
      error: null,
      _update: null,
      _cachedBytes: null,
      _downloadUrl: '',
      _expectedSize: 0,
      _etag: '',
    })
    return { hasUpdate: false, changed: Boolean(previous._update) }
  }

  let platformKey = ''
  try {
    platformKey = await updaterPlatformKey()
  } catch (e) {
    console.warn('[updater] platform key lookup failed:', e)
  }
  const { url, etag } = extractUpdateMeta(update, platformKey)
  const expectedSize = 0
  const changed = previous._update?.version !== update.version
    || previous._downloadUrl !== url
    || previous._etag !== etag

  set({
    _update: update,
    version: update.version,
    notes: update.body ?? '',
    _downloadUrl: url,
    _expectedSize: expectedSize,
    _etag: etag,
    _cachedBytes: null,
    progress: null,
    installProgress: null,
    error: null,
  })

  let cacheStatus: 'complete' | 'partial' | 'none' = 'none'
  let downloadedSize = 0
  try {
    const r = await updaterCheckCache(update.version, expectedSize, etag)
    cacheStatus = r.status
    downloadedSize = r.downloaded_size
  } catch (e) {
    console.warn('[updater] cache check failed:', e)
  }

  if (cacheStatus === 'complete') {
    try {
      const bytes = await updaterReadCachedBytes(update.version)
      set({
        phase: 'ready',
        _cachedBytes: new Uint8Array(bytes),
        progress: { downloaded: bytes.length, total: bytes.length },
      })
      return { hasUpdate: true, changed }
    } catch (e) {
      console.warn('[updater] read cached bytes failed:', e)
    }
  }

  set({
    phase: 'available',
    _cachedBytes: null,
    progress: cacheStatus === 'partial' ? { downloaded: downloadedSize, total: expectedSize } : null,
  })
  return { hasUpdate: true, changed }
}

export const useUpdaterStore = create<UpdaterState>()((set, get) => ({
  phase: 'idle',
  version: null,
  notes: '',
  progress: null,
  installProgress: null,
  error: null,
  panelOpen: false,
  online: typeof navigator !== 'undefined' ? navigator.onLine : true,
  _update: null,
  _cachedBytes: null,
  _expectedSize: 0,
  _downloadUrl: '',
  _etag: '',
  _devPreview: false,
  _bootstrapPromise: null,
  _downloadInFlight: null,
  _progressUnlisten: null,
  _failedUnlisten: null,
  _installProgressUnlisten: null,

  async bootstrap(opts?: { triggeredBy?: 'auto' | 'manual' }) {
    const triggeredBy = opts?.triggeredBy ?? 'auto'
    const inFlight = get()._bootstrapPromise
    if (inFlight) return inFlight

    const currentPhase = get().phase
    if (currentPhase === 'downloading' || currentPhase === 'installing') {
      return
    }
    if (currentPhase === 'available' || currentPhase === 'ready' || currentPhase === 'failed') {
      const refreshed = await refreshLatestCandidate(set, get)
      if (triggeredBy === 'auto' && refreshed.hasUpdate && refreshed.changed) {
        void get().startDownload()
      }
      return
    }

    let resolveHolder!: () => void
    const holder = new Promise<void>((r) => { resolveHolder = r })
    set({ _bootstrapPromise: holder })

    const run = (async () => {
      if (typeof navigator !== 'undefined' && !networkListenersInstalled) {
        networkListenersInstalled = true
        window.addEventListener('online', () => set({ online: true }))
        window.addEventListener('offline', () => set({ online: false }))
      }
      await setupEventListeners(set, get)

      set({ phase: 'checking', error: null })

      let update: Update | null = null
      try {
        update = await check()
      } catch (e) {
        console.warn('[updater] check failed:', e)
        set({ phase: 'idle' })
        return
      }
      if (!update) {
        set({ phase: 'idle', version: null, notes: '', progress: null, installProgress: null, _update: null, _cachedBytes: null })
        return
      }
      const current = await getVersion()
      if (update.version === current) {
        set({ phase: 'idle', version: null, notes: '', progress: null, installProgress: null, _update: null, _cachedBytes: null })
        return
      }

      let platformKey = ''
      try {
        platformKey = await updaterPlatformKey()
      } catch (e) {
        console.warn('[updater] platform key lookup failed:', e)
      }
      const { url, etag } = extractUpdateMeta(update, platformKey)
      const expectedSize = 0  // 0 = unknown (downloader treats as best-effort)

      set({
        _update: update,
        version: update.version,
        notes: update.body ?? '',
        _downloadUrl: url,
        _expectedSize: expectedSize,
        _etag: etag,
        error: null,
        installProgress: null,
      })

      // Check cache
      let cacheStatus: 'complete' | 'partial' | 'none' = 'none'
      try {
        const r = await updaterCheckCache(update.version, expectedSize, etag)
        cacheStatus = r.status
      } catch (e) {
        console.warn('[updater] cache check failed:', e)
      }

      if (cacheStatus === 'complete') {
        try {
          const bytes = await updaterReadCachedBytes(update.version)
          set({
            phase: 'ready',
            _cachedBytes: new Uint8Array(bytes),
            progress: { downloaded: bytes.length, total: bytes.length },
          })
          return
        } catch (e) {
          console.warn('[updater] read cached bytes failed:', e)
        }
      }

      // Partial cache means a download was interrupted previously. Always
      // continue regardless of the autoDownload setting — the user already
      // committed to downloading this version, we're just resuming. Showing
      // the available phase here would be confusing: clicking "立即更新" in
      // the dialog would jump to a non-zero progress and look broken.
      if (cacheStatus === 'partial') {
        void get().startDownload()
        return
      }

      // No cache. Auto trigger downloads silently in the background; manual
      // trigger stays in `available` so the user explicitly confirms in the
      // dialog before bytes start flowing — that's the whole point of the
      // manual path: not surprising the user with a download.
      if (triggeredBy === 'manual') {
        set({ phase: 'available' })
      } else {
        void get().startDownload()
      }
    })()

    try { await run } finally {
      resolveHolder()
      if (get()._bootstrapPromise === holder) set({ _bootstrapPromise: null })
    }
  },

  async startDownload() {
    if (get().phase === 'failed') {
      const refreshed = await refreshLatestCandidate(set, get)
      if (!refreshed.hasUpdate) return
      if (get().phase === 'ready') return
    }

    const { _update, _downloadUrl, _expectedSize, _etag, phase } = get()
    if (!_update || (phase !== 'available' && phase !== 'failed' && phase !== 'checking')) return
    if (!_downloadUrl) {
      set({ phase: 'failed', error: 'Download URL not available from update metadata' })
      return
    }
    // Dedup concurrent calls — guards against React StrictMode double-mount,
    // HMR re-runs, and parallel UI triggers. Without this two downloads append
    // to the same partial file in Rust and corrupt it.
    if (get()._downloadInFlight) {
      return get()._downloadInFlight!
    }

    set({
      phase: 'downloading',
      progress: { downloaded: 0, total: _expectedSize },
      installProgress: null,
      error: null,
      _cachedBytes: null,
    })

    const run = (async () => {
      try {
        await updaterDownload(_downloadUrl, _update.version, _expectedSize, _etag)
        const bytes = await updaterReadCachedBytes(_update.version)
        set({
          phase: 'ready',
          _cachedBytes: new Uint8Array(bytes),
          progress: { downloaded: bytes.length, total: bytes.length },
        })
      } catch (e) {
        const msg = String((e as Error)?.message ?? e)
        if (get().phase !== 'failed') {
          set({ phase: 'failed', error: msg })
        }
      } finally {
        set({ _downloadInFlight: null })
      }
    })()
    set({ _downloadInFlight: run })
    return run
  },

  async retryDownload() {
    if (get().phase !== 'failed') return
    const refreshed = await refreshLatestCandidate(set, get)
    if (!refreshed.hasUpdate) return
    if (get().phase === 'ready') return
    await get().startDownload()
  },

  openPanel() { set({ panelOpen: true }) },
  closePanel() { set({ panelOpen: false }) },

  async installNow() {
    const { _update, _cachedBytes, phase, online } = get()
    if (!_update || !_cachedBytes || phase !== 'ready') {
      useNotificationStore.getState().push({
        context: 'toast',
        level: 'error',
        title: i18n.t('updater.installFailedTitle'),
        message: i18n.t('updater.notReadyMessage'),
        actions: [], dismissible: true, autoHide: 6,
      })
      return
    }
    if (!online) {
      useNotificationStore.getState().push({
        context: 'toast',
        level: 'error',
        title: i18n.t('updater.installFailedTitle'),
        message: i18n.t('updater.offlineHint'),
        actions: [], dismissible: true, autoHide: 6,
      })
      return
    }

    const refreshed = await refreshLatestCandidate(set, get)
    if (!refreshed.hasUpdate) {
      useNotificationStore.getState().push({
        context: 'toast',
        level: 'error',
        title: i18n.t('updater.installFailedTitle'),
        message: i18n.t('updater.notReadyMessage'),
        actions: [], dismissible: true, autoHide: 6,
      })
      return
    }
    if (refreshed.changed || get().phase !== 'ready') {
      await get().startDownload()
      return
    }

    const latest = get()
    if (!latest._update || !latest._cachedBytes || latest.phase !== 'ready') {
      useNotificationStore.getState().push({
        context: 'toast',
        level: 'error',
        title: i18n.t('updater.installFailedTitle'),
        message: i18n.t('updater.notReadyMessage'),
        actions: [], dismissible: true, autoHide: 6,
      })
      return
    }

    set({
      phase: 'installing',
      installProgress: { stage: 'preparing', current: 1, total: 4 },
    })
    let installed = false
    try {
      // The JS plugin-updater's install() requires a prior JS-side download()
      // call (it tracks bytesRid internally). We have bytes in Rust cache,
      // so we go through our custom command that uses the Rust-side
      // Update::install(bytes) API directly.
      await updaterInstallCached(latest._update.version)
      installed = true
      await updaterClearCache().catch(() => {})
      set({ phase: 'idle', version: null, notes: '', progress: null, installProgress: null, _update: null, _cachedBytes: null })
      await relaunch()
    } catch (e) {
      console.error('[updater] install failed:', e)
      if (installed) {
        useNotificationStore.getState().push({
          context: 'toast', level: 'info',
          title: i18n.t('updater.installSuccessTitle'),
          message: i18n.t('updater.relaunchFailedHint'),
          actions: [], dismissible: true, autoHide: 10,
        })
        set({ phase: 'idle', version: null, notes: '', progress: null, installProgress: null, _update: null, _cachedBytes: null })
      } else {
        const msg = String((e as Error)?.message ?? e)
        const lower = msg.toLowerCase()
        if (lower.includes('signature') || lower.includes('version mismatch')) {
          await updaterClearCache().catch(() => {})
        }
        if (lower.includes('version mismatch')) {
          const refreshedAfterMismatch = await refreshLatestCandidate(set, get)
          if (refreshedAfterMismatch.hasUpdate && get().phase !== 'ready') {
            await get().startDownload()
          }
          useNotificationStore.getState().push({
            context: 'toast', level: 'error',
            title: i18n.t('updater.installFailedTitle'),
            message: msg,
            actions: [], dismissible: true, autoHide: 8,
          })
          return
        }
        useNotificationStore.getState().push({
          context: 'toast', level: 'error',
          title: i18n.t('updater.installFailedTitle'),
          message: msg,
          actions: [], dismissible: true, autoHide: 8,
        })
        set({ phase: 'failed', error: msg })
      }
    }
  },
}))
