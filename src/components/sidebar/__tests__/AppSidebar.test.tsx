import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    conversations: [],
    activeConversationId: null,
    switchConversation: vi.fn(),
    createNewConversation: vi.fn(),
  }),
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (sel: (s: unknown) => unknown) =>
    sel({
      route: { kind: 'home' },
      setRoute: vi.fn(),
      openSettings: vi.fn(),
    }),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: (s: unknown) => unknown) => sel({ user: null, tenant: null }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: (s: unknown) => unknown) =>
    sel({ productName: '仁励家网络科技(杭州)', logoUrl: '/app-icon.png' }),
}))

import { AppSidebar } from '../AppSidebar'

describe('AppSidebar', () => {
  it('has sidebar background and 256 px width', () => {
    const { container } = render(<AppSidebar />)
    const aside = container.querySelector('aside')
    expect(aside?.className).toMatch(/w-\[256px\]/)
    expect(aside?.className).toMatch(/bg-sidebar/)
  })

  it('renders TenantHeader name', () => {
    render(<AppSidebar />)
    expect(screen.getByText('仁励家网络科技(杭州)')).toBeInTheDocument()
  })

  it('renders 3 nav items, the section title 项目, and footer 设置', () => {
    render(<AppSidebar />)
    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
    expect(screen.getByText('项目')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument()
  })

  it('renders a top drag-region spacer on macOS', async () => {
    // isMac is evaluated at module-load time, so we need to reset modules and
    // re-import with a mocked userAgent.
    const orig = Object.getOwnPropertyDescriptor(navigator, 'userAgent')
    Object.defineProperty(navigator, 'userAgent', {
      value: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)',
      configurable: true,
    })
    vi.resetModules()
    const { AppSidebar: MacSidebar } = await import('../AppSidebar')
    const { container } = render(<MacSidebar />)
    expect(container.querySelector('[data-tauri-drag-region]')).toBeInTheDocument()
    if (orig) Object.defineProperty(navigator, 'userAgent', orig)
    vi.resetModules()
  })
})
