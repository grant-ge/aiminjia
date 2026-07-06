import '@testing-library/jest-dom'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  getLogLevel: vi.fn(),
  setLogLevel: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  uploadDiagnosticLogs: vi.fn(),
  getDevEnvironment: vi.fn(),
  setDevEnvironment: vi.fn(),
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
    vi.stubEnv('DEV', false)
    authMock.tenantType = 'personal'
    useUiStore.getState().closeSettings()
    tauriMock.getLogLevel.mockResolvedValue('info')
    tauriMock.setLogLevel.mockResolvedValue(undefined)
    tauriMock.getSettings.mockResolvedValue({})
    tauriMock.updateSettings.mockResolvedValue(undefined)
    tauriMock.getDevEnvironment.mockResolvedValue({
      currentTenant: 'https://ai-tenant.renlijia.com',
      currentOps: 'https://ai-ops.renlijia.com',
      isOverride: false,
      presets: [
        { key: 'test', tenant: 'https://test-ai-tenant.renlijia.com', ops: 'https://test-ai-ops.renlijia.com' },
        { key: 'pre', tenant: 'https://pre-ai-tenant.renlijia.com', ops: 'https://pre-ai-ops.renlijia.com' },
        { key: 'prod', tenant: 'https://ai-tenant.renlijia.com', ops: 'https://ai-ops.renlijia.com' },
      ],
    })
    tauriMock.setDevEnvironment.mockResolvedValue({
      currentTenant: 'https://test-ai-tenant.renlijia.com',
      currentOps: 'https://test-ai-ops.renlijia.com',
      isOverride: true,
      presets: [],
    })
  })

  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it('renders nothing when closed', () => {
    const { container } = render(<SettingsModal />)
    expect(container.firstChild).toBeNull()
  })

  it('renders menu and profile panel content when account opened', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)
    expect(screen.getByRole('button', { name: '个人资料' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '系统设置' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '账户与消耗' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '系统权限' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '环境切换' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '关于' })).toBeInTheDocument()
    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.getByText('头像、账号与组织信息。')).toBeInTheDocument()
    expect(screen.getByText('yyq')).toBeInTheDocument()
    expect(screen.queryByText('字体大小')).not.toBeInTheDocument()
  })

  it('shows system settings as a separate settings category', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)

    fireEvent.click(screen.getByRole('button', { name: '系统设置' }))

    expect(screen.getByRole('heading', { name: '界面设置', level: 3 })).toBeInTheDocument()
    expect(screen.getByText('界面设置')).toBeInTheDocument()
    expect(screen.getByText('字体大小')).toBeInTheDocument()
    expect(screen.getByText('聊天区域宽度')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: '运行设置', level: 3 })).toBeInTheDocument()
    expect(screen.getByText('运行设置')).toBeInTheDocument()
    expect(screen.getByText('运行时防止休眠')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: '系统权限', level: 3 })).toBeInTheDocument()
    expect(screen.getByText('系统权限')).toBeInTheDocument()
    expect(screen.getByText('默认访问')).toBeInTheDocument()
    expect(screen.queryByText('头像、账号与组织信息。')).not.toBeInTheDocument()
  })

  it('switching to enabled menu changes the right panel', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)
    fireEvent.click(screen.getByRole('button', { name: '关于' }))
    expect(screen.getByRole('button', { name: '检查更新' })).toBeInTheDocument()
  })

  it('shows a dev-only environment settings category', async () => {
    vi.stubEnv('DEV', true)
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)

    fireEvent.click(screen.getByRole('button', { name: '环境切换' }))

    const panel = screen.getByRole('heading', { name: '环境切换', level: 3 }).closest('section')
    expect(panel).not.toBeNull()
    await waitFor(() => {
      expect(within(panel as HTMLElement).getAllByText('生产').length).toBeGreaterThan(0)
    })
    expect(within(panel as HTMLElement).getByText('https://ai-tenant.renlijia.com')).toBeInTheDocument()
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

    expect(screen.getByRole('button', { name: '个人资料' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '系统设置' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '系统权限' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '账户与消耗' })).not.toBeInTheDocument()

    useUiStore.getState().openSettings('account-billing')
    await waitFor(() => {
      expect(useUiStore.getState().settingsModal).toBe('account')
    })
  })
})
