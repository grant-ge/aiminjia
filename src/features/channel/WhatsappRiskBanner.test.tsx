import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useBrandingStore } from '@/stores/brandingStore'

import { WhatsappRiskBanner } from './WhatsappRiskBanner'

describe('WhatsappRiskBanner', () => {
  beforeEach(() => {
    useBrandingStore.getState().reset()
  })

  it('uses the shared dialog body and footer spacing', () => {
    render(<WhatsappRiskBanner open onAccept={vi.fn()} onCancel={vi.fn()} />)

    expect(screen.getByTestId('whatsapp-risk-dialog-body')).toHaveClass('px-6')
    expect(screen.getByTestId('whatsapp-risk-dialog-footer')).toHaveClass('px-6', 'pb-6')
  })

  it('uses tenant product name in WhatsApp risk copy', () => {
    useBrandingStore.setState({ productName: '小新助手' })

    render(<WhatsappRiskBanner open onAccept={vi.fn()} onCancel={vi.fn()} />)

    expect(screen.getByRole('dialog', { name: '关于 小新助手 接入 WhatsApp 的说明' })).toBeInTheDocument()
    expect(screen.getByText(/小新助手 接入 WhatsApp 当前采用/)).toBeInTheDocument()
    expect(screen.getByText(/把 小新助手 作为一台/)).toBeInTheDocument()
    expect(screen.getByText(/不在 小新助手 中群发/)).toBeInTheDocument()
  })
})
