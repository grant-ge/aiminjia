import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'

const tauriMock = vi.hoisted(() => ({
  sendMessage: vi.fn().mockResolvedValue(undefined),
  stopStreaming: vi.fn(),
  getMessages: vi.fn(),
  getTasks: vi.fn(),
  createConversation: vi.fn().mockResolvedValue('conv-skill'),
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
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

describe('useChat skill launch', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useChatStore.setState({
      conversations: [],
      activeConversationId: null,
      messages: [],
      busyConversations: new Set(),
      streamStates: {},
      taskStates: {},
      isStreaming: false,
      streamingContent: '',
      toolExecutions: [],
    })
    useSkillStore.setState({
      skills: [
        { id: 'skill-smith', displayName: '创建自己的技能', description: '', source: 'builtin', hasWorkflow: true, icon: 'file-text', category: 'general', triggerText: '我想创建一个技能', shortDescription: '', displayNameEn: 'Skill Smith', shortDescriptionEn: '' },
      ],
      recommendedIds: [],
      isLoading: false,
    })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('createConversationFromSkill 创建会话并发送技能 triggerText', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.createConversationFromSkill('skill-smith')
    })

    expect(tauriMock.sendMessage).toHaveBeenCalledWith(
      'conv-skill',
      '我想创建一个技能',
      undefined,
      undefined,
      expect.any(String),
    )
    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'conv-skill' })
  })
})
