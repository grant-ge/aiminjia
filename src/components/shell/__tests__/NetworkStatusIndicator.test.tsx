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

  it('renders offline indicator when offline', () => {
    useNetworkStore.setState({ status: 'offline', errorKind: 'dns' })
    render(<NetworkStatusIndicator />)
    expect(screen.getByRole('button', { name: /network\.offlineBadge/i })).toBeInTheDocument()
  })

  it('renders degraded indicator when server-degraded', () => {
    useNetworkStore.setState({ status: 'server-degraded' })
    render(<NetworkStatusIndicator />)
    expect(screen.getByRole('button', { name: /network\.degradedBadge/i })).toBeInTheDocument()
  })

  it('calls forceProbe when retry button clicked', async () => {
    const forceProbe = vi.fn().mockResolvedValue(undefined)
    useNetworkStore.setState({ status: 'offline', forceProbe })
    render(<NetworkStatusIndicator />)
    fireEvent.click(screen.getByRole('button', { name: /network\.offlineBadge/i }))
    const retry = await screen.findByRole('button', { name: /network\.retryNow/i })
    fireEvent.click(retry)
    expect(forceProbe).toHaveBeenCalledTimes(1)
  })
})
