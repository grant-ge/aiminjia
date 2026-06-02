import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'

import { useSettingsStore } from '@/stores/settingsStore'
import { useBrandingStore } from '@/stores/brandingStore'
import i18n from '@/i18n'
import { GeneralPanel } from '../panels/GeneralPanel'

const mockUser = { name: '姚域权', tenantName: '仁励家网络科技(杭州)有限公司', avatarUrl: '' }

describe('GeneralPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    void i18n.changeLanguage('zh-CN')
    useBrandingStore.setState({ productName: 'AI猫' })
  })

  it('renders user info card with name, backend product name, and logout button', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.getByText('AI猫')).toBeInTheDocument()
    expect(screen.queryByText(/仁励家网络科技/)).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '退出登录' })).toBeInTheDocument()
  })

  it('fires onLogout when logout button clicked', () => {
    const onLogout = vi.fn()
    render(<GeneralPanel user={mockUser} onLogout={onLogout} />)
    fireEvent.click(screen.getByRole('button', { name: '退出登录' }))
    expect(onLogout).toHaveBeenCalledTimes(1)
  })

  it('hides unavailable general settings', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.queryByText('语言')).not.toBeInTheDocument()
    expect(screen.queryByRole('combobox', { name: '语言' })).not.toBeInTheDocument()
    expect(screen.queryByText('开机自启动')).not.toBeInTheDocument()
    expect(screen.queryByText('任务运行时阻止自动休眠')).not.toBeInTheDocument()
    expect(screen.queryAllByRole('switch')).toHaveLength(0)
  })

  it('does not render the local accent color picker (tenant-driven now)', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.queryByText('强调色')).not.toBeInTheDocument()
    expect(screen.queryByRole('radiogroup', { name: '强调色' })).not.toBeInTheDocument()
  })

  it('renders font size options and applies the selected scale', () => {
    const setFontScale = vi.fn()
    useSettingsStore.setState({ fontScale: 'medium', setFontScale } as never)

    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)

    expect(screen.getByText('字体大小')).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: '小' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: '中' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('radio', { name: '大' })).toHaveAttribute('aria-checked', 'false')

    fireEvent.click(screen.getByRole('radio', { name: '大' }))
    expect(setFontScale).toHaveBeenCalledWith('large')
  })

  it('renders chat width options and applies full width mode', () => {
    const setChatWidthMode = vi.fn()
    useSettingsStore.setState({ chatWidthMode: 'centered', setChatWidthMode } as never)

    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)

    expect(screen.getByText('聊天区域宽度')).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: '居中' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('radio', { name: '全宽' })).toHaveAttribute('aria-checked', 'false')

    fireEvent.click(screen.getByRole('radio', { name: '全宽' }))
    expect(setChatWidthMode).toHaveBeenCalledWith('full')
  })

  it('does not render language select while language switching is unavailable', () => {
    const setAppLanguage = vi.fn()
    useSettingsStore.setState({ setAppLanguage } as never)
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.queryByRole('combobox', { name: '语言' })).not.toBeInTheDocument()
    expect(setAppLanguage).not.toHaveBeenCalled()
  })

  it('marks the live UI language as selected even when the settings store is stale', async () => {
    await i18n.changeLanguage('en-US')
    useSettingsStore.setState({ appLanguage: 'zh-CN' } as never)

    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)

    expect(screen.getByRole('radio', { name: 'English' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('radio', { name: '中文' })).toHaveAttribute('aria-checked', 'false')
  })
})
