import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  TAURI_EVENTS: {
    SKILL_REGISTRY_REFRESHED: 'skill:registry-refreshed',
  },
  cloudLogin: vi.fn().mockResolvedValue({
    loggedIn: true,
    user: { id: 1, name: 'Test', username: 'test' },
    tenant: { id: 2, name: 'Tenant', balance: '0', accentColor: '#DBAA22' },
    models: [{ id: 'glm', name: 'GLM', modelType: 'cloud' }],
  }),
  getCloudAuth: vi.fn().mockResolvedValue({
    loggedIn: false,
    user: null,
    tenant: null,
    models: [],
  }),
  getCloudModels: vi.fn().mockResolvedValue([]),
  getSettings: vi.fn().mockResolvedValue({
    primaryModel: 'qwen-plus',
    primaryApiKey: '',
    autoModelRouting: true,
    workspacePath: '',
    analysisThreshold: 1.65,
    autoCleanupEnabled: true,
    tempFileRetentionDays: 7,
    keepOldVersions: 1,
    customModelEndpoint: '',
    customModelName: '',
    cloudModel: '',
    cloudModelType: '',
  }),
  updateSettings: vi.fn().mockResolvedValue(undefined),
  getDevGateway: vi.fn().mockResolvedValue({
    currentHost: 'https://ai.renlijia.com',
    isOverride: false,
    presets: [],
  }),
  setDevGateway: vi.fn().mockResolvedValue({
    currentHost: 'https://ai.renlijia.com',
    isOverride: false,
    presets: [],
  }),
  getConversations: vi.fn().mockResolvedValue([]),
  isAgentBusy: vi.fn().mockResolvedValue([]),
  cloudLogout: vi.fn().mockResolvedValue(undefined),
  cloudSendSmsCode: vi.fn().mockResolvedValue(undefined),
  cloudSendEmailCode: vi.fn().mockResolvedValue(undefined),
  cloudRegister: vi.fn().mockResolvedValue(undefined),
  cloudResetPassword: vi.fn().mockResolvedValue(undefined),
  syncBuiltinSkills: vi.fn().mockResolvedValue({ installed: [], skipped: [] }),
  workplaceDirectoryCatalog: vi.fn().mockResolvedValue({ schemaVersion: 1, categories: [], items: [] }),
  getLastBrand: vi.fn().mockResolvedValue(null),
  saveLastBrand: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import { AuthGate } from '@/components/auth/AuthGate'
import { useAuthStore } from '@/stores/authStore'
import { useUiStore } from '@/stores/uiStore'

describe('AuthGate', () => {
  beforeEach(() => {
    tauriMock.getCloudAuth.mockResolvedValue({
      loggedIn: false,
      user: null,
      tenant: null,
      models: [],
    })
    tauriMock.getCloudModels.mockResolvedValue([])
    tauriMock.getSettings.mockResolvedValue({
      primaryModel: 'qwen-plus',
      primaryApiKey: '',
      autoModelRouting: true,
      workspacePath: '',
      analysisThreshold: 1.65,
      autoCleanupEnabled: true,
      tempFileRetentionDays: 7,
      keepOldVersions: 1,
      customModelEndpoint: '',
      customModelName: '',
      cloudModel: '',
      cloudModelType: '',
    })
    tauriMock.updateSettings.mockResolvedValue(undefined)
    tauriMock.getConversations.mockResolvedValue([])
    tauriMock.isAgentBusy.mockResolvedValue([])
    tauriMock.cloudSendSmsCode.mockResolvedValue(undefined)
    tauriMock.cloudSendEmailCode.mockResolvedValue(undefined)
    tauriMock.cloudRegister.mockResolvedValue(undefined)
    tauriMock.cloudResetPassword.mockResolvedValue(undefined)
    tauriMock.syncBuiltinSkills.mockResolvedValue({ installed: [], skipped: [] })
    tauriMock.workplaceDirectoryCatalog.mockResolvedValue({ schemaVersion: 1, categories: [], items: [] })
    tauriMock.workplaceDirectoryCatalog.mockClear()

    useAuthStore.setState({
      isLoggedIn: false,
      user: null,
      tenant: null,
      cloudModels: [],
      selectedCloudModel: null,
      redirectFrom: { kind: 'skill-center' },
      isAuthPending: false,
    })
    useUiStore.setState({ route: { kind: 'home' }, settingsModal: null })
  })

  it('未登录时渲染 LoginPage', async () => {
    render(
      <AuthGate>
        <div>APP SHELL</div>
      </AuthGate>,
    )

    expect(await screen.findByRole('button', { name: '登录' })).toBeInTheDocument()
    expect(screen.queryByText('APP SHELL')).not.toBeInTheDocument()
  })

  it('登录成功后恢复 redirectFrom', async () => {
    render(
      <AuthGate>
        <div>APP SHELL</div>
      </AuthGate>,
    )

    fireEvent.change(await screen.findByLabelText('账号'), { target: { value: 'demo' } })
    fireEvent.change(await screen.findByLabelText('密码'), { target: { value: '123456' } })
    fireEvent.click(screen.getByRole('button', { name: '登录' }))

    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-center' })
    })
  })

  it('登录密码错误时留在登录页并展示错误提示', async () => {
    tauriMock.cloudLogin.mockRejectedValueOnce(new Error('用户名或密码错误'))

    render(
      <AuthGate>
        <div>APP SHELL</div>
      </AuthGate>,
    )

    fireEvent.change(await screen.findByLabelText('账号'), { target: { value: 'demo@org' } })
    fireEvent.change(screen.getByLabelText('密码'), { target: { value: 'wrong-password' } })
    fireEvent.click(screen.getByRole('button', { name: '登录' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('用户名或密码错误')
    expect(screen.queryByTestId('fullscreen-loader')).not.toBeInTheDocument()
    expect(screen.queryByText('APP SHELL')).not.toBeInTheDocument()
  })

  it('个人手机号注册完成后使用同一凭证自动登录', async () => {
    render(
      <AuthGate>
        <div>APP SHELL</div>
      </AuthGate>,
    )

    fireEvent.click(await screen.findByRole('button', { name: '立即注册' }))
    fireEvent.change(screen.getByLabelText('手机号'), { target: { value: '13800138000' } })
    fireEvent.click(screen.getByRole('button', { name: '获取验证码' }))

    await waitFor(() => {
      expect(tauriMock.cloudSendSmsCode).toHaveBeenCalledWith('13800138000')
    })

    fireEvent.change(screen.getByLabelText('验证码'), { target: { value: '123456' } })
    fireEvent.change(screen.getByLabelText('昵称（可选）'), { target: { value: 'Tester' } })
    fireEvent.change(screen.getByLabelText('密码'), { target: { value: 'newpass123' } })
    fireEvent.click(screen.getByRole('button', { name: '注册' }))

    await waitFor(() => {
      expect(tauriMock.cloudRegister).toHaveBeenCalledWith({
        method: 'phone',
        phone: '13800138000',
        email: '',
        code: '123456',
        password: 'newpass123',
        name: 'Tester',
      })
      expect(tauriMock.cloudLogin).toHaveBeenCalledWith('13800138000', 'newpass123')
    })
  })

  it('找回密码完成后可用新密码登录', async () => {
    render(
      <AuthGate>
        <div>APP SHELL</div>
      </AuthGate>,
    )

    fireEvent.click(await screen.findByRole('button', { name: '忘记密码？' }))
    fireEvent.change(screen.getByLabelText('手机号'), { target: { value: '13800138000' } })
    fireEvent.click(screen.getByRole('button', { name: '获取验证码' }))

    await waitFor(() => {
      expect(tauriMock.cloudSendSmsCode).toHaveBeenCalledWith('13800138000')
    })

    fireEvent.change(screen.getByLabelText('验证码'), { target: { value: '654321' } })
    fireEvent.change(screen.getByLabelText('新密码'), { target: { value: 'newpass456' } })
    fireEvent.click(screen.getByRole('button', { name: '重置密码' }))

    await waitFor(() => {
      expect(tauriMock.cloudResetPassword).toHaveBeenCalledWith({
        method: 'phone',
        phone: '13800138000',
        email: '',
        code: '654321',
        password: 'newpass456',
      })
    })

    fireEvent.change(screen.getByLabelText('密码'), { target: { value: 'newpass456' } })
    fireEvent.click(screen.getByRole('button', { name: '登录' }))

    await waitFor(() => {
      expect(tauriMock.cloudLogin).toHaveBeenCalledWith('13800138000', 'newpass456')
    })
  })

  it('主动退出登录后回到登录页且不保留 redirectFrom', async () => {
    useAuthStore.setState({
      isLoggedIn: true,
      user: { id: 1, name: 'Test', username: 'test' },
      tenant: { id: 2, name: 'Tenant', balance: '0' },
      cloudModels: [],
      selectedCloudModel: null,
      redirectFrom: { kind: 'chat', conversationId: 'c1' },
      isAuthPending: false,
    })

    await useAuthStore.getState().logout()

    expect(useAuthStore.getState().isLoggedIn).toBe(false)
    expect(useAuthStore.getState().redirectFrom).toBeNull()
  })

  it('恢复登录时刷新 cloudModels 列表但不再回写 settings.cloudModel（Step 2 后由网关决定路由）', async () => {
    tauriMock.getCloudAuth.mockResolvedValue({
      loggedIn: true,
      user: { id: 1, name: 'Test', username: 'test' },
      tenant: { id: 2, name: 'Tenant', balance: '0' },
      models: [],
    })
    tauriMock.getCloudModels.mockResolvedValue([
      { id: 'claude-sonnet-4-5', name: 'Claude Sonnet', modelType: 'chat' },
      { id: 'claude-ops', name: 'Claude Ops', modelType: 'chat' },
    ])
    tauriMock.getSettings.mockResolvedValue({
      primaryModel: 'qwen-plus',
      primaryApiKey: '',
      autoModelRouting: true,
      workspacePath: '',
      analysisThreshold: 1.65,
      autoCleanupEnabled: true,
      tempFileRetentionDays: 7,
      keepOldVersions: 1,
      customModelEndpoint: '',
      customModelName: '',
      cloudModel: 'qwen3-coder-30b-a3b-instruct',
      cloudModelType: 'chat',
    })

    render(
      <AuthGate>
        <div>APP SHELL</div>
      </AuthGate>,
    )

    await screen.findByText('APP SHELL')

    // cloudModels 列表刷新，selectedCloudModel 取首条（仅 in-memory，
    // 给依赖该字段的 UI 留兜底值；不再持久化到 settings）。
    expect(useAuthStore.getState().cloudModels).toHaveLength(2)
    expect(useAuthStore.getState().selectedCloudModel).toBe('claude-sonnet-4-5')

    // 关键回归点：不再把 cloudModel 写回 settings —— 网关按协议+优先级
    // 路由，桌面端不该再固化用户的"第一次选择"。
    expect(tauriMock.updateSettings).not.toHaveBeenCalled()
  })

  it('恢复登录后自动同步工作台资源目录', async () => {
    tauriMock.getCloudAuth.mockResolvedValue({
      loggedIn: true,
      user: { id: 1, name: 'Test', username: 'test' },
      tenant: { id: 2, name: 'Tenant', balance: '0' },
      models: [],
    })

    render(
      <AuthGate>
        <div>APP SHELL</div>
      </AuthGate>,
    )

    await screen.findByText('APP SHELL')

    await waitFor(() => {
      expect(tauriMock.workplaceDirectoryCatalog).toHaveBeenCalled()
    })
  })
})
