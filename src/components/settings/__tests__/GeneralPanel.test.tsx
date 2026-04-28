import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'

import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { GeneralPanel } from '../panels/GeneralPanel'

const mockUser = { name: '姚域权', tenantName: '仁励家网络科技(杭州)有限公司', avatarUrl: '' }

describe('GeneralPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders user info card with name, tenant, and logout button', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.getByText(/仁励家网络科技/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '退出登录' })).toBeInTheDocument()
  })

  it('fires onLogout when logout button clicked', () => {
    const onLogout = vi.fn()
    render(<GeneralPanel user={mockUser} onLogout={onLogout} />)
    fireEvent.click(screen.getByRole('button', { name: '退出登录' }))
    expect(onLogout).toHaveBeenCalledTimes(1)
  })

  it('renders 通用 section with disabled language selector', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('通用')).toBeInTheDocument()
    expect(screen.getByText('语言')).toBeInTheDocument()
    expect(screen.getByRole('combobox', { name: '语言' })).toBeDisabled()
  })

  it('renders disabled autostart and prevent-sleep toggles', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('开机自启动')).toBeInTheDocument()
    expect(screen.getByText('任务运行时阻止自动休眠')).toBeInTheDocument()
    const toggles = screen.getAllByRole('switch')
    expect(toggles).toHaveLength(2)
    toggles.forEach((t) => expect(t).toBeDisabled())
  })

  it('renders 外观 section with accent color swatches', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('外观')).toBeInTheDocument()
    expect(screen.getByText('强调色')).toBeInTheDocument()
    const swatches = screen.getByRole('radiogroup', { name: '强调色' }).querySelectorAll('[role="radio"]')
    expect(swatches).toHaveLength(7)
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

  it('selecting an accent color swatch calls applyBranding with new color', () => {
    const applyBranding = vi.fn()
    useBrandingStore.setState({ accentColor: '#DBAA22', applyBranding } as never)
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    // Click the indigo swatch (#4f46e5)
    fireEvent.click(screen.getByRole('radio', { name: '#4f46e5' }))
    expect(applyBranding).toHaveBeenCalledWith({ accentColor: '#4f46e5' })
  })

  it('language select is disabled and does not call setAppLanguage', () => {
    const setAppLanguage = vi.fn()
    useSettingsStore.setState({ setAppLanguage } as never)
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    const select = screen.getByRole('combobox', { name: '语言' })
    expect(select).toBeDisabled()
    fireEvent.change(select, { target: { value: 'en-US' } })
    expect(setAppLanguage).not.toHaveBeenCalled()
  })
})
