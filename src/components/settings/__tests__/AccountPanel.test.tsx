import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AccountPanel } from '../panels/AccountPanel'

describe('AccountPanel', () => {
  it('renders user info card and notice', () => {
    render(
      <AccountPanel
        user={{ name: '姚域权', tenantName: '仁励家网络科技(杭州)有限公司', avatarUrl: '' }}
        onLogout={() => {}}
      />,
    )
    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.getByText(/仁励家网络科技/)).toBeInTheDocument()
    expect(screen.getByText(/账户信息以企业 SSO/)).toBeInTheDocument()
  })

  it('fires onLogout when button clicked', () => {
    const onLogout = vi.fn()
    render(
      <AccountPanel
        user={{ name: 'X', tenantName: 'Y', avatarUrl: '' }}
        onLogout={onLogout}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: '退出登录' }))
    expect(onLogout).toHaveBeenCalled()
  })
})
