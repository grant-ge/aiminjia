import '@testing-library/jest-dom'
import { act, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RegistrationModal } from './RegistrationModal'

const noop = async () => 'waiting' as const

describe('RegistrationModal — state machine', () => {
  it('renders the title for mode="url"', () => {
    render(
      <RegistrationModal
        mode="url"
        title="配置钉钉"
        url="https://example.com/oauth?user_code=ABCD"
        userCode="ABCD-EFGH"
        expireSeconds={7200}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(screen.getByRole('heading', { name: /配置钉钉/ })).toBeInTheDocument()
  })

  it('renders the title for mode="qr_url"', () => {
    render(
      <RegistrationModal
        mode="qr_url"
        title="添加个人微信账号"
        qrUrl="https://ilink.weixin.qq.com/qr/abc"
        expireSeconds={120}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(screen.getByRole('heading', { name: /添加个人微信账号/ })).toBeInTheDocument()
  })
})

describe('RegistrationModal — countdown', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows mm:ss formatted remaining time', () => {
    render(
      <RegistrationModal
        mode="url"
        title="配置钉钉"
        url="https://x.test"
        expireSeconds={125}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    // 125s = 02:05
    expect(screen.getByTestId('registration-countdown')).toHaveTextContent('02:05')

    act(() => {
      vi.advanceTimersByTime(1000)
    })
    expect(screen.getByTestId('registration-countdown')).toHaveTextContent('02:04')
  })
})

describe('RegistrationModal — mode rendering', () => {
  it('mode="url" renders the URL link and userCode', async () => {
    render(
      <RegistrationModal
        mode="url"
        title="配置钉钉"
        url="https://example.com/oauth?user_code=ABCD-EFGH"
        userCode="ABCD-EFGH"
        expireSeconds={7200}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(screen.getByText('ABCD-EFGH')).toBeInTheDocument()
    const link = screen.getByRole('link', { name: /继续/ })
    expect(link).toHaveAttribute('href', 'https://example.com/oauth?user_code=ABCD-EFGH')
    expect(link).toHaveAttribute('target', '_blank')
    // QR <img> renders asynchronously once qrcode lib resolves the data URL.
    expect(await screen.findByRole('img', { name: /注册二维码/ })).toBeInTheDocument()
  })

  it('mode="qr_url" renders only the QR canvas, no URL link', async () => {
    render(
      <RegistrationModal
        mode="qr_url"
        title="添加个人微信账号"
        qrUrl="https://ilink.weixin.qq.com/qr/abc123"
        expireSeconds={120}
        pollState={noop}
        onConfirmed={vi.fn()}
        onCancel={vi.fn()}
      />,
    )
    expect(await screen.findByRole('img', { name: /注册二维码/ })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: /继续/ })).not.toBeInTheDocument()
  })
})

describe('RegistrationModal — polling state machine', () => {
  it('calls onConfirmed when pollState resolves "confirmed"', async () => {
    const onConfirmed = vi.fn()
    const pollState = vi.fn().mockResolvedValueOnce('confirmed' as const)
    render(
      <RegistrationModal
        mode="qr_url"
        title="t"
        qrUrl="https://x"
        expireSeconds={60}
        pollState={pollState}
        pollIntervalMs={50}
        onConfirmed={onConfirmed}
        onCancel={vi.fn()}
      />,
    )
    await vi.waitFor(() => expect(onConfirmed).toHaveBeenCalledTimes(1), { timeout: 1000 })
  })

  it('shows "expired" state and calls onCancel when pollState resolves "expired"', async () => {
    const onCancel = vi.fn()
    const pollState = vi.fn().mockResolvedValueOnce('expired' as const)
    render(
      <RegistrationModal
        mode="qr_url"
        title="t"
        qrUrl="https://x"
        expireSeconds={60}
        pollState={pollState}
        pollIntervalMs={50}
        onConfirmed={vi.fn()}
        onCancel={onCancel}
      />,
    )
    await vi.waitFor(() => expect(onCancel).toHaveBeenCalledTimes(1), { timeout: 1000 })
    expect(screen.getByText(/二维码已过期/)).toBeInTheDocument()
  })

  it('keeps polling while pollState returns "waiting"', async () => {
    const pollState = vi
      .fn()
      .mockResolvedValueOnce('waiting' as const)
      .mockResolvedValueOnce('waiting' as const)
      .mockResolvedValueOnce('confirmed' as const)
    const onConfirmed = vi.fn()
    render(
      <RegistrationModal
        mode="qr_url"
        title="t"
        qrUrl="https://x"
        expireSeconds={60}
        pollState={pollState}
        pollIntervalMs={20}
        onConfirmed={onConfirmed}
        onCancel={vi.fn()}
      />,
    )
    await vi.waitFor(() => expect(onConfirmed).toHaveBeenCalledTimes(1), { timeout: 2000 })
    expect(pollState).toHaveBeenCalledTimes(3)
  })
})
