import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  login: vi.fn(),
  sendSmsCode: vi.fn(),
  sendEmailCode: vi.fn(),
  resetPassword: vi.fn(),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: (s: unknown) => unknown) => sel({
    login: mocks.login,
    isAuthPending: false,
  }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: (s: unknown) => unknown) => sel({
    productName: 'AI小家',
    logoUrl: '/brand-avatar-gold.svg',
  }),
}))

vi.mock('@/lib/tauri', () => ({
  cloudRegister: vi.fn(),
  cloudSendSmsCode: mocks.sendSmsCode,
  cloudSendEmailCode: mocks.sendEmailCode,
  cloudResetPassword: mocks.resetPassword,
  getDevEnvironment: vi.fn().mockResolvedValue({
    currentTenant: 'https://ai.renlijia.com',
    currentOps: 'https://ops.renlijia.com',
    isOverride: false,
    presets: [],
  }),
  setDevEnvironment: vi.fn(),
}))

import { useNotificationStore } from '@/stores/notificationStore'
import { LoginPage } from '../LoginPage'

describe('LoginPage password reset', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.sendSmsCode.mockResolvedValue(undefined)
    mocks.sendEmailCode.mockResolvedValue(undefined)
    mocks.resetPassword.mockResolvedValue(undefined)
    useNotificationStore.getState().dismissAll()
    localStorage.clear()
  })

  it('shows an alert and toast when login rejects invalid password', async () => {
    mocks.login.mockRejectedValueOnce(new Error('用户名或密码错误'))

    render(<LoginPage />)

    fireEvent.change(screen.getByLabelText('账号'), { target: { value: 'alice@acme' } })
    fireEvent.change(screen.getByLabelText('密码'), { target: { value: 'wrong-password' } })
    fireEvent.click(screen.getByRole('button', { name: '登录' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('用户名或密码错误')
    expect(useNotificationStore.getState().notifications.at(-1)).toMatchObject({
      level: 'error',
      title: '登录失败',
      message: '用户名或密码错误',
      context: 'toast',
    })
  })

  it('keeps backend error details when phone login fails', async () => {
    mocks.login.mockRejectedValueOnce(new Error('Personal account not registered'))

    render(<LoginPage />)

    fireEvent.change(screen.getByLabelText('账号'), { target: { value: '13800138000' } })
    fireEvent.change(screen.getByLabelText('密码'), { target: { value: 'wrong-password' } })
    fireEvent.click(screen.getByRole('button', { name: '登录' }))

    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('Personal account not registered')
    expect(alert).toHaveTextContent('手机号登录仅支持个人账号；企业成员请使用 用户名@企业编号 登录')
  })

  it('resets a personal password with phone verification and returns to login', async () => {
    render(<LoginPage />)

    fireEvent.click(screen.getByRole('button', { name: '忘记密码？' }))

    expect(screen.getByText('通过手机号或邮箱验证码重置密码')).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('手机号'), { target: { value: '13800138000' } })
    fireEvent.click(screen.getByRole('button', { name: '获取验证码' }))

    await waitFor(() => {
      expect(mocks.sendSmsCode).toHaveBeenCalledWith('13800138000')
    })

    fireEvent.change(screen.getByLabelText('验证码'), { target: { value: '123456' } })
    fireEvent.change(screen.getByLabelText('新密码'), { target: { value: 'newpass123' } })
    fireEvent.click(screen.getByRole('button', { name: '重置密码' }))

    await waitFor(() => {
      expect(mocks.resetPassword).toHaveBeenCalledWith({
        method: 'phone',
        phone: '13800138000',
        email: '',
        code: '123456',
        password: 'newpass123',
      })
    })
    expect(screen.getByText('密码已重置，请使用新密码登录')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '登录' })).toBeInTheDocument()
  })

  it('explains phone login is for personal accounts and enterprise users need username@orgcode', () => {
    render(<LoginPage />)

    fireEvent.change(screen.getByLabelText('账号'), { target: { value: '13800138000' } })

    expect(screen.getByText('手机号登录仅支持个人账号；企业成员请使用 用户名@企业编号 登录')).toBeInTheDocument()
  })
})
