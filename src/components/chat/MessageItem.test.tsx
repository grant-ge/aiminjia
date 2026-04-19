import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { Message } from '@/types/message'

const formatRelativeTimeMock = vi.fn(() => '5 分钟前')

vi.mock('@/lib/format', () => ({
  formatRelativeTime: (iso: string) => formatRelativeTimeMock(iso),
}))

vi.mock('./UserBubble', () => ({
  UserBubble: () => <div data-testid="user-bubble">USER</div>,
}))

vi.mock('./AiBubble', () => ({
  AiBubble: () => <div data-testid="ai-bubble">AI</div>,
}))

import { MessageItem } from './MessageItem'

const baseMessage: Message = {
  id: 'msg-1',
  conversationId: 'conv-1',
  role: 'user',
  createdAt: '2026-04-19T10:00:00Z',
  content: { text: 'hello' },
}

describe('MessageItem', () => {
  it('renders relative timestamp under user bubble', () => {
    render(<MessageItem message={baseMessage} />)

    expect(screen.getByTestId('user-bubble')).toBeInTheDocument()
    expect(screen.getByText('5 分钟前')).toBeInTheDocument()
    expect(formatRelativeTimeMock).toHaveBeenCalledWith(baseMessage.createdAt)
  })

  it('renders relative timestamp under assistant bubble', () => {
    render(<MessageItem message={{ ...baseMessage, role: 'assistant' }} />)

    expect(screen.getByTestId('ai-bubble')).toBeInTheDocument()
    expect(screen.getByText('5 分钟前')).toBeInTheDocument()
  })
})
