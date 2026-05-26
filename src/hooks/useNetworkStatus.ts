import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'

import { TAURI_EVENTS, networkGetStatus } from '@/lib/tauri'
import type { NetworkStatusPayload } from '@/lib/tauri'
import { useNetworkStore } from '@/stores/networkStore'

/**
 * 挂载一次（App 顶层）。拉取启动初值 + 订阅 network:status event。
 */
export function useNetworkStatus() {
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | null = null

    void (async () => {
      try {
        const initial = await networkGetStatus()
        if (!cancelled && initial) {
          useNetworkStore.getState().applyEvent(initial)
        }
      } catch (err) {
        console.warn('[useNetworkStatus] initial fetch failed:', err)
      }

      try {
        const handle = await listen<NetworkStatusPayload>(
          TAURI_EVENTS.NETWORK_STATUS,
          (event) => {
            useNetworkStore.getState().applyEvent(event.payload)
          },
        )
        if (cancelled) {
          handle()
        } else {
          unlisten = handle
        }
      } catch (err) {
        console.warn('[useNetworkStatus] listen failed:', err)
      }
    })()

    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }
  }, [])
}
