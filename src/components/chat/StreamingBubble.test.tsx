import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (
      key: string,
      fallbackOrOptions?: string | { defaultValue?: string; count?: number },
    ) => {
      if (typeof fallbackOrOptions === 'string') return fallbackOrOptions
      return fallbackOrOptions?.defaultValue ?? key
    },
    i18n: { language: 'en-US' },
  }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}))
vi.mock('@/stores/brandingStore', () => ({
  DEFAULTS: {
    productName: 'AI小家',
    productNameEn: 'AIjia',
  },
  useBrandingStore: (selector: (state: { productName: string; productNameEn: string }) => unknown) =>
    selector({
      productName: 'AI小家',
      productNameEn: 'AIjia',
    }),
}))

import { StreamingBubble } from './StreamingBubble'
import { useChatStore } from '@/stores/chatStore'

describe('StreamingBubble — tool error visibility', () => {
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

  it('renders error tool with ❌ icon and truncated summary', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [
            {
              toolName: 'execute_python',
              toolId: 'tool-err-1',
              status: 'error',
              summary: 'ModuleNotFoundError: No module named pandas. Please install it first.',
            },
          ],
        },
      },
      toolExecutions: [
        {
          toolName: 'execute_python',
          toolId: 'tool-err-1',
          status: 'error',
          summary: 'ModuleNotFoundError: No module named pandas. Please install it first.',
        },
      ],
    })

    const { getByText, getByLabelText } = render(<StreamingBubble content="" />)
    expect(getByLabelText('tool error')).toBeTruthy()
    expect(getByText(/ModuleNotFoundError/)).toBeTruthy()
  })

  it('does not render error section when no error tools', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [
            { toolName: 'load_data', toolId: 'tool-1', status: 'executing' },
          ],
        },
      },
      toolExecutions: [
        { toolName: 'load_data', toolId: 'tool-1', status: 'executing' },
      ],
    })

    const { queryByLabelText } = render(<StreamingBubble content="" />)
    expect(queryByLabelText('tool error')).toBeNull()
  })

  it('truncates summary longer than 80 characters', () => {
    const longSummary = 'A'.repeat(120)
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [
            { toolName: 'execute_python', toolId: 'tool-2', status: 'error', summary: longSummary },
          ],
        },
      },
      toolExecutions: [
        { toolName: 'execute_python', toolId: 'tool-2', status: 'error', summary: longSummary },
      ],
    })

    const { getByLabelText, getByText } = render(<StreamingBubble content="" />)
    const errorEl = getByLabelText('tool error')
    expect(errorEl.textContent?.length).toBeLessThanOrEqual(120)
    expect(getByText(/…/)).toBeTruthy()
  })
})

describe('StreamingBubble — S1+S2 combined', () => {
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

  it('renders both error tools and task status when both are present', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: 'Analyzing...',
          toolExecutions: [
            { toolName: 'execute_python', toolId: 'tool-err', status: 'error', summary: 'TypeError: unsupported operand' },
            { toolName: 'load_data', toolId: 'tool-ok', status: 'executing' },
          ],
        },
      },
      toolExecutions: [
        { toolName: 'execute_python', toolId: 'tool-err', status: 'error', summary: 'TypeError: unsupported operand' },
        { toolName: 'load_data', toolId: 'tool-ok', status: 'executing' },
      ],
      taskStates: {
        'conv-1': [
          { taskId: 'task-running-1', status: 'running', runId: 'run-1', subject: '' },
          { taskId: 'task-done-0000', status: 'completed', runId: 'run-0', subject: '' },
        ],
      },
    })

    const { getByLabelText, getByRole } = render(<StreamingBubble content="Analyzing..." />)
    expect(getByLabelText('tool error')).toBeTruthy()
    expect(getByRole('img', { name: /running/i })).toBeTruthy()
  })
})
