import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/components/ui/button', () => ({
  Button: ({ variant, size, children, ...props }: React.ComponentProps<'button'> & { variant?: string; size?: string }) => (
    <button type="button" data-ui-button="true" data-variant={variant ?? 'default'} data-size={size ?? 'default'} {...props}>
      {children}
    </button>
  ),
}))

import { AboutPanel } from './AboutPanel'

const baseProps = {
  appName: 'AI小家',
  version: '0.9.30-26041603',
  copyright: '仁励家网络科技(杭州)有限公司 版权所有',
  logoUrl: '/brand-avatar-gold.svg',
  onCheckUpdate: vi.fn(),
  onUploadLogs: vi.fn(),
  onResetData: vi.fn(),
  links: {
    customerService: vi.fn(),
    productSuggestion: vi.fn(),
    privacyPolicy: vi.fn(),
    terms: vi.fn(),
  },
}

describe('AboutPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
  })

  it('renders the copied about page sections and app metadata', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.getByText('AI小家')).toBeInTheDocument()
    expect(screen.getByText('版本 0.9.30-26041603')).toBeInTheDocument()
    expect(screen.getByText('版权公告：仁励家网络科技(杭州)有限公司 版权所有')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '检查更新' })).toBeInTheDocument()
    expect(screen.getByText('帮助与反馈')).toBeInTheDocument()
    expect(screen.getByText('用户体验改进计划')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '隐私权政策' })).toHaveClass('text-primary')
    expect(screen.getByRole('button', { name: /隐私政策/ })).toHaveClass('text-primary')
    expect(screen.getByText('开发者模式')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '上传日志' })).toBeInTheDocument()
  })

  it('uses the shared Button component for about page action buttons', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.getByRole('button', { name: '检查更新' })).toHaveAttribute('data-ui-button', 'true')
    expect(screen.getByRole('button', { name: '上传日志' })).toHaveAttribute('data-ui-button', 'true')
    expect(screen.getByRole('button', { name: '重置' })).toHaveAttribute('data-ui-button', 'true')
    expect(screen.getByRole('button', { name: '重置' })).toHaveAttribute('data-variant', 'destructive')
  })

  it('wires the still-active actions to their handlers', () => {
    render(<AboutPanel {...baseProps} />)

    fireEvent.click(screen.getByRole('button', { name: '检查更新' }))

    expect(baseProps.onCheckUpdate).toHaveBeenCalledTimes(1)
  })

  it('disables the help, feedback, log-upload, and reset entries pending implementation', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.getByRole('switch', { name: '用户体验改进计划' })).toBeDisabled()
    expect(screen.getByRole('button', { name: /在线客服/ })).toBeDisabled()
    expect(screen.getByRole('button', { name: /产品建议/ })).toBeDisabled()
    expect(screen.getByRole('button', { name: /服务条款/ })).toBeDisabled()
    expect(screen.getByRole('button', { name: '上传日志' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '重置' })).toBeDisabled()
    expect(screen.getAllByText('即将支持').length).toBeGreaterThan(0)
  })

  it('defaults the user experience improvement opt-in to ON', () => {
    render(<AboutPanel {...baseProps} />)

    const toggle = screen.getByRole('switch', { name: '用户体验改进计划' })
    expect(toggle).toBeChecked()
  })
})
