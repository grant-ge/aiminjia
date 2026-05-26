import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useNetworkStore } from '@/stores/networkStore'
import { useNotificationStore } from '@/stores/notificationStore'

import { useOfflineSendWarning } from './useOfflineSendWarning'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

describe('useOfflineSendWarning', () => {
  beforeEach(() => {
    useNetworkStore.setState({
      status: 'unknown',
      lastOnlineAt: null,
      lastCheckAt: null,
      latencyMs: null,
      errorKind: null,
    })
    useNotificationStore.setState({ notifications: [] })
  })

  it('does not push toast when online', () => {
    useNetworkStore.setState({ status: 'online' })
    const { result } = renderHook(() => useOfflineSendWarning())
    act(() => {
      result.current.warnIfOffline()
    })
    expect(useNotificationStore.getState().notifications).toHaveLength(0)
  })

  it('does not push toast when server-degraded (LLM error path handles it)', () => {
    useNetworkStore.setState({ status: 'server-degraded' })
    const { result } = renderHook(() => useOfflineSendWarning())
    act(() => {
      result.current.warnIfOffline()
    })
    expect(useNotificationStore.getState().notifications).toHaveLength(0)
  })

  it('pushes toast when offline', () => {
    useNetworkStore.setState({ status: 'offline' })
    const { result } = renderHook(() => useOfflineSendWarning())
    act(() => {
      result.current.warnIfOffline()
    })
    const notifs = useNotificationStore.getState().notifications
    expect(notifs).toHaveLength(1)
    expect(notifs[0].level).toBe('warning')
    expect(notifs[0].title).toBe('network.sendWhileOfflineTitle')
  })

  it('pushes once per call (caller decides cadence)', () => {
    useNetworkStore.setState({ status: 'offline' })
    const { result } = renderHook(() => useOfflineSendWarning())
    act(() => {
      result.current.warnIfOffline()
      result.current.warnIfOffline()
    })
    expect(useNotificationStore.getState().notifications).toHaveLength(2)
  })
})
