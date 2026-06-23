import '@testing-library/jest-dom'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  getLogLevel: vi.fn(),
  setLogLevel: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  uploadDiagnosticLogs: vi.fn(),
}))

const authMock = vi.hoisted(() => ({
  tenantType: 'personal',
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: (s: unknown) => unknown) =>
    sel({
      user: { name: '姚域权', username: 'yyq' },
      tenant: { name: '仁励家网络科技(杭州)有限公司', tenantType: authMock.tenantType },
      logout: vi.fn().mockResolvedValue(undefined),
    }),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import { useUiStore } from '@/stores/uiStore'
import { SettingsModal } from '../SettingsModal'

describe('SettingsModal', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    authMock.tenantType = 'personal'
    useUiStore.getState().closeSettings()
    tauriMock.getLogLevel.mockResolvedValue('info')
    tauriMock.setLogLevel.mockResolvedValue(undefined)
    tauriMock.getSettings.mockResolvedValue({})
    tauriMock.updateSettings.mockResolvedValue(undefined)
  })

  it('renders nothing when closed', () => {
    const { container } = render(<SettingsModal />)
    expect(container.firstChild).toBeNull()
  })

  it('renders menu and general panel content when account opened', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)
    expect(screen.getByRole('button', { name: '通用设置' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '账户与消耗' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '关于' })).toBeInTheDocument()
    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.queryByText('语言')).not.toBeInTheDocument()
    expect(screen.getByText('外观')).toBeInTheDocument()
  })

  it('switching to enabled menu changes the right panel', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)
    fireEvent.click(screen.getByRole('button', { name: '关于' }))
    expect(screen.getByRole('button', { name: '检查更新' })).toBeInTheDocument()
  })

  it('loads and persists the log level from the about panel', async () => {
    tauriMock.getLogLevel.mockResolvedValue('debug')
    useUiStore.getState().openSettings('about')
    render(<SettingsModal />)

    await waitFor(() => {
      expect(screen.getByRole('radio', { name: '调试' })).toHaveAttribute('aria-checked', 'true')
    })

    fireEvent.click(screen.getByRole('radio', { name: '警告' }))

    expect(tauriMock.getLogLevel).toHaveBeenCalledTimes(1)
    expect(tauriMock.setLogLevel).toHaveBeenCalledWith('warn')
  })

  it('does not render unavailable settings', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)

    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'MCP 服务（未开放）' })).not.toBeInTheDocument()
    expect(screen.queryByText(/MCP 服务 · 即将上线/)).not.toBeInTheDocument()
  })

  it('hides account billing for enterprise tenants', async () => {
    authMock.tenantType = 'enterprise'
    useUiStore.getState().openSettings('account')

    render(<SettingsModal />)

    expect(screen.getByRole('button', { name: '通用设置' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '账户与消耗' })).not.toBeInTheDocument()

    useUiStore.getState().openSettings('account-billing')
    await waitFor(() => {
      expect(useUiStore.getState().settingsModal).toBe('account')
    })
  })
})
