import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/hooks/useTurnRenderModel', () => ({
  useTurnRenderModel: () => [],
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn((selector: (state: { isStreaming: boolean; activeConversationId: string | null; streamStates: Record<string, { streamingContent?: string }> }) => unknown) => selector({
    isStreaming: false,
    activeConversationId: 'conv-layout',
    streamStates: {},
  })),
}))

import { MessageList } from './MessageList'

describe('MessageList layout', () => {
  it('does not add horizontal padding inside the shared chat width container', () => {
    const { container } = render(<MessageList />)
    const root = container.firstElementChild

    expect(root).toHaveClass('py-3')
    expect(root).not.toHaveClass('px-6')
  })
})
