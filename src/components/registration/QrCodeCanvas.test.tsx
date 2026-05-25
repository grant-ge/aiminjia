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

  it('shows the spinner overlay on top of the QR while loading=true (refresh)', async () => {
    render(<QrCodeCanvas value="https://example.com/qr-payload" loading={true} />)
    // 等 qrcode lib 把 dataURL 渲出来再断言 overlay,因为 overlay 现在只在
    // 已有 QR 之上显示(不再叠在 placeholder 上,避免双 spinner)。
    await screen.findByRole('img', { name: /二维码/ })
    await waitFor(() => {
      expect(screen.getByTestId('qr-spinner-overlay')).toBeInTheDocument()
    })
  })

  it('renders a loading placeholder when value is empty', () => {
    render(<QrCodeCanvas value="" loading={false} />)
    expect(screen.queryByRole('img', { name: /二维码/ })).not.toBeInTheDocument()
    expect(screen.getByTestId('qr-placeholder-loading')).toBeInTheDocument()
  })
})
