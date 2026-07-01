import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const loginMock = vi.fn()

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: (s: unknown) => unknown) => sel({
    login: loginMock,
    isAuthPending: false,
  }),
}))

vi.mock('@/stores/brandingStore', () => ({
  DEFAULTS: {
    productName: 'AI小家',
    productNameEn: 'AIjia',
  },
  useBrandingStore: (sel: (s: unknown) => unknown) => sel({
    productName: 'AI小家',
    productNameEn: 'AIjia',
    logoUrl: '/brand-avatar-gold.svg',
  }),
}))

import { LoginPage } from '../LoginPage'

describe('LoginPage legal documents', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
  })

  it('opens the local software license in an iframe dialog', () => {
    render(<LoginPage />)

    fireEvent.click(screen.getByRole('button', { name: '服务条款' }))

    expect(screen.getByRole('dialog', { name: 'AI小家软件许可及服务协议' })).toBeInTheDocument()
    const iframe = screen.getByTitle('AI小家软件许可及服务协议')
    expect(iframe).toHaveAttribute('sandbox', '')
    expect(iframe).toHaveAttribute('srcdoc', expect.stringContaining('AI小家软件许可及服务协议'))
  })

  it('opens the local privacy policy in an iframe dialog', () => {
    render(<LoginPage />)

    fireEvent.click(screen.getByRole('button', { name: '隐私政策' }))

    expect(screen.getByRole('dialog', { name: 'AI小家隐私政策' })).toBeInTheDocument()
    const iframe = screen.getByTitle('AI小家隐私政策')
    expect(iframe).toHaveAttribute('sandbox', '')
    expect(iframe).toHaveAttribute('srcdoc', expect.stringContaining('AI小家隐私政策'))
  })
})
