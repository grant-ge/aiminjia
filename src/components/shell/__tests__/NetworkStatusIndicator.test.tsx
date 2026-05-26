import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useNetworkStore } from '@/stores/networkStore'

import { NetworkStatusIndicator } from '../NetworkStatusIndicator'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

describe('NetworkStatusIndicator', () => {
  beforeEach(() => {
    useNetworkStore.setState({
      status: 'unknown',
      lastOnlineAt: null,
      lastCheckAt: null,
      latencyMs: null,
      errorKind: null,
    })
  })

  it('renders nothing when status is unknown', () => {
    const { container } = render(<NetworkStatusIndicator />)
    expect(container.firstChild).toBeNull()
  })

  it('renders nothing when online', () => {
    useNetworkStore.setState({ status: 'online' })
    const { container } = render(<NetworkStatusIndicator />)
    expect(container.firstChild).toBeNull()
  })

  it('renders banner when offline', () => {
    useNetworkStore.setState({ status: 'offline', errorKind: 'dns' })
    render(<NetworkStatusIndicator />)
    const banner = screen.getByRole('alert')
    expect(banner).toBeInTheDocument()
    expect(banner).toHaveTextContent('network.bannerOfflineText')
  })

  it('renders banner when server-degraded', () => {
    useNetworkStore.setState({ status: 'server-degraded' })
    render(<NetworkStatusIndicator />)
    const banner = screen.getByRole('alert')
    expect(banner).toBeInTheDocument()
    expect(banner).toHaveTextContent('network.bannerDegradedText')
  })

  it('calls forceProbe when retry button clicked', () => {
    const forceProbe = vi.fn().mockResolvedValue(undefined)
    useNetworkStore.setState({ status: 'offline', forceProbe })
    render(<NetworkStatusIndicator />)
    const retryBtn = screen.getByRole('button', { name: 'network.retryNow' })
    fireEvent.click(retryBtn)
    expect(forceProbe).toHaveBeenCalledTimes(1)
  })
})
