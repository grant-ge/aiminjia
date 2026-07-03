import { useEffect } from 'react'
import { useUpdaterStore } from '@/lib/updaterStore'

const DEFAULT_POLL_INTERVAL_MS = 60 * 60 * 1000
const PREVIEW_PHASES = new Set(['available', 'downloading', 'ready', 'failed', 'installing'])

function getPollIntervalMs(): number {
  const raw = import.meta.env.VITE_AIJIA_UPDATER_POLL_INTERVAL_MS
  const parsed = Number(raw)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_POLL_INTERVAL_MS
}

function applyDevPreviewState(): boolean {
  if (!import.meta.env.DEV) return false

  const queryPhase = new URLSearchParams(window.location.search).get('updaterPreview')
  const storedPhase = window.localStorage.getItem('aijia-updater-preview-phase')
  const phase = queryPhase || storedPhase
  if (!phase || !PREVIEW_PHASES.has(phase)) return false

  useUpdaterStore.setState({
    phase: phase as never,
    version: '0.5.99-preview',
    notes: '- 更新入口预览\n- 这是开发态模拟数据',
    progress: phase === 'downloading'
      ? { downloaded: 18 * 1024 * 1024, total: 42 * 1024 * 1024 }
      : phase === 'ready'
        ? { downloaded: 42 * 1024 * 1024, total: 42 * 1024 * 1024 }
        : null,
    installProgress: phase === 'installing'
      ? { stage: 'installing', current: 65, total: 100 }
      : null,
    error: phase === 'failed' ? '开发态预览：下载失败' : null,
    panelOpen: false,
    online: true,
    _update: null,
    _cachedBytes: null,
    _expectedSize: 42 * 1024 * 1024,
    _downloadUrl: '',
    _etag: 'preview',
    _devPreview: true,
    _bootstrapPromise: null,
    _downloadInFlight: null,
  })
  return true
}

export function useUpdater(): void {
  useEffect(() => {
    if (applyDevPreviewState()) return

    useUpdaterStore.setState({ _devPreview: false })

    const tick = () => { void useUpdaterStore.getState().bootstrap() }
    tick()
    const id = window.setInterval(tick, getPollIntervalMs())
    const onVisible = () => { if (document.visibilityState === 'visible') tick() }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      window.clearInterval(id)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [])
}
