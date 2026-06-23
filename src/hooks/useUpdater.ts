import { useEffect } from 'react'
import { useUpdaterStore } from '@/lib/updaterStore'

const DEFAULT_POLL_INTERVAL_MS = 60 * 60 * 1000

function getPollIntervalMs(): number {
  const raw = import.meta.env.VITE_AIJIA_UPDATER_POLL_INTERVAL_MS
  const parsed = Number(raw)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_POLL_INTERVAL_MS
}

export function useUpdater(): void {
  useEffect(() => {
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
