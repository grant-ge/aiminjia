import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const uiState = vi.hoisted(() => ({
  route: { kind: 'home' } as { kind: string; conversationId?: string; sessionId?: string; skillId?: string },
  setRoute: vi.fn((next: { kind: string; conversationId?: string; sessionId?: string; skillId?: string }) => {
    uiState.route = next
  }),
}))

const chatState = vi.hoisted(() => ({
  activeConversationId: null as string | null,
  conversations: [] as Array<{ id: string; title: string; workspaceName?: string | null }>,
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    conversations: chatState.conversations,
    activeConversationId: chatState.activeConversationId,
    switchConversation: vi.fn(),
    createNewConversation: vi.fn(),
  }),
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (sel: (s: unknown) => unknown) =>
    sel({
      route: uiState.route,
      setRoute: uiState.setRoute,
      openSettings: vi.fn(),
    }),
  useActiveConversationId: () => (uiState.route.kind === 'chat' ? uiState.route.conversationId ?? null : null),
  useActiveChannelSessionId: () => (uiState.route.kind === 'channel' ? uiState.route.sessionId ?? null : null),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: (s: unknown) => unknown) => sel({ user: null, tenant: null }),
}))


vi.mock('@/stores/channelStore', () => ({
  useChannelStore: (sel: (s: unknown) => unknown) =>
    sel({
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
        },
      },
      conversations: [
        {
          sessionId: 'dt-session-1',
          conversationType: 'private',
          displayName: '姚斌权',
          unreadCount: 0,
        },
      ],
    }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: (s: unknown) => unknown) =>
    sel({ productName: '仁励家网络科技(杭州)', logoUrl: '/app-icon.png' }),
}))

import { AppSidebar } from '../AppSidebar'

describe('AppSidebar', () => {
  beforeEach(() => {
    uiState.route = { kind: 'home' }
    uiState.setRoute.mockClear()
    chatState.activeConversationId = null
    chatState.conversations = []
  })

  it('has sidebar background and 256 px width', () => {
    const { container } = render(<AppSidebar />)
    const aside = container.querySelector('aside')
    expect(aside?.className).toMatch(/w-\[256px\]/)
    expect(aside?.className).toMatch(/bg-sidebar/)
  })

  it('renders TenantHeader name', () => {
    render(<AppSidebar />)
    expect(screen.getByText('仁励家网络科技(杭州)')).toBeInTheDocument()
  })

  it('renders the main nav items, the section title 项目, and footer 设置', () => {
    render(<AppSidebar />)
    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '数字员工' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '汇报中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'IM 频道' })).toBeInTheDocument()
    expect(screen.getAllByText('项目').length).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument()
  })

  it('places IM 频道 directly after 定时任务 in the main sidebar nav', () => {
    render(<AppSidebar />)

    const buttons = screen.getAllByRole('button').map((button) => button.textContent?.trim())
    const schedulesIndex = buttons.indexOf('定时任务')

    expect(schedulesIndex).toBeGreaterThanOrEqual(0)
    expect(buttons[schedulesIndex + 1]).toBe('IM 频道')
  })

  it('renders a top drag-region spacer on macOS', async () => {
    // isMac is evaluated at module-load time, so we need to reset modules and
    // re-import with a mocked userAgent.
    const orig = Object.getOwnPropertyDescriptor(navigator, 'userAgent')
    Object.defineProperty(navigator, 'userAgent', {
      value: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)',
      configurable: true,
    })
    vi.resetModules()
    const { AppSidebar: MacSidebar } = await import('../AppSidebar')
    const { container } = render(<MacSidebar />)
    expect(container.querySelector('[data-tauri-drag-region]')).toBeInTheDocument()
    if (orig) Object.defineProperty(navigator, 'userAgent', orig)
    vi.resetModules()
  })

  it('does not highlight 数字员工 while a chat route is active', () => {
    uiState.route = { kind: 'chat', conversationId: 'conv-1' }

    render(<AppSidebar />)

    expect(screen.getByRole('button', { name: '数字员工' }).className).not.toMatch(
      /(^|\s)bg-sidebar-accent(\s|$)/,
    )
  })



  it('highlights 数字员工 when employees route is active', () => {
    uiState.route = { kind: 'employees' }

    render(<AppSidebar />)

    expect(screen.getByRole('button', { name: '数字员工' }).className).toMatch(
      /(^|\s)bg-sidebar-accent(\s|$)/,
    )
    expect(screen.getByRole('button', { name: '新任务' }).className).not.toMatch(
      /(^|\s)bg-sidebar-accent(\s|$)/,
    )
  })

  it('does not highlight an active conversation while schedules route is active', () => {
    uiState.route = { kind: 'schedules' }
    chatState.activeConversationId = 'conv-1'
    chatState.conversations = [{ id: 'conv-1', title: '旧对话', workspaceName: 'txl' }]

    render(<AppSidebar />)

    expect(screen.getByRole('button', { name: /旧对话/ }).className).not.toMatch(
      /(^|\s)bg-sidebar-accent(\s|$)/,
    )
  })

  it('switches the sidebar body between 项目 and 频道 tabs without changing route', async () => {
    render(<AppSidebar />)

    expect(screen.getAllByRole('button', { name: '项目' }).length).toBeGreaterThan(0)
    expect(screen.getAllByRole('button', { name: '频道' }).length).toBeGreaterThan(0)
    await userEvent.click(screen.getByRole('button', { name: '频道' }))
    // Tab switching is local UI state — must not affect route.
    expect(uiState.setRoute).not.toHaveBeenCalled()
    expect(screen.getByText('钉钉')).toBeInTheDocument()
    expect(screen.getByText('姚斌权')).toBeInTheDocument()
    expect(screen.queryByText('飞书')).not.toBeInTheDocument()
    expect(screen.queryByText('微信')).not.toBeInTheDocument()
    expect(screen.queryByText('企业微信')).not.toBeInTheDocument()
  })

})

describe('AppSidebar route-derived sidebarTab', () => {
  beforeEach(() => {
    localStorage.removeItem('aijia-sidebar-tab')
  })

  it('shows channel list after fresh mount when channel tab persisted', () => {
    localStorage.setItem('aijia-sidebar-tab', 'channel')
    uiState.route = { kind: 'channel', sessionId: 'dt-session-1' }
    chatState.activeConversationId = null
    chatState.conversations = []
    render(<AppSidebar />)
    // 钉钉会话 should be highlighted (the mock has displayName: '姚斌权')
    expect(screen.getByText('姚斌权')).toBeInTheDocument()
  })

  it('clicking IM 频道 nav while a session is selected resets route to channel overview', async () => {
    uiState.route = { kind: 'channel', sessionId: 'dt-session-1' }
    chatState.activeConversationId = null
    chatState.conversations = []
    render(<AppSidebar />)
    await userEvent.click(screen.getByRole('button', { name: 'IM 频道' }))
    expect(uiState.setRoute).toHaveBeenCalledWith({ kind: 'channel' })
  })

  it('highlights skill-center nav when route is skill-detail', () => {
    uiState.route = { kind: 'skill-detail', skillId: 'sk-1' }
    chatState.activeConversationId = null
    chatState.conversations = []
    render(<AppSidebar />)
    expect(screen.getByRole('button', { name: '技能中心' }).className).toMatch(
      /(^|\s)bg-sidebar-accent(\s|$)/,
    )
  })

  it('does NOT highlight IM 频道 nav when a specific session is selected (leaf-only)', () => {
    uiState.route = { kind: 'channel', sessionId: 'dt-session-1' }
    chatState.activeConversationId = null
    chatState.conversations = []
    render(<AppSidebar />)
    expect(screen.getByRole('button', { name: 'IM 频道' }).className).not.toMatch(
      /(^|\s)bg-sidebar-accent(\s|$)/,
    )
  })
})
