import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { useChatStore } from '@/stores/chatStore'
import { ChatPage } from './ChatPage'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ switchConversation: vi.fn() }),
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
