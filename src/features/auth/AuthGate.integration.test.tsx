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
    customModelEndpoint: '',
    customModelName: '',
    cloudModel: '',
    cloudModelType: '',
  }),
  updateSettings: vi.fn().mockResolvedValue(undefined),
  getConversations: vi.fn().mockResolvedValue([]),
  isAgentBusy: vi.fn().mockResolvedValue([]),
  cloudLogout: vi.fn().mockResolvedValue(undefined),
  syncBuiltinSkills: vi.fn().mockResolvedValue({ installed: [], skipped: [] }),
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
      dataMaskingLevel: 'strict',
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
      dataMaskingLevel: 'strict',
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
})
