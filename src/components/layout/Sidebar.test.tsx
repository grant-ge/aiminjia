import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useNotificationStore } from '@/stores/notificationStore'

const createNewConversation = vi.fn()
const switchConversation = vi.fn()
const deleteConversation = vi.fn()
const renameConversation = vi.fn()
const exportConversation = vi.fn()

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    conversations: [
      {
        id: 'conv-python',
        title: 'Python 分析',
        createdAt: '2026-04-19T10:00:00Z',
        updatedAt: '2026-04-19T10:00:00Z',
        isArchived: false,
      },
      {
        id: 'conv-finance',
        title: '财务报告',
        createdAt: '2026-04-18T10:00:00Z',
        updatedAt: '2026-04-18T10:00:00Z',
        isArchived: false,
      },
    ],
    activeConversationId: 'conv-python',
    createNewConversation,
    switchConversation,
    deleteConversation,
    renameConversation,
  }),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: (selector: (state: { busyConversations: Set<string> }) => unknown) =>
    selector({ busyConversations: new Set() }),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (selector: (state: { isLoggedIn: boolean; user: null; tenant: null }) => unknown) =>
    selector({ isLoggedIn: false, user: null, tenant: null }),
}))

vi.mock('@/stores/settingsStore', () => ({
  useSettingsStore: (selector: (state: { useCloud: boolean; setSettings: () => void }) => unknown) =>
    selector({ useCloud: false, setSettings: () => {} }),
}))

vi.mock('@/stores/personaStore', () => ({
  usePersonaStore: () => ({
    personas: [],
    activePersona: null,
    setActive: vi.fn(),
  }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (selector: (state: { logoUrl: string; accentColor: string }) => unknown) =>
    selector({ logoUrl: '/app-icon.png', accentColor: '#D4A843' }),
}))

vi.mock('@/hooks/useProductName', () => ({
  useProductName: () => 'AIjia',
}))

vi.mock('@/lib/tauri', () => ({
  updateSettings: vi.fn(),
  getSettings: vi.fn(),
  exportConversation: (...args: unknown[]) => exportConversation(...args),
}))

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.0.0'),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (
      key: string,
      fallbackOrOptions?:
        | string
        | {
            defaultValue?: string
            fileName?: string
          },
    ) => {
      if (typeof fallbackOrOptions === 'string') return fallbackOrOptions
      if (fallbackOrOptions?.defaultValue) return fallbackOrOptions.defaultValue
      if (key === 'sidebar.searchPlaceholder') return '搜索对话...'
      if (key === 'sidebar.noSearchResults') return '没有匹配的对话'
      if (key === 'sidebar.conversationActions') return '对话操作'
      if (key === 'sidebar.newChat') return '新任务'
      if (key === 'sidebar.settings') return '设置'
      if (key === 'topBar.exportAsHtml') return '导出为 HTML'
      if (key === 'topBar.exportAsPdf') return '导出为 PDF'
      if (key === 'topBar.exportSuccess') return '导出成功'
      if (key === 'topBar.exportFailed') return '导出失败'
      return key
    },
  }),
}))

import { Sidebar } from './Sidebar'

describe('Sidebar', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useNotificationStore.getState().dismissAll()
  })

  it('renders search input and full conversation list by default', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)

    expect(screen.getByPlaceholderText('搜索对话...')).toBeInTheDocument()
    expect(screen.getByText('Python 分析')).toBeInTheDocument()
    expect(screen.getByText('财务报告')).toBeInTheDocument()
  })

  it('renders sidebar navigation skeleton', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)

    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument()
  })

  it('filters conversations by title and highlights matches', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)

    fireEvent.change(screen.getByPlaceholderText('搜索对话...'), {
      target: { value: 'python' },
    })

    const highlightedTitle = screen.getByText('Python').closest('span')
    expect(highlightedTitle).toHaveTextContent('Python 分析')
    expect(screen.queryByText('财务报告')).not.toBeInTheDocument()
    expect(screen.getByText('Python')).toContainHTML('mark')
  })

  it('shows empty state when no conversations match the search query', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)

    fireEvent.change(screen.getByPlaceholderText('搜索对话...'), {
      target: { value: '不存在' },
    })

    expect(screen.getByText('没有匹配的对话')).toBeInTheDocument()
  })

  it('exports conversation from sidebar action menu', async () => {
    exportConversation.mockResolvedValueOnce({ fileName: 'python-report.html' })

    render(<Sidebar onOpenSettings={vi.fn()} />)

    fireEvent.click(screen.getByRole('button', { name: '对话操作 Python 分析' }))
    fireEvent.click(screen.getByRole('button', { name: '导出为 HTML' }))

    await waitFor(() => {
      expect(exportConversation).toHaveBeenCalledWith('conv-python', 'html')
    })

    expect(
      useNotificationStore
        .getState()
        .notifications.some((notification) => notification.message === 'python-report.html'),
    ).toBe(true)
  })
})
