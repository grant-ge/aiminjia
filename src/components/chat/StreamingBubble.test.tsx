import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { StreamingBubble } from './StreamingBubble'
import { useChatStore } from '@/stores/chatStore'

describe('StreamingBubble', () => {
  beforeEach(() => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {},
      taskStates: {},
      toolExecutions: [],
      isStreaming: false,
      streamingContent: '',
      busyConversations: new Set(),
      conversations: [],
      messages: [],
    })
  })

  it('uses the design TypingIndicator before the first streamed token', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [],
        },
      },
    })

    const { getByText } = render(<StreamingBubble content="" />)
    expect(getByText('思考中…')).toBeInTheDocument()
  })

  it('does not render the old avatar-offset layout while streaming', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: 'Hello',
          toolExecutions: [],
        },
      },
    })

    const { container } = render(<StreamingBubble content="Hello" />)

    expect(container.querySelector('.pl-9')).toBeNull()
  })

  it('streaming 内容的代码块不含 hljs 高亮 className', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '```ts\nlet a = 1\n```',
          toolExecutions: [],
        },
      },
    })

    const { container } = render(
      <StreamingBubble content={'```ts\nlet a = 1\n```'} />,
    )
    const code = container.querySelector('pre code')
    expect(code).not.toBeNull()
    expect(code?.className ?? '').not.toMatch(/hljs/)
  })
})
