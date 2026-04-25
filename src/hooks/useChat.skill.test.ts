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
      selectedSkillCommands: {},
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

  it('createConversationFromSkill 创建会话并设置技能命令 token', async () => {
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.createConversationFromSkill('skill-smith')
    })

    expect(tauriMock.sendMessage).not.toHaveBeenCalled()
    expect(useChatStore.getState().selectedSkillCommands['conv-skill']).toEqual({
      id: 'skill-smith',
      label: '创建自己的技能',
      command: '/skill-smith',
    })
    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'conv-skill' })
  })

  it('sendUserMessage 透传 selectedSkillId 且用户消息保持原文', async () => {
    useChatStore.setState({ activeConversationId: 'conv-skill' })
    const { result } = renderHook(() => useChat())

    await act(async () => {
      await result.current.sendUserMessage('帮我分析薪酬', undefined, undefined, 'salary-query')
    })

    expect(tauriMock.sendMessage).toHaveBeenCalledWith(
      'conv-skill',
      '帮我分析薪酬',
      undefined,
      undefined,
      expect.any(String),
      'salary-query',
      'salary-query',
    )
    expect(useChatStore.getState().messages.at(-1)?.content.text).toBe('帮我分析薪酬')
    expect(useChatStore.getState().messages.at(-1)?.content.commandText).toBe('/salary-query 帮我分析薪酬')
    expect(useChatStore.getState().messages.at(-1)?.content.skillCommand).toEqual({
      id: 'salary-query',
      label: 'salary-query',
      command: '/salary-query',
    })
  })

})
