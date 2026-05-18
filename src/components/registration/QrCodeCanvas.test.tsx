import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { QrCodeCanvas } from './QrCodeCanvas'

describe('QrCodeCanvas', () => {
  it('renders an <img> with a data URL after qrcode lib resolves', async () => {
    render(<QrCodeCanvas value="https://example.com/qr-payload" loading={false} />)
    const img = await screen.findByRole('img', { name: /二维码/ })
    await waitFor(() => {
      expect(img.getAttribute('src')).toMatch(/^data:image\/png;base64,/)
    })
  })

  it('shows the spinner overlay when loading=true', () => {
    render(<QrCodeCanvas value="https://example.com/qr-payload" loading={true} />)
    expect(screen.getByTestId('qr-spinner-overlay')).toBeInTheDocument()
  })

  it('renders the structural placeholder grid when value is empty', () => {
    render(<QrCodeCanvas value="" loading={false} />)
    expect(screen.queryByRole('img', { name: /二维码/ })).not.toBeInTheDocument()
    expect(screen.getByTestId('qr-placeholder-grid')).toBeInTheDocument()
  })
})
