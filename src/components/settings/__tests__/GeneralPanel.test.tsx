import '@testing-library/jest-dom'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'

import { useSettingsStore } from '@/stores/settingsStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { DEFAULT_SETTINGS } from '@/types/settings'
import i18n from '@/i18n'
import { GeneralPanel } from '../panels/GeneralPanel'

const tauriMock = vi.hoisted(() => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  saveProfileAvatarImage: vi.fn(),
}))

const dialogMock = vi.hoisted(() => ({
  open: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMock)

vi.mock('@tauri-apps/plugin-dialog', () => dialogMock)

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://localhost/${path}`,
}))

const mockUser = { name: '姚域权', tenantName: '仁励家网络科技(杭州)有限公司', avatarUrl: '' }

describe('GeneralPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    void i18n.changeLanguage('zh-CN')
    useBrandingStore.setState({ productName: 'AI猫' })
    useSettingsStore.setState({ ...DEFAULT_SETTINGS, isLoaded: false })
    tauriMock.getSettings.mockResolvedValue({ ...DEFAULT_SETTINGS })
    tauriMock.updateSettings.mockResolvedValue(undefined)
    tauriMock.saveProfileAvatarImage.mockResolvedValue('/Users/me/.renlijia/users/t_1__u_2/profile/avatars/avatar.png')
    dialogMock.open.mockResolvedValue(null)
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

  it('renders profile avatar controls without name or background editing', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('个人信息')).toBeInTheDocument()
    expect(screen.getByText('图标')).toBeInTheDocument()
    expect(screen.getByTestId('settings-profile-avatar-preview')).toHaveStyle({
      background: 'rgba(var(--primary-rgb), 0.12)',
    })
    const initialRadio = screen.getByRole('radio', { name: '用户名首字符' })
    expect(initialRadio).toHaveAttribute('aria-checked', 'true')
    expect(initialRadio.querySelector('span')).toHaveClass('border-primary')
    expect(initialRadio.querySelector('span')).not.toHaveClass('border-foreground')
    expect(screen.getByRole('radio', { name: 'Emoji' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: '上传图片' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.queryByText('背景色')).not.toBeInTheDocument()
    expect(screen.queryByRole('textbox', { name: '姓名' })).not.toBeInTheDocument()
  })

  it('saves an emoji avatar choice into user settings', async () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)

    fireEvent.click(screen.getByRole('radio', { name: 'Emoji' }))
    fireEvent.click(screen.getByRole('button', { name: '选择头像 🐱' }))

    await waitFor(() => {
      expect(tauriMock.updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          profileAvatarMode: 'emoji',
          profileAvatarEmoji: '🐱',
        }),
      )
    })
    expect(screen.getByTestId('settings-profile-avatar-preview')).toHaveTextContent('🐱')
  })

  it('shows an explicit upload button before opening the image picker', async () => {
    dialogMock.open.mockResolvedValue('/tmp/source-avatar.png')
    tauriMock.saveProfileAvatarImage.mockResolvedValue('/Users/me/.renlijia/users/t_1__u_2/profile/avatars/avatar.png')

    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)

    fireEvent.click(screen.getByRole('radio', { name: '上传图片' }))
    expect(screen.getByRole('radio', { name: '上传图片' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('button', { name: '选择图片' })).toBeInTheDocument()
    expect(dialogMock.open).not.toHaveBeenCalled()
    expect(tauriMock.saveProfileAvatarImage).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: '选择图片' }))

    await waitFor(() => {
      expect(tauriMock.saveProfileAvatarImage).toHaveBeenCalledWith('/tmp/source-avatar.png')
      expect(tauriMock.updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          profileAvatarMode: 'image',
          profileAvatarImagePath: '/Users/me/.renlijia/users/t_1__u_2/profile/avatars/avatar.png',
        }),
      )
    })
    expect(screen.getByRole('img', { name: '当前头像' })).toHaveAttribute(
      'src',
      'asset://localhost//Users/me/.renlijia/users/t_1__u_2/profile/avatars/avatar.png',
    )
  })

  it('shows the backend error when image upload fails', async () => {
    dialogMock.open.mockResolvedValue('/tmp/source-avatar.pdf')
    tauriMock.saveProfileAvatarImage.mockRejectedValue('仅支持 png、jpg、jpeg、webp、gif、bmp 图片')

    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)

    fireEvent.click(screen.getByRole('radio', { name: '上传图片' }))
    fireEvent.click(screen.getByRole('button', { name: '选择图片' }))

    expect(await screen.findByText('仅支持 png、jpg、jpeg、webp、gif、bmp 图片')).toBeInTheDocument()
  })

  it('falls back to the username initial when a saved avatar image cannot load', () => {
    useSettingsStore.setState({
      profileAvatarMode: 'image',
      profileAvatarImagePath: '/Users/me/.renlijia/users/t_1__u_2/profile/avatars/missing.png',
    } as never)

    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)

    fireEvent.error(screen.getByRole('img', { name: '当前头像' }))

    expect(screen.queryByRole('img', { name: '当前头像' })).not.toBeInTheDocument()
    expect(screen.getByTestId('settings-profile-avatar-preview')).toHaveTextContent('姚')
  })

  it('renders font size options and applies the selected scale', () => {
    const setFontScale = vi.fn()
    useSettingsStore.setState({ fontScale: 'medium', setFontScale } as never)

    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)

    expect(screen.getByText('字体大小')).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: '小' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: '小' })).toHaveAttribute('title', '12px')
    expect(screen.getByRole('radio', { name: '中' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('radio', { name: '中' })).toHaveAttribute('title', '13px')
    expect(screen.getByRole('radio', { name: '大' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: '大' })).toHaveAttribute('title', '14px')

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
