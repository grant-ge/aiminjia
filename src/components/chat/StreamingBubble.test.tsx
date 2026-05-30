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

  it('inline 模式（suppressIndicator）下 content 被 sanitize 剃光时不渲染空壳', () => {
    // 复现：LLM 流式吐到 `<function_calls>` 开始标签但还没闭合时，
    // stripHallucinatedXml 会把整段砍掉。如果 inline StreamingBubble
    // 还渲染 wrapper，ChatRow 会把它当 flex item 占 gap-3 + mb-7 空位，
    // 在"运行了 X 个命令"和"思考中…"之间撑出一块空白。
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '\n<function_calls>',
          toolExecutions: [],
        },
      },
    })

    const { container } = render(
      <StreamingBubble content={'\n<function_calls>'} suppressIndicator />,
    )

    expect(container.querySelector('[data-aijia-streaming-bubble]')).toBeNull()
  })

  it('placeholder 模式（content="" + treatAsHasContent）仍然渲染 indicator', () => {
    // 末尾 indicator-only placeholder：上面已经有 persisted blocks，
    // content="" 但 turn 没结束，必须挂 indicator 给用户"还在思考"的反馈。
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

    const { container, getByText } = render(
      <StreamingBubble content="" treatAsHasContent />,
    )
    expect(container.querySelector('[data-aijia-streaming-bubble]')).not.toBeNull()
    expect(getByText('思考中…')).toBeInTheDocument()
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
