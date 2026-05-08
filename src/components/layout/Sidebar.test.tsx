import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const createNewConversation = vi.fn()

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
    switchConversation: vi.fn(),
    deleteConversation: vi.fn(),
    renameConversation: vi.fn(),
  }),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (selector: (state: { isLoggedIn: boolean; user: { name: string; username: string } | null; tenant: { name: string } | null }) => unknown) =>
    selector({
      isLoggedIn: true,
      user: { name: '测试用户', username: 'tester' },
      tenant: { name: '测试租户' },
    }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (selector: (state: { productName: string; logoUrl: string }) => unknown) =>
    selector({
      productName: 'AI小家',
      logoUrl: '/app-icon.png',
    }),
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (selector: (state: { route: { kind: string }; settingsModal: null; setRoute: (route: unknown) => void; openSettings: () => void }) => unknown) =>
    selector({
      route: { kind: 'home' },
      settingsModal: null,
      setRoute: vi.fn(),
      openSettings: vi.fn(),
    }),
}))

import { Sidebar } from './Sidebar'

describe('Sidebar', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders skill-first navigation and conversation list', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)

    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '数字员工' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '汇报中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument()
    expect(screen.getByText('项目')).toBeInTheDocument()
  })
})
