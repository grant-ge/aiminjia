import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, beforeEach, vi } from 'vitest'

import { AppSidebar } from './AppSidebar'
import { useChannelStore } from '@/stores/channelStore'
import type { ChannelConversation, ChannelPlatformState } from '@/lib/tauri'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    conversations: [],
    activeConversationId: null,
    switchConversation: vi.fn(),
    renameConversation: vi.fn(),
    archiveConversation: vi.fn(),
  }),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: (s: unknown) => unknown) => sel({ tenant: { name: 'T' } }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: (s: unknown) => unknown) => sel({ productName: 'AIjia', logoUrl: '' }),
}))

const uiState = vi.hoisted(() => ({
  route: { kind: 'home' } as { kind: string; sessionId?: string; conversationId?: string; skillId?: string },
}))

const setRouteMock = vi.hoisted(() => vi.fn((next: typeof uiState.route) => {
  uiState.route = next
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (sel: (s: unknown) => unknown) =>
    sel({ route: uiState.route, setRoute: setRouteMock, openSettings: vi.fn() }),
  useActiveConversationId: () => (uiState.route.kind === 'chat' ? uiState.route.conversationId ?? null : null),
  useActiveChannelSessionId: () => (uiState.route.kind === 'channel' ? uiState.route.sessionId ?? null : null),
}))

const conv = (overrides: Partial<ChannelConversation>): ChannelConversation => ({
  sessionId: 's',
  platform: 'dingtalk',
  conversationType: 'private',
  externalId: 'ext',
  displayName: 'name',
  unreadCount: 0,
  robotCode: 'robot-1',
  isActiveRobot: true,
  ...overrides,
})

describe('AppSidebar 频道区', () => {
  beforeEach(() => {
    uiState.route = { kind: 'home' }
    setRouteMock.mockClear()
    localStorage.setItem('aijia-sidebar-tab', 'channel')
    useChannelStore.setState({
      platforms: {
        dingtalk: {
          platform: 'dingtalk',
          capability: 'available',
          configured: true,
          enabled: true,
          connection: 'connected',
          config: null,
          lastConnectedAt: null,
          lastError: null,
        } as ChannelPlatformState,
      },
      conversations: [],
    })
  })

  it('活跃 0 + legacy 0 → 显示 "未配置，点击右侧设置"', () => {
    useChannelStore.setState({
      platforms: {
        dingtalk: {
          platform: 'dingtalk',
          capability: 'available',
          configured: false,
          enabled: false,
          connection: 'unconfigured',
          config: null,
          lastConnectedAt: null,
          lastError: null,
        } as ChannelPlatformState,
      },
      conversations: [],
    })
    render(<AppSidebar />)
    expect(screen.getByText('未配置，点击右侧设置')).toBeInTheDocument()
    expect(screen.queryByText(/历史会话/)).not.toBeInTheDocument()
  })

  it('活跃 0 + legacy >0 → 显示 "未配置" + 折叠按钮', () => {
    useChannelStore.setState({
      conversations: [
        conv({ sessionId: 's1', displayName: '老用户A', isActiveRobot: false, robotCode: 'old-A' }),
        conv({ sessionId: 's2', displayName: '老用户B', isActiveRobot: false, robotCode: 'old-A' }),
      ],
    })
    render(<AppSidebar />)
    expect(screen.getByText('未配置，点击右侧设置')).toBeInTheDocument()
    expect(screen.getByText(/历史会话/)).toBeInTheDocument()
    expect(screen.queryByText('老用户A')).not.toBeInTheDocument() // 默认折叠
  })

  it('活跃 >0 + legacy >0 → 顶部活跃列表 + 底部折叠按钮', () => {
    useChannelStore.setState({
      conversations: [
        conv({ sessionId: 's1', displayName: '姚斌权', isActiveRobot: true, robotCode: 'cur' }),
        conv({ sessionId: 's2', displayName: '老用户', isActiveRobot: false, robotCode: 'old' }),
      ],
    })
    render(<AppSidebar />)
    expect(screen.getByText('姚斌权')).toBeInTheDocument()
    expect(screen.getByText(/历史会话/)).toBeInTheDocument()
  })

  it('点击折叠按钮展开 → legacy 对话按 robotCode 二级分组显示', () => {
    useChannelStore.setState({
      conversations: [
        conv({ sessionId: 's1', displayName: '张三', isActiveRobot: false, robotCode: 'robot-old-001' }),
        conv({ sessionId: 's2', displayName: '李四', isActiveRobot: false, robotCode: 'robot-old-001' }),
        conv({ sessionId: 's3', displayName: '王五', isActiveRobot: false, robotCode: 'robot-old-002' }),
      ],
    })
    render(<AppSidebar />)
    fireEvent.click(screen.getByText(/历史会话/))
    expect(screen.getByText('张三')).toBeInTheDocument()
    expect(screen.getByText('李四')).toBeInTheDocument()
    expect(screen.getByText('王五')).toBeInTheDocument()
    // 二级分组标题至少出现 2 次（两个 robotCode）
    expect(screen.getAllByText(/robot-old/).length).toBeGreaterThanOrEqual(2)
  })

  it('活跃 >0 + legacy 0 → 仅活跃列表，不显示折叠按钮', () => {
    useChannelStore.setState({
      conversations: [
        conv({ sessionId: 's1', displayName: '姚斌权', isActiveRobot: true, robotCode: 'cur' }),
      ],
    })
    render(<AppSidebar />)
    expect(screen.getByText('姚斌权')).toBeInTheDocument()
    expect(screen.queryByText(/历史会话/)).not.toBeInTheDocument()
  })
})
