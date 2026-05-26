import { beforeEach, describe, expect, it } from 'vitest'

import { useNetworkStore } from './networkStore'

describe('networkStore', () => {
  beforeEach(() => {
    useNetworkStore.setState({
      status: 'unknown',
      lastOnlineAt: null,
      lastCheckAt: null,
      latencyMs: null,
      errorKind: null,
    })
  })

  it('starts in unknown state', () => {
    expect(useNetworkStore.getState().status).toBe('unknown')
  })

  it('applies online event', () => {
    useNetworkStore.getState().applyEvent({
      status: 'online',
      lastCheckAtMs: 1000,
      latencyMs: 42,
      errorKind: null,
    })
    const s = useNetworkStore.getState()
    expect(s.status).toBe('online')
    expect(s.lastOnlineAt).toBe(1000)
    expect(s.lastCheckAt).toBe(1000)
    expect(s.latencyMs).toBe(42)
    expect(s.errorKind).toBeNull()
  })

  it('applying offline does not overwrite lastOnlineAt', () => {
    useNetworkStore.getState().applyEvent({
      status: 'online',
      lastCheckAtMs: 1000,
      latencyMs: 42,
      errorKind: null,
    })
    useNetworkStore.getState().applyEvent({
      status: 'offline',
      lastCheckAtMs: 2000,
      latencyMs: null,
      errorKind: 'dns',
    })
    const s = useNetworkStore.getState()
    expect(s.status).toBe('offline')
    expect(s.lastOnlineAt).toBe(1000) // preserved
    expect(s.lastCheckAt).toBe(2000)
    expect(s.errorKind).toBe('dns')
  })

  it('server-degraded preserves lastOnlineAt', () => {
    useNetworkStore.getState().applyEvent({
      status: 'online',
      lastCheckAtMs: 1000,
      latencyMs: 42,
      errorKind: null,
    })
    useNetworkStore.getState().applyEvent({
      status: 'server-degraded',
      lastCheckAtMs: 2000,
      latencyMs: 88,
      errorKind: null,
    })
    expect(useNetworkStore.getState().lastOnlineAt).toBe(1000)
  })
})
