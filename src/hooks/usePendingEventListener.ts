import { useEffect } from 'react'

import {
  listenPendingDrained,
  listenPendingQueued,
  listenPendingRemoved,
  listenPendingSnapshot,
} from '@/lib/tauri'
import { usePendingStore } from '@/stores/pendingStore'

/** Mount once at App level. Subscribes to all 4 pending events and forwards to the store. */
export function usePendingEventListener(): void {
  useEffect(() => {
    const unlisteners: Array<Promise<() => void>> = []

    unlisteners.push(
      listenPendingSnapshot((p) => usePendingStore.getState().applySnapshot(p.sessionId, p.items)),
    )
    unlisteners.push(
      listenPendingQueued((p) => usePendingStore.getState().applyQueued(p.sessionId, p.item)),
    )
    unlisteners.push(
      listenPendingDrained((p) =>
        usePendingStore.getState().applyDrained(p.sessionId, p.drainedIds),
      ),
    )
    unlisteners.push(
      listenPendingRemoved((p) => usePendingStore.getState().applyRemoved(p.sessionId, p.itemId)),
    )

    return () => {
      unlisteners.forEach((p) => {
        p.then((fn) => fn()).catch(() => {})
      })
    }
  }, [])
}
