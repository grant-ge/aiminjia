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
})
