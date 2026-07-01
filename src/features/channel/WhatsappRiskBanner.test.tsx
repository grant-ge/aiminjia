import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { WhatsappRiskBanner } from './WhatsappRiskBanner'

describe('WhatsappRiskBanner', () => {
  it('uses the shared dialog body and footer spacing', () => {
    render(<WhatsappRiskBanner open onAccept={vi.fn()} onCancel={vi.fn()} />)

    expect(screen.getByTestId('whatsapp-risk-dialog-body')).toHaveClass('px-6')
    expect(screen.getByTestId('whatsapp-risk-dialog-footer')).toHaveClass('px-6', 'pb-6')
  })
})
