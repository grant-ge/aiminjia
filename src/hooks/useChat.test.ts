import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'

const tauriMock = vi.hoisted(() => ({
  sendMessage: vi.fn().mockResolvedValue(undefined),
  stopStreaming: vi.fn().mockResolvedValue(undefined),
  getMessages: vi.fn(),
  getTasks: vi.fn(),
  createConversation: vi.fn().mockResolvedValue('conv-test'),
  deleteConversation: vi.fn(),
  getConversations: vi.fn(),
  isAgentBusy: vi.fn(),
  renameConversation: vi.fn(),
  archiveConversation: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMock)
vi.mock('@/i18n', () => ({ default: { t: (key: string) => key } }))

import { useChat } from './useChat'
import { useChatStore } from '@/stores/chatStore'

describe('useChat sendUserMessage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useChatStore.setState({
      conversations: [],
      activeConversationId: 'conv-test',
      messages: [],
      busyConversations: new Set(),
      streamStates: {},
      taskStates: {},
      isStreaming: false,
      streamingContent: '',
      toolExecutions: [],
    })
  })

  it('sends slash-prefixed text verbatim without selectedSkillId', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendUserMessage('/salary-query 北京 算法工程师')
    })

    expect(tauriMock.sendMessage).toHaveBeenCalledWith(
      'conv-test',
      expect.any(String),
      '/salary-query 北京 算法工程师',
      [],
    )
  })

  it('does not pass selectedSkillId param — signature is (convId, msgId, text, fileIds)', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendUserMessage('普通消息')
    })

    const callArgs = tauriMock.sendMessage.mock.calls[0]
    expect(callArgs).toHaveLength(4)
    expect(callArgs[0]).toBe('conv-test')
    expect(callArgs[2]).toBe('普通消息')
    expect(callArgs[3]).toEqual([])
  })

  it('stopCurrentStream keeps conversation busy until backend terminal event clears it', () => {
    useChatStore.setState({
      activeConversationId: 'conv-test',
      busyConversations: new Set(['conv-test']),
      streamStates: {
        'conv-test': {
          isStreaming: true,
          streamingContent: 'partial',
          toolExecutions: [],
        },
      },
      isStreaming: true,
      streamingContent: 'partial',
    })
    const { result } = renderHook(() => useChat())

    act(() => {
      result.current.stopCurrentStream()
    })

    expect(tauriMock.stopStreaming).toHaveBeenCalledWith('conv-test')
    expect(useChatStore.getState().isStreaming).toBe(false)
    expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(true)
  })
})
