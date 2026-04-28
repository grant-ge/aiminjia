import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
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
    dataMaskingLevel: 'strict',
    autoCleanupEnabled: true,
    tempFileRetentionDays: 7,
    keepOldVersions: 1,
    tavilyApiKey: '',
    bochaApiKey: '',
    customModelEndpoint: '',
    customModelName: '',
    cloudModel: '',
    cloudModelType: '',
    useCloud: true,
  }),
  updateSettings: vi.fn().mockResolvedValue(undefined),
  getConversations: vi.fn().mockResolvedValue([]),
  isAgentBusy: vi.fn().mockResolvedValue([]),
  cloudLogout: vi.fn().mockResolvedValue(undefined),
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
      dataMaskingLevel: 'strict',
      autoCleanupEnabled: true,
      tempFileRetentionDays: 7,
      keepOldVersions: 1,
      tavilyApiKey: '',
      bochaApiKey: '',
      customModelEndpoint: '',
      customModelName: '',
      cloudModel: '',
      cloudModelType: '',
      useCloud: true,
    })
    tauriMock.updateSettings.mockResolvedValue(undefined)
    tauriMock.getConversations.mockResolvedValue([])
    tauriMock.isAgentBusy.mockResolvedValue([])

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

    fireEvent.change(await screen.findByPlaceholderText('用户名'), { target: { value: 'demo' } })
    fireEvent.change(await screen.findByPlaceholderText('企业编号'), { target: { value: 'test' } })
    fireEvent.change(await screen.findByLabelText('密码'), { target: { value: '123456' } })
    fireEvent.click(screen.getByRole('button', { name: '登录' }))

    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-center' })
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

  it('恢复登录时自动把过期云端模型切到最新可用模型并持久化', async () => {
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
      dataMaskingLevel: 'strict',
      autoCleanupEnabled: true,
      tempFileRetentionDays: 7,
      keepOldVersions: 1,
      tavilyApiKey: '',
      bochaApiKey: '',
      customModelEndpoint: '',
      customModelName: '',
      cloudModel: 'qwen3-coder-30b-a3b-instruct',
      cloudModelType: 'chat',
      useCloud: true,
    })

    render(
      <AuthGate>
        <div>APP SHELL</div>
      </AuthGate>,
    )

    await screen.findByText('APP SHELL')

    expect(useAuthStore.getState().selectedCloudModel).toBe('claude-sonnet-4-5')
    expect(tauriMock.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        useCloud: true,
        cloudModel: 'claude-sonnet-4-5',
        cloudModelType: 'chat',
      }),
    )
  })
})
