import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const chatState = vi.hoisted(() => ({
  isStreaming: false,
}))

const sendUserMessageMock = vi.hoisted(() => vi.fn(async () => true))
const stopCurrentStreamMock = vi.hoisted(() => vi.fn())
const selectAndUploadFilesMock = vi.hoisted(() => vi.fn(async () => []))
const selectAndAuthorizeDirectoryMock = vi.hoisted(() => vi.fn(async () => undefined))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: sendUserMessageMock,
    isStreaming: chatState.isStreaming,
    stopCurrentStream: stopCurrentStreamMock,
  }),
}))

vi.mock('@/hooks/useAuthorizedWorkspace', () => ({
  useAuthorizedWorkspace: () => ({
    workspace: null,
  }),
}))

vi.mock('@/hooks/useFileUpload', () => ({
  useFileUpload: () => ({
    isUploading: false,
    selectAndUploadFiles: selectAndUploadFilesMock,
  }),
}))

vi.mock('@/hooks/useWorkspaceAuthorization', () => ({
  useWorkspaceAuthorization: () => ({
    isAuthorizingDirectory: false,
    selectAndAuthorizeDirectory: selectAndAuthorizeDirectoryMock,
  }),
}))

vi.mock('@/components/chat/SlashCommandPopover', () => ({
  SlashCommandPopover: () => null,
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const map: Record<string, string> = {
        'inputBar.placeholder': '描述你的任务',
        'inputBar.placeholderWithFile': '结合附件继续输入',
        'inputBar.analyzeFile': '请分析附件',
        'inputBar.attachData': '添加附件',
        'inputBar.uploadFile': '上传文件',
      }
      return map[key] ?? key
    },
  }),
}))

import { useChatStore } from '@/stores/chatStore'
import { useSkillStore } from '@/stores/skillStore'
import { ChatBottomArea } from '../ChatBottomArea'

describe('ChatBottomArea', () => {
  beforeEach(() => {
    chatState.isStreaming = false
    vi.clearAllMocks()
    useChatStore.setState({
      activeConversationId: 'conv-chat-bottom',
      conversations: [],
      messages: [],
    })
    useSkillStore.setState({
      skills: [
        { id: 'salary-query', displayName: '薪酬查询', description: '', source: 'local', hasWorkflow: true, icon: '', category: 'general', triggerText: '/salary-query', shortDescription: '', displayNameEn: 'Salary Query', shortDescriptionEn: '' },
      ],
      recommendedIds: [],
      isLoading: false,
    })
  })

  it('hides project button but keeps permission label and tips', () => {
    render(<ChatBottomArea />)

    expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
    expect(screen.getByText('完全访问权限')).toBeInTheDocument()
    expect(screen.getByText('Enter 发送')).toBeInTheDocument()
    expect(screen.getByText('Shift+Enter 换行')).toBeInTheDocument()
  })

  it('sends message on Enter without skill id param', async () => {
    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'hello' },
    })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('hello', undefined)
    })
  })

  it('shows stop while streaming', () => {
    chatState.isStreaming = true
    render(<ChatBottomArea />)

    fireEvent.click(screen.getByRole('button', { name: '停止' }))
    expect(stopCurrentStreamMock).toHaveBeenCalled()
  })

  it('sends slash-prefixed text verbatim (skill id not in IPC)', async () => {
    render(<ChatBottomArea />)

    // Type a slash command that is NOT in the skill store (unknown skill)
    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: '/not-a-skill hello' },
    })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('/not-a-skill hello', undefined)
    })
  })

  it('typing /salary-query expands to triggerText in input via useSkillComposer', () => {
    render(<ChatBottomArea />)

    // Type the skill command with a space-separated tail — useSkillComposer will
    // replace the slash-command prefix with triggerText
    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: '/salary-query 你好' },
    })

    // The input value should now be the triggerText + tail
    // '/salary-query' triggerText + ' 你好' tail
    expect(screen.getByRole('textbox')).toHaveValue('/salary-query 你好')
    // No selectedSkillCommands on the store
    expect((useChatStore.getState() as Record<string, unknown>)['selectedSkillCommands']).toBeUndefined()
  })

  it('clears input after successful send, no skill state involved', async () => {
    sendUserMessageMock.mockResolvedValueOnce(true)

    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello' } })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(screen.getByRole('textbox')).toHaveValue('')
    })

    expect(sendUserMessageMock).toHaveBeenCalledWith('hello', undefined)
  })
})
