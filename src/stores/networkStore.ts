import { create } from 'zustand'

import { networkForceProbe } from '@/lib/tauri'
import type { NetworkErrorKind, NetworkStatus, NetworkStatusPayload } from '@/lib/tauri'

interface NetworkState {
  status: NetworkStatus | 'unknown'
  lastOnlineAt: number | null
  lastCheckAt: number | null
  latencyMs: number | null
  errorKind: NetworkErrorKind | null

  applyEvent: (payload: NetworkStatusPayload) => void
  forceProbe: () => Promise<void>
}

export const useNetworkStore = create<NetworkState>((set, get) => ({
  status: 'unknown',
  lastOnlineAt: null,
  lastCheckAt: null,
  latencyMs: null,
  errorKind: null,

  applyEvent: (payload) => {
    const prevOnlineAt = get().lastOnlineAt
    set({
      status: payload.status,
      lastCheckAt: payload.lastCheckAtMs,
      latencyMs: payload.latencyMs,
      errorKind: payload.errorKind,
      lastOnlineAt:
        payload.status === 'online' ? payload.lastCheckAtMs : prevOnlineAt,
    })
    if (payload.errorKind) {
      console.debug(
        '[networkStore] offline errorKind=%s lastCheckAtMs=%d',
        payload.errorKind,
        payload.lastCheckAtMs,
      )
    }
  },

  forceProbe: async () => {
    await networkForceProbe()
  },
}))
