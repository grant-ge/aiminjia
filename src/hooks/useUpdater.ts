import { useEffect } from 'react'
import { useUpdaterStore } from '@/lib/updaterStore'

const POLL_INTERVAL_MS = 60 * 60 * 1000

export function useUpdater(): void {
  useEffect(() => {
    const tick = () => { void useUpdaterStore.getState().bootstrap() }
    tick()
    const id = window.setInterval(tick, POLL_INTERVAL_MS)
    const onVisible = () => { if (document.visibilityState === 'visible') tick() }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      window.clearInterval(id)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [])
}
