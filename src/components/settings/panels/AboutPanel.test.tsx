import '@testing-library/jest-dom'
import { act, fireEvent, render, screen } from '@testing-library/react'
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
  dataMaskingLevel: 'relaxed' as const,
  onDataMaskingChange: vi.fn(),
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
    expect(screen.getByRole('button', { name: '检查更新' })).toBeInTheDocument()
    expect(screen.queryByText('帮助与反馈')).not.toBeInTheDocument()
    expect(screen.queryByText('用户体验改进计划')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '隐私权政策' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '隐私政策' })).toBeInTheDocument()
    expect(screen.getByText('开发者')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '上传日志' })).toBeInTheDocument()
  })

  it('vertically aligns the app logo with the metadata text', () => {
    const { container } = render(<AboutPanel {...baseProps} />)

    const metadataSection = container.querySelector('section')
    expect(metadataSection).toHaveClass('items-center')
    expect(metadataSection).toHaveClass('justify-between')

    const metadataRow = metadataSection?.querySelector('div')
    expect(metadataRow).toHaveClass('items-center')
    expect(metadataRow).not.toHaveClass('items-start')
  })

  it('uses the shared Button component for about page action buttons', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.getByRole('button', { name: '检查更新' })).toHaveAttribute('data-ui-button', 'true')
    expect(screen.getByRole('button', { name: '上传日志' })).toHaveAttribute('data-ui-button', 'true')
    expect(screen.queryByRole('button', { name: '重置' })).not.toBeInTheDocument()
  })

  it('wires the check-update button to its handler', () => {
    render(<AboutPanel {...baseProps} />)

    const button = screen.getByRole('button', { name: '检查更新' })
    expect(button).toHaveAttribute('data-aijia-settings-action', 'check-update')

    fireEvent.click(button)

    expect(baseProps.onCheckUpdate).toHaveBeenCalledTimes(1)
  })

  it('shows checking state and disables the button while checking', () => {
    render(<AboutPanel {...baseProps} checkingUpdate />)

    const button = screen.getByRole('button', { name: '检查中…' })
    expect(button).toBeDisabled()
  })

  it('hides unavailable help and feedback entries', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.queryByRole('switch', { name: '用户体验改进计划' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /在线客服/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /产品建议/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '用户协议' })).toBeInTheDocument()
  })

  it('does not render reset while reset is unavailable', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.queryByText('清除本地缓存')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '重置' })).not.toBeInTheDocument()
  })

  it('enables the log-upload button and invokes the handler when clicked', async () => {
    render(<AboutPanel {...baseProps} />)

    const button = screen.getByRole('button', { name: '上传日志' })
    expect(button).not.toBeDisabled()

    await act(async () => {
      fireEvent.click(button)
    })

    expect(baseProps.onUploadLogs).toHaveBeenCalledTimes(1)
  })

  it('does not render the user experience improvement opt-in while unavailable', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.queryByRole('switch', { name: '用户体验改进计划' })).not.toBeInTheDocument()
  })

  it('does not render privacy protection section while it is hidden', () => {
    render(<AboutPanel {...baseProps} dataMaskingLevel="relaxed" onDataMaskingChange={() => {}} />)
    expect(screen.queryByText('隐私')).not.toBeInTheDocument()
    expect(screen.queryByText('隐私保护增强')).not.toBeInTheDocument()
    expect(screen.queryByRole('switch', { name: '隐私保护增强' })).not.toBeInTheDocument()
  })

  it('does not call onDataMaskingChange while privacy protection section is hidden', () => {
    const onChange = vi.fn()
    render(<AboutPanel {...baseProps} dataMaskingLevel="relaxed" onDataMaskingChange={onChange} />)
    expect(screen.queryByRole('switch', { name: '隐私保护增强' })).not.toBeInTheDocument()
    expect(onChange).not.toHaveBeenCalled()
  })
})
