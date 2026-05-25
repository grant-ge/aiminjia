import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useChatStore } from '@/stores/chatStore'
import { setExpertTeam, clearExpertTeam } from '@/features/expert-teams/expertTeamRegistry'
import { ChatPage } from './ChatPage'

const switchConversationMock = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ switchConversation: switchConversationMock }),
}))

vi.mock('@/components/shell/ChatTopBar', () => ({
  ChatTopBar: ({ title }: { title: string }) => <header data-testid="chat-header">{title}</header>,
}))

vi.mock('@/components/layout/ChatArea', () => ({
  ChatArea: () => <main data-testid="chat-content" />,
}))

vi.mock('@/components/chat-scene/ChatBottomArea', () => ({
  ChatBottomArea: () => <footer data-testid="chat-footer-input" />,
}))

vi.mock('@/components/chat/RightPanel', () => ({
  RightPanel: () => <div data-testid="right-panel" />,
}))

describe('ChatPage layout', () => {
  beforeEach(async () => {
    switchConversationMock.mockClear()
    await clearExpertTeam('conv-layout')
    await clearExpertTeam('conv-team')
    useChatStore.setState({ activeConversationId: null, conversations: [], messages: [] })
  })


  it('loads messages on reload when route conversation is already active but message cache is empty', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-reload',
      conversations: [{ id: 'conv-reload', title: '刷新恢复', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-reload" />)

    await waitFor(() => {
      expect(switchConversationMock).toHaveBeenCalledWith('conv-reload')
    })
  })



  it('does not render a redundant expert team banner above the chat content', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-team',
      conversations: [{ id: 'conv-team', title: '专家团会话', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [{ id: 'm1', conversationId: 'conv-team', role: 'assistant', content: { text: '已有消息' }, createdAt: '' }],
    })
    await setExpertTeam('conv-team', 'marketing')

    render(<ChatPage conversationId="conv-team" />)

    expect(screen.queryByLabelText('关闭专家团')).not.toBeInTheDocument()
    expect(screen.queryByText('市场营销策划团')).not.toBeInTheDocument()
  })

  it('composes the chat column as header, content, and footer using flex layout', () => {
    useChatStore.setState({
      activeConversationId: 'conv-layout',
      conversations: [{ id: 'conv-layout', title: '布局测试', createdAt: '', updatedAt: '', isArchived: false }],
    })

    render(<ChatPage conversationId="conv-layout" />)

    const column = screen.getByTestId('chat-layout-column')
    expect(column).toHaveClass('flex')
    expect(column).toHaveClass('flex-col')
    expect(column).toHaveClass('overflow-hidden')

    expect(screen.getByTestId('chat-header')).toBeInTheDocument()
    expect(screen.getByTestId('chat-content')).toBeInTheDocument()
    expect(screen.getByTestId('chat-footer-input')).toBeInTheDocument()
    expect(screen.queryByTestId('right-panel')).not.toBeInTheDocument()
  })
})
