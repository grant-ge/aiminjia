import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ChannelConfig } from './ChannelConfig'
import { useChannelStore } from '@/stores/channelStore'

const beginResult = {
  deviceCode: 'device-1',
  userCode: 'ABCD-EFGH-IJKL',
  verificationUriComplete:
    'https://open-dev.dingtalk.com/openapp/registration/openClaw?user_code=ABCD-EFGH-IJKL&source=OPEN_CLAW',
  verificationUri: 'https://open-dev.dingtalk.com/openapp/registration/openClaw?source=OPEN_CLAW',
  intervalSeconds: 30,
  expiresInSeconds: 7200,
  source: 'OPEN_CLAW',
}

const platformState = {
  platform: 'dingtalk' as const,
  capability: 'available' as const,
  configured: true,
  enabled: true,
  connection: 'connected' as const,
  config: {
    platform: 'dingtalk' as const,
    appKey: 'ding-app-key',
    appSecretMasked: '••••••••••••cret',
    robotCode: 'robot-code',
    robotCodeSource: 'registration' as const,
    source: 'OPEN_CLAW' as const,
    createdAt: '2026-05-07T00:00:00Z',
    updatedAt: '2026-05-07T00:00:00Z',
  },
  lastConnectedAt: null,
  lastError: null,
}

describe('ChannelConfig OPEN_CLAW registration', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useRealTimers()
    useChannelStore.setState({ platforms: {}, conversations: [], activeSessionId: null })
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('renders a QR-only first flow and starts OPEN_CLAW registration automatically', async () => {
    const beginRegistration = vi.fn().mockResolvedValue(beginResult)
    const pollRegistration = vi.fn().mockResolvedValue({ state: 'waiting' })
    useChannelStore.setState({ beginRegistration, pollRegistration })

    render(<ChannelConfig />)

    expect(screen.getByLabelText('钉钉扫码二维码')).toBeInTheDocument()
    expect(screen.queryByText('手动配置')).not.toBeInTheDocument()
    await waitFor(() => {
      expect(beginRegistration).toHaveBeenCalledWith('dingtalk')
    })
    expect(await screen.findByText(/等待你在钉钉页面完成创建/)).toBeInTheDocument()
  })

  it('can regenerate the QR code after a registration error', async () => {
    const beginRegistration = vi
      .fn()
      .mockRejectedValueOnce(new Error('network error'))
      .mockResolvedValueOnce(beginResult)
    const pollRegistration = vi.fn().mockResolvedValue({ state: 'waiting' })
    useChannelStore.setState({ beginRegistration, pollRegistration })

    render(<ChannelConfig />)

    expect(await screen.findByText('network error')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重新生成二维码' }))

    await waitFor(() => {
      expect(beginRegistration).toHaveBeenCalledTimes(2)
    })
    expect(await screen.findByText(/等待你在钉钉页面完成创建/)).toBeInTheDocument()
  })

  it('shows appKey, masked appSecret, and robotCode after polling succeeds', async () => {
    const onSaved = vi.fn()
    const beginRegistration = vi.fn().mockResolvedValue(beginResult)
    const pollRegistration = vi.fn().mockResolvedValue({
      state: 'success',
      config: platformState.config,
      platformState,
    })
    const setPlatformState = vi.fn()
    useChannelStore.setState({ beginRegistration, pollRegistration, setPlatformState })

    render(<ChannelConfig onSaved={onSaved} />)

    await waitFor(() => {
      expect(onSaved).toHaveBeenCalledTimes(1)
    })
    expect(setPlatformState).toHaveBeenCalledWith(platformState)
    expect(await screen.findByText('扫码开通成功')).toBeInTheDocument()
    expect(screen.getByText('AppKey')).toBeInTheDocument()
    expect(screen.getByText('ding-app-key')).toBeInTheDocument()
    expect(screen.getByText('AppSecret')).toBeInTheDocument()
    expect(screen.getByText('••••••••••••cret')).toBeInTheDocument()
    expect(screen.getByText('RobotCode')).toBeInTheDocument()
    expect(screen.getByText('robot-code')).toBeInTheDocument()
  })
})
