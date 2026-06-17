import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'

const tauriMock = vi.hoisted(() => ({
  sendMessage: vi.fn().mockResolvedValue(undefined),
  compactConversation: vi.fn().mockResolvedValue(undefined),
  stopStreaming: vi.fn().mockResolvedValue(undefined),
  getMessages: vi.fn(),
  getTasks: vi.fn(),
  createConversation: vi.fn().mockResolvedValue('conv-test'),
  deleteConversation: vi.fn(),
  getConversations: vi.fn(),
  isAgentBusy: vi.fn(),
  getActiveTurnStage: vi.fn(),
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
    tauriMock.getMessages.mockResolvedValue([])
    tauriMock.getTasks.mockResolvedValue([])
    tauriMock.isAgentBusy.mockResolvedValue([])
    tauriMock.getActiveTurnStage.mockResolvedValue(null)
  })

  it('sends slash-prefixed text verbatim without skill metadata', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendUserMessage('/salary-query 北京 算法工程师')
    })

    expect(tauriMock.sendMessage).toHaveBeenCalledWith(
      'conv-test',
      '/salary-query 北京 算法工程师',
      undefined,
      null,
      expect.any(String),
      null,
      undefined,
      undefined,
    )
  })

  it('routes /compact as a manual compact control command without adding a user bubble', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendUserMessage('/compact 保留本次排查结论')
    })

    expect(tauriMock.compactConversation).toHaveBeenCalledWith(
      'conv-test',
      '保留本次排查结论',
    )
    expect(tauriMock.sendMessage).not.toHaveBeenCalled()
    expect(useChatStore.getState().messages).toHaveLength(0)
    expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(false)
  })

  it('passes explicit skill metadata and keeps it on the optimistic user message', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendUserMessage('查今天日程', undefined, {
        id: 'dingtalk-workspace',
        label: '玩转钉钉',
        command: '/dingtalk-workspace',
      })
    })

    expect(tauriMock.sendMessage).toHaveBeenCalledWith(
      'conv-test',
      '查今天日程',
      undefined,
      null,
      expect.any(String),
      {
        id: 'dingtalk-workspace',
        label: '玩转钉钉',
        command: '/dingtalk-workspace',
      },
      undefined,
      undefined,
    )
    expect(useChatStore.getState().messages[0].content.skillCommand).toEqual({
      id: 'dingtalk-workspace',
      label: '玩转钉钉',
      command: '/dingtalk-workspace',
    })
    expect(useChatStore.getState().messages[0].content.commandText).toBe('/dingtalk-workspace')
  })

  it('passes per-turn reasoning mode and keeps it on the optimistic user message', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendUserMessage('做一份复杂薪酬审查', undefined, null, 'default', 'deep')
    })

    expect(tauriMock.sendMessage).toHaveBeenCalledWith(
      'conv-test',
      '做一份复杂薪酬审查',
      undefined,
      null,
      expect.any(String),
      null,
      'default',
      'deep',
    )
    expect(useChatStore.getState().messages[0].content.reasoningMode).toBe('deep')
  })

  it('stopCurrentStream clears busy state immediately so stopped turns do not keep rendering as active', () => {
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
    expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(false)
  })

  it('clears stale busy state when switched conversation has no active turn stage and backend is idle', async () => {
    useChatStore.setState({
      activeConversationId: 'other-conv',
      busyConversations: new Set(['conv-test']),
      streamStates: {
        'conv-test': {
          isStreaming: false,
          streamingContent: '',
          toolExecutions: [],
          turnStage: {
            kind: 'waitingPermission',
            toolName: 'Read',
            toolCallId: 'tool-1',
          },
        },
      },
    })
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.switchConversation('conv-test')
    })

    await waitFor(() =>
      expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(false),
    )
    expect(useChatStore.getState().streamStates['conv-test']?.turnStage).toBeNull()
  })
})
