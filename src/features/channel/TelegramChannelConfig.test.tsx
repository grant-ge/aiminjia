import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/lib/tauri', () => ({
  channelTelegramSave: vi.fn(),
  channelTelegramRemove: vi.fn(),
  channelTelegramBeginPairing: vi.fn().mockResolvedValue({
    code: 'ABC12345',
    deepLink: 'https://t.me/test_bot?start=ABC12345',
    expiresInSeconds: 300,
    botUsername: 'test_bot',
  }),
  channelTelegramListPendingPairings: vi.fn().mockResolvedValue([]),
  channelTelegramListPairedUsers: vi.fn().mockResolvedValue([]),
  channelTelegramApprovePairing: vi.fn(),
  channelTelegramRejectPairing: vi.fn(),
  channelTelegramRevokeUser: vi.fn(),
  channelTelegramSetEnabled: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('qrcode', () => ({
  default: { toDataURL: vi.fn().mockResolvedValue('data:image/png;base64,xxx') },
}))
vi.mock('@/stores/channelStore', () => ({
  useChannelStore: (selector: (state: { platforms: Record<string, unknown>; setPlatformState: () => void }) => unknown) =>
    selector({ platforms: {}, setPlatformState: vi.fn() }),
}))
vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: (selector: (state: { push: () => void }) => unknown) => selector({ push: vi.fn() }),
}))
vi.mock('@/components/common/ConfirmDialogHost', () => ({
  requestConfirm: vi.fn().mockResolvedValue(true),
}))

import { TelegramChannelConfig } from './TelegramChannelConfig'
import { channelTelegramSave } from '@/lib/tauri'

describe('TelegramChannelConfig', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('disables 下一步 button when token is empty', () => {
    render(<TelegramChannelConfig />)
    const nextBtn = screen.getByText('下一步').closest('button') as HTMLButtonElement
    expect(nextBtn.disabled).toBe(true)
  })

  it('calls channelTelegramSave when 下一步 clicked with token', async () => {
    ;(channelTelegramSave as ReturnType<typeof vi.fn>).mockResolvedValue({
      platform: 'telegram',
      configured: true,
      enabled: true,
      capability: 'available',
      connection: 'connected',
      config: null,
    })
    render(<TelegramChannelConfig />)
    const input = screen.getByPlaceholderText(/123456789:/) as HTMLInputElement
    fireEvent.change(input, { target: { value: '123:abc' } })
    fireEvent.click(screen.getByText('下一步'))
    await waitFor(() => expect(channelTelegramSave).toHaveBeenCalledWith('123:abc'))
  })
})
