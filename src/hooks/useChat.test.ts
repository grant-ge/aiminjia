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
  clearActiveTurnStage: vi.fn(),
  pendingPermissionSnapshotForSession: vi.fn(),
  pendingInteractionSnapshotForSession: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  renameConversation: vi.fn(),
  archiveConversation: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMock)
vi.mock('@/i18n', () => ({ default: { t: (key: string) => key } }))

import { useChat } from './useChat'
import { useChatStore } from '@/stores/chatStore'
import { useSidebarStatusStore } from '@/stores/sidebarStatusStore'
import { DEFAULT_SETTINGS } from '@/types/settings'

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
    tauriMock.clearActiveTurnStage.mockResolvedValue(undefined)
    tauriMock.pendingPermissionSnapshotForSession.mockResolvedValue([])
    tauriMock.pendingInteractionSnapshotForSession.mockResolvedValue([])
    tauriMock.getSettings.mockResolvedValue({ ...DEFAULT_SETTINGS })
    tauriMock.updateSettings.mockResolvedValue(undefined)
    useSidebarStatusStore.setState({ statuses: {} })
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
    )
    expect(useChatStore.getState().messages[0].content.skillCommand).toEqual({
      id: 'dingtalk-workspace',
      label: '玩转钉钉',
      command: '/dingtalk-workspace',
    })
    expect(useChatStore.getState().messages[0].content.commandText).toBe('/dingtalk-workspace')
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
    expect(useChatStore.getState().streamStates['conv-test']?.turnStage ?? null).toBeNull()
  })

  it('does not hydrate a persisted permission stage when runtime has no recoverable ask', async () => {
    useSidebarStatusStore.setState({
      statuses: {
        'conv-test': {
          kind: 'permission-review',
          updatedAt: 1780000000000,
          toolCallId: 'tool-1',
        },
      },
    })
    tauriMock.getActiveTurnStage.mockResolvedValueOnce({
      stage: {
        kind: 'waitingPermission',
        toolName: 'Bash',
        toolCallId: 'tool-1',
      },
      stageStartedAtMs: 1780000000000,
      lastHeartbeatAtMs: 1780000000000,
    })
    tauriMock.pendingPermissionSnapshotForSession.mockResolvedValueOnce([])

    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.switchConversation('conv-test')
    })

    await waitFor(() =>
      expect(tauriMock.clearActiveTurnStage).toHaveBeenCalledWith('conv-test'),
    )
    expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(false)
    expect(useChatStore.getState().streamStates['conv-test']?.turnStage ?? null).toBeNull()
    expect(useSidebarStatusStore.getState().statuses['conv-test']).toBeUndefined()
    expect(tauriMock.pendingPermissionSnapshotForSession).toHaveBeenCalledWith('conv-test')
  })

  it('does not hydrate a persisted AskUserQuestion stage when runtime has no recoverable interaction', async () => {
    useSidebarStatusStore.setState({
      statuses: {
        'conv-test': {
          kind: 'waiting-reply',
          updatedAt: 1780000000000,
          interactionId: 'ask-1',
        },
      },
    })
    tauriMock.getActiveTurnStage.mockResolvedValueOnce({
      stage: {
        kind: 'waitingInteraction',
        interactionKind: 'askUserQuestion',
        interactionId: 'ask-1',
      },
      stageStartedAtMs: 1780000000000,
      lastHeartbeatAtMs: 1780000000000,
    })
    tauriMock.pendingInteractionSnapshotForSession.mockResolvedValueOnce([])

    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.switchConversation('conv-test')
    })

    await waitFor(() =>
      expect(tauriMock.clearActiveTurnStage).toHaveBeenCalledWith('conv-test'),
    )
    expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(false)
    expect(useChatStore.getState().streamStates['conv-test']?.turnStage ?? null).toBeNull()
    expect(useSidebarStatusStore.getState().statuses['conv-test']).toBeUndefined()
    expect(tauriMock.pendingInteractionSnapshotForSession).toHaveBeenCalledWith('conv-test')
  })

  it('restores AskUserQuestion tool loading when a persisted interaction stage is recoverable', async () => {
    tauriMock.getActiveTurnStage.mockResolvedValueOnce({
      stage: {
        kind: 'waitingInteraction',
        interactionKind: 'askUserQuestion',
        interactionId: 'ask-1',
      },
      stageStartedAtMs: 1780000000000,
      lastHeartbeatAtMs: 1780000000000,
    })
    tauriMock.pendingInteractionSnapshotForSession.mockResolvedValueOnce([
      {
        conversationId: 'conv-test',
        runId: 'run-1',
        interactionId: 'ask-1',
        toolCallId: 'tool-1',
        toolName: 'AskUserQuestion',
        kind: 'askUserQuestion',
        payload: {
          questions: [
            {
              header: '范围',
              question: '测哪个？',
              options: [{ label: '官网', description: '测试官网' }],
            },
          ],
        },
      },
    ])

    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.switchConversation('conv-test')
    })

    await waitFor(() =>
      expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(true),
    )
    expect(useChatStore.getState().streamStates['conv-test']?.toolExecutions).toMatchObject([
      {
        toolId: 'tool-1',
        toolName: 'AskUserQuestion',
        status: 'executing',
      },
    ])
  })

  it('restores AskUserQuestion tool loading from a persisted tools stage', async () => {
    tauriMock.getActiveTurnStage.mockResolvedValueOnce({
      stage: {
        kind: 'tools',
        iteration: 0,
        running: [
          {
            toolName: 'AskUserQuestion',
            toolCallId: 'tool-1',
            startedAtMs: 1780000000000,
          },
        ],
        completedInBatch: 0,
      },
      stageStartedAtMs: 1780000000000,
      lastHeartbeatAtMs: 1780000000000,
    })
    tauriMock.pendingInteractionSnapshotForSession.mockResolvedValueOnce([
      {
        conversationId: 'conv-test',
        runId: 'run-1',
        interactionId: 'ask-1',
        toolCallId: 'tool-1',
        toolName: 'AskUserQuestion',
        kind: 'askUserQuestion',
        payload: {
          questions: [
            {
              header: '范围',
              question: '测哪个？',
              options: [{ label: '官网', description: '测试官网' }],
            },
          ],
        },
      },
    ])

    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.switchConversation('conv-test')
    })

    await waitFor(() =>
      expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(true),
    )
    expect(useChatStore.getState().streamStates['conv-test']?.toolExecutions).toMatchObject([
      {
        toolId: 'tool-1',
        toolName: 'AskUserQuestion',
        status: 'executing',
      },
    ])
    expect(useSidebarStatusStore.getState().statuses['conv-test']).toMatchObject({
      kind: 'waiting-reply',
      interactionId: 'ask-1',
    })
  })

  it('keeps AskUserQuestion loading after refresh when the turn stage is missing but interaction is recoverable', async () => {
    tauriMock.getActiveTurnStage.mockResolvedValueOnce(null)
    tauriMock.pendingInteractionSnapshotForSession.mockResolvedValueOnce([
      {
        conversationId: 'conv-test',
        runId: 'run-1',
        interactionId: 'ask-1',
        toolCallId: 'tool-1',
        toolName: 'AskUserQuestion',
        kind: 'askUserQuestion',
        payload: {
          questions: [
            {
              header: '范围',
              question: '测哪个？',
              options: [{ label: '官网', description: '测试官网' }],
            },
          ],
        },
      },
    ])

    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.switchConversation('conv-test')
    })

    await waitFor(() =>
      expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(true),
    )
    expect(useChatStore.getState().streamStates['conv-test']?.turnStage).toEqual({
      kind: 'waitingInteraction',
      interactionKind: 'askUserQuestion',
      interactionId: 'ask-1',
    })
    expect(useChatStore.getState().streamStates['conv-test']?.toolExecutions).toMatchObject([
      {
        toolId: 'tool-1',
        toolName: 'AskUserQuestion',
        status: 'executing',
      },
    ])
    expect(useSidebarStatusStore.getState().statuses['conv-test']).toMatchObject({
      kind: 'waiting-reply',
      interactionId: 'ask-1',
    })
  })

  it('does not hydrate a persisted non-human stage when backend is idle', async () => {
    tauriMock.getActiveTurnStage.mockResolvedValueOnce({
      stage: {
        kind: 'tools',
        iteration: 0,
        running: [{ toolName: 'Bash', toolCallId: 'tool-1' }],
        completedCount: 0,
      },
      stageStartedAtMs: 1780000000000,
      lastHeartbeatAtMs: 1780000000000,
    })
    tauriMock.isAgentBusy.mockResolvedValueOnce([])

    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.switchConversation('conv-test')
    })

    await waitFor(() =>
      expect(tauriMock.clearActiveTurnStage).toHaveBeenCalledWith('conv-test'),
    )
    expect(useChatStore.getState().busyConversations.has('conv-test')).toBe(false)
    expect(useChatStore.getState().streamStates['conv-test']?.turnStage ?? null).toBeNull()
  })
})
