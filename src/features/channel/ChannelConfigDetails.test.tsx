import '@testing-library/jest-dom'
import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmDialogHost, useConfirmDialogStore } from '@/components/common/ConfirmDialogHost'
import { useChannelStore } from '@/stores/channelStore'
import { ChannelConfigDetails } from './ChannelConfigDetails'

const config = {
  platform: 'dingtalk' as const,
  appKey: 'ding-app-key',
  appSecretMasked: '************cret',
  robotCode: 'robot-code',
  robotCodeSource: 'registration' as const,
  source: 'OPEN_CLAW' as const,
  createdAt: '2026-05-07T00:00:00Z',
  updatedAt: '2026-05-07T01:00:00Z',
}

describe('ChannelConfigDetails', () => {
  beforeEach(() => {
    useConfirmDialogStore.setState({ request: null })
    useChannelStore.setState({
      platforms: {},
      conversations: [],
      revealSecret: vi.fn().mockResolvedValue('plain-secret'),
    })
  })

  it('renders read-only fields without QR code or textboxes', () => {
    render(<ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />)

    expect(screen.getByText('钉钉配置')).toBeInTheDocument()
    expect(screen.getByText('ding-app-key')).toBeInTheDocument()
    expect(screen.getByText('************cret')).toBeInTheDocument()
    expect(screen.getByText('robot-code')).toBeInTheDocument()
    expect(screen.getByText('OPEN_CLAW')).toBeInTheDocument()
    expect(screen.getByText('2026-05-07T00:00:00Z')).toBeInTheDocument()
    expect(screen.getByText('2026-05-07T01:00:00Z')).toBeInTheDocument()
    expect(screen.queryByLabelText('钉钉扫码二维码')).not.toBeInTheDocument()
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
  })

  it('reveals AppSecret only after second confirmation', async () => {
    const revealSecret = vi.fn().mockResolvedValue('plain-secret')
    useChannelStore.setState({ revealSecret })

    render(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )

    await userEvent.click(screen.getByRole('button', { name: '显示 AppSecret' }))

    expect(revealSecret).not.toHaveBeenCalled()
    await userEvent.click(await screen.findByRole('button', { name: '确认显示' }))

    await waitFor(() => {
      expect(revealSecret).toHaveBeenCalledWith('dingtalk')
    })
    expect(await screen.findByText('plain-secret')).toBeInTheDocument()
  })

  it('does not reveal AppSecret when second confirmation is cancelled', async () => {
    const revealSecret = vi.fn().mockResolvedValue('plain-secret')
    useChannelStore.setState({ revealSecret })

    render(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )

    await userEvent.click(screen.getByRole('button', { name: '显示 AppSecret' }))
    await userEvent.click(await screen.findByRole('button', { name: '取消' }))

    expect(revealSecret).not.toHaveBeenCalled()
    expect(screen.queryByText('plain-secret')).not.toBeInTheDocument()
    expect(screen.getByText('************cret')).toBeInTheDocument()
  })

  it('clears revealed secret when closed', async () => {
    const onOpenChange = vi.fn()
    const revealSecret = vi.fn().mockResolvedValue('plain-secret')
    useChannelStore.setState({ revealSecret })

    const { rerender } = render(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={onOpenChange} />
        <ConfirmDialogHost />
      </>,
    )
    await userEvent.click(screen.getByRole('button', { name: '显示 AppSecret' }))
    await userEvent.click(await screen.findByRole('button', { name: '确认显示' }))
    expect(await screen.findByText('plain-secret')).toBeInTheDocument()

    rerender(
      <>
        <ChannelConfigDetails config={config} open={false} onOpenChange={onOpenChange} />
        <ConfirmDialogHost />
      </>,
    )
    rerender(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={onOpenChange} />
        <ConfirmDialogHost />
      </>,
    )

    expect(screen.queryByText('plain-secret')).not.toBeInTheDocument()
    expect(screen.getByText('************cret')).toBeInTheDocument()
  })

  it('does not restore revealed secret from an in-flight request after closing', async () => {
    let resolveSecret!: (secret: string) => void
    const revealSecret = vi.fn(
      () =>
        new Promise<string>((resolve) => {
          resolveSecret = resolve
        }),
    )
    useChannelStore.setState({ revealSecret })

    const { rerender } = render(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )
    await userEvent.click(screen.getByRole('button', { name: '显示 AppSecret' }))
    await userEvent.click(await screen.findByRole('button', { name: '确认显示' }))

    rerender(
      <>
        <ChannelConfigDetails config={config} open={false} onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )
    await act(async () => {
      resolveSecret('late-secret')
      await Promise.resolve()
    })
    rerender(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )

    await waitFor(() => {
      expect(screen.queryByText('late-secret')).not.toBeInTheDocument()
    })
    expect(screen.getByText('************cret')).toBeInTheDocument()
  })

  it('does not call revealSecret when confirmation resolves after the dialog closed', async () => {
    const revealSecret = vi.fn().mockResolvedValue('plain-secret')
    useChannelStore.setState({ revealSecret })

    const { rerender } = render(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )
    await userEvent.click(screen.getByRole('button', { name: '显示 AppSecret' }))
    expect(await screen.findByText('显示 AppSecret？')).toBeInTheDocument()

    rerender(
      <>
        <ChannelConfigDetails config={config} open={false} onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )
    await userEvent.click(screen.getByRole('button', { name: '确认显示' }))

    await act(async () => {
      await Promise.resolve()
    })
    expect(revealSecret).not.toHaveBeenCalled()

    rerender(
      <>
        <ChannelConfigDetails config={config} open onOpenChange={vi.fn()} />
        <ConfirmDialogHost />
      </>,
    )
    expect(screen.queryByText('plain-secret')).not.toBeInTheDocument()
    expect(screen.getByText('************cret')).toBeInTheDocument()
  })
})
