import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  getLogLevel: vi.fn(),
  setLogLevel: vi.fn(),
}))

vi.mock('@/components/ui/button', () => ({
  Button: ({ variant, size, children, ...props }: React.ComponentProps<'button'> & { variant?: string; size?: string }) => (
    <button type="button" data-ui-button="true" data-variant={variant ?? 'default'} data-size={size ?? 'default'} {...props}>
      {children}
    </button>
  ),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import { AboutPanel } from './AboutPanel'

const baseProps = {
  appName: 'AI小家',
  version: '0.9.30-26041603',
  copyright: '仁励家网络科技(杭州)有限公司 版权所有',
  logoUrl: '/brand-avatar-gold.svg',
  tenantName: '仁励家网络科技(杭州)有限公司',
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
    tauriMock.getLogLevel.mockResolvedValue('info')
    tauriMock.setLogLevel.mockResolvedValue(undefined)
    if (typeof localStorage.clear === 'function') {
      localStorage.clear()
    }
  })

  it('renders the copied about page sections and app metadata', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.getByText('AI小家')).toBeInTheDocument()
    expect(screen.getByText('仁励家网络科技(杭州)有限公司')).toBeInTheDocument()
    expect(screen.getByText('版本 0.9.30-26041603')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '检查更新' })).toBeInTheDocument()
    expect(screen.queryByText('帮助与反馈')).not.toBeInTheDocument()
    expect(screen.queryByText('用户体验改进计划')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '隐私权政策' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '隐私政策' })).toBeInTheDocument()
    expect(screen.getByText('开发者')).toBeInTheDocument()
    expect(screen.getByText('日志级别')).toBeInTheDocument()
    expect(screen.getByText('调整运行时日志详细程度，减少不必要的噪音。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '上传日志' })).toBeInTheDocument()
  })

  it('vertically aligns the app logo with the metadata text', () => {
    const { container } = render(<AboutPanel {...baseProps} />)

    const metadataSection = container.querySelector('section')
    expect(metadataSection).toHaveClass('items-center')
    expect(metadataSection).toHaveClass('justify-between')

    const metadataRow = container.querySelector('[data-aijia-about-metadata]')
    expect(metadataRow?.tagName).toBe('DIV')
    expect(metadataRow).toHaveClass('items-center')
    expect(metadataRow).not.toHaveClass('items-start')
  })

  it('uses the shared Button component for about page action buttons', () => {
    render(<AboutPanel {...baseProps} />)

    expect(screen.getByRole('button', { name: '检查更新' })).toHaveAttribute('data-ui-button', 'true')
    expect(screen.getByRole('button', { name: '上传日志' })).toHaveAttribute('data-ui-button', 'true')
    expect(screen.queryByRole('button', { name: '重置' })).not.toBeInTheDocument()
  })

  it('renders app log level options and marks the current level', async () => {
    tauriMock.getLogLevel.mockResolvedValue('warn')
    render(<AboutPanel {...baseProps} />)

    expect(screen.getByRole('radiogroup', { name: '日志级别' })).toBeInTheDocument()
    await waitFor(() => {
      expect(screen.getByRole('radio', { name: '警告' })).toHaveAttribute('aria-checked', 'true')
    })
    expect(screen.getByRole('radio', { name: '仅错误' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: '标准' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: '调试' })).toHaveAttribute('aria-checked', 'false')
  })

  it('persists app log level when a level is selected', () => {
    render(<AboutPanel {...baseProps} />)

    fireEvent.click(screen.getByRole('radio', { name: '调试' }))

    expect(tauriMock.setLogLevel).toHaveBeenCalledWith('debug')
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

  it('opens the dev control panel after seven clicks on app metadata', () => {
    const { container } = render(<AboutPanel {...baseProps} />)

    const appMetadata = container.querySelector('[data-aijia-about-metadata]')
    expect(appMetadata?.tagName).toBe('DIV')
    for (let i = 0; i < 7; i += 1) {
      fireEvent.click(appMetadata as Element)
    }

    const dialog = screen.getByRole('dialog', { name: '控制面板' })
    expect(dialog).toHaveClass('w-[560px]', 'max-w-[calc(100vw-32px)]')
    expect(screen.getByText('隐藏功能和高级操作入口，不会出现在常规设置中。')).toBeInTheDocument()

    const displayGroup = screen.getByRole('region', { name: '显示' })
    const switchControl = within(displayGroup).getByRole('radiogroup', {
      name: '显示工具失败图标',
    })
    expect(within(switchControl).getByRole('radio', { name: '关' })).toHaveAttribute('aria-checked', 'true')

    fireEvent.click(within(switchControl).getByRole('radio', { name: '开' }))

    expect(within(switchControl).getByRole('radio', { name: '开' })).toHaveAttribute('aria-checked', 'true')
    expect(JSON.parse(localStorage.getItem('aijia-dev-settings') ?? '{}')).toMatchObject({
      showToolErrorIcon: true,
    })
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

  it('does not render privacy protection section', () => {
    render(<AboutPanel {...baseProps} />)
    expect(screen.queryByText('隐私')).not.toBeInTheDocument()
    expect(screen.queryByText('隐私保护增强')).not.toBeInTheDocument()
    expect(screen.queryByRole('switch', { name: '隐私保护增强' })).not.toBeInTheDocument()
  })
})
