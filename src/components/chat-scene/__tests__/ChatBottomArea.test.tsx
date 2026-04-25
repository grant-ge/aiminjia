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
import { ChatBottomArea } from '../ChatBottomArea'

describe('ChatBottomArea', () => {
  beforeEach(() => {
    chatState.isStreaming = false
    vi.clearAllMocks()
    useChatStore.setState({
      activeConversationId: 'conv-chat-bottom',
      conversations: [],
      messages: [],
      selectedSkillCommands: {},
    })
  })

  it('hides project button but keeps permission label and tips', () => {
    render(<ChatBottomArea />)

    expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
    expect(screen.getByText('完全访问权限')).toBeInTheDocument()
    expect(screen.getByText('Enter 发送')).toBeInTheDocument()
    expect(screen.getByText('Shift+Enter 换行')).toBeInTheDocument()
  })

  it('sends message on Enter', async () => {
    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'hello' },
    })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('hello', undefined, undefined, undefined)
    })
  })

  it('shows stop while streaming', () => {
    chatState.isStreaming = true
    render(<ChatBottomArea />)

    fireEvent.click(screen.getByRole('button', { name: '停止' }))
    expect(stopCurrentStreamMock).toHaveBeenCalled()
  })

  it('renders selected skill command token from chat store', () => {
    useChatStore.setState({
      selectedSkillCommands: {
        'conv-chat-bottom': {
          id: 'skill-smith',
          label: '创建自己的技能',
          command: '/skill-smith',
        },
      },
    })

    render(<ChatBottomArea />)

    expect(screen.getByText('创建自己的技能')).toBeInTheDocument()
    expect(screen.getByText('/skill-smith')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /当前已加载技能 创建自己的技能/ })).toBeInTheDocument()
  })

  it('does not leak selected skill command across conversations', () => {
    useChatStore.setState({
      activeConversationId: 'conv-other',
      selectedSkillCommands: {
        'conv-chat-bottom': {
          id: 'skill-smith',
          label: '创建自己的技能',
          command: '/skill-smith',
        },
      },
    })

    render(<ChatBottomArea />)

    expect(screen.queryByText('创建自己的技能')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '打开技能选择' })).toBeInTheDocument()
  })

  it('keeps token and input when sendUserMessage reports failure', async () => {
    sendUserMessageMock.mockResolvedValueOnce(false)
    useChatStore.setState({
      selectedSkillCommands: {
        'conv-chat-bottom': {
          id: 'skill-smith',
          label: '创建自己的技能',
          command: '/skill-smith',
        },
      },
    })

    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello' } })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('hello', undefined, undefined, 'skill-smith')
    })

    expect(screen.getByRole('textbox')).toHaveValue('hello')
    expect(screen.getByText('创建自己的技能')).toBeInTheDocument()
  })

  it('clears token and input after successful send', async () => {
    sendUserMessageMock.mockResolvedValueOnce(true)
    useChatStore.setState({
      selectedSkillCommands: {
        'conv-chat-bottom': {
          id: 'skill-smith',
          label: '创建自己的技能',
          command: '/skill-smith',
        },
      },
    })

    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello' } })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(screen.getByRole('textbox')).toHaveValue('')
    })

    expect(sendUserMessageMock).toHaveBeenCalledWith('hello', undefined, undefined, 'skill-smith')
    expect(screen.queryByText('创建自己的技能')).not.toBeInTheDocument()
  })

})
