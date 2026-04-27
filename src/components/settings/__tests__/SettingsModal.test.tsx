import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: (s: unknown) => unknown) =>
    sel({
      user: { name: '姚域权', username: 'yyq' },
      tenant: { name: '仁励家网络科技(杭州)有限公司' },
      logout: vi.fn().mockResolvedValue(undefined),
    }),
}))

import { useUiStore } from '@/stores/uiStore'
import { SettingsModal } from '../SettingsModal'

describe('SettingsModal', () => {
  beforeEach(() => useUiStore.getState().closeSettings())

  it('renders nothing when closed', () => {
    const { container } = render(<SettingsModal />)
    expect(container.firstChild).toBeNull()
  })

  it('renders menu and general panel content when account opened', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)
    expect(screen.getByRole('button', { name: '通用' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '关于 AI 小家' })).toBeInTheDocument()
    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.getByText('语言')).toBeInTheDocument()
    expect(screen.getByText('强调色')).toBeInTheDocument()
  })

  it('switching menu changes the right panel', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)
    fireEvent.click(screen.getByRole('button', { name: 'MCP 服务' }))
    expect(screen.getByText(/MCP 服务 · 即将上线/)).toBeInTheDocument()
  })
})
