import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useHomeStore } from '@/stores/homeStore'
import type { PendingAttachment } from '@/hooks/useChatAttachments'

import { HomeTaskComposerCard } from '../HomeTaskComposerCard'

const sendUserMessageMock = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
const pickAttachmentsMock = vi.hoisted(() => vi.fn(async (): Promise<PendingAttachment[]> => []))

vi.mock('@/lib/tauri', () => ({
  getDefaultFolder: vi.fn().mockResolvedValue({
    id: 'default',
    rootPath: '/Users/test/.renlijia/defaultFolder',
    displayName: '测试默认项目', // distinct from static fallback '默认项目'
  }),
  pickLocalDirectory: vi.fn(),
  authorizeLocalDirectory: vi.fn().mockResolvedValue({ id: 'ws1', rootPath: '/tmp/proj', displayName: 'proj' }),
  createConversation: vi.fn().mockResolvedValue('conv-123'),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: sendUserMessageMock }),
}))

vi.mock('@/hooks/useChatAttachments', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/hooks/useChatAttachments')>()
  return {
    ...actual,
    useChatAttachments: () => ({
      isPickingAttachments: false,
      pickAttachments: pickAttachmentsMock,
      resolvePastedPaths: vi.fn(async () => []),
      saveClipboardImage: vi.fn(),
    }),
  }
})

vi.mock('@/stores/chatStore', () => ({
  useChatStore: {
    getState: () => ({
      conversations: [],
      setConversations: vi.fn(),
      setActiveConversation: vi.fn(),
      setMessages: vi.fn(),
    }),
  },
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: {
    getState: () => ({ setRoute: vi.fn(), consumePrefillText: vi.fn(() => null) }),
  },
}))

vi.mock('@/stores/homeStore', () => ({
  useHomeStore: vi.fn().mockReturnValue({
    selectedWorkspace: null,
    setSelectedWorkspace: vi.fn(),
  }),
}))

vi.mock('@/components/chat/SkillPopover', () => ({ SkillPopover: () => null }))

describe('HomeTaskComposerCard', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    // Restore default homeStore mock so each test starts clean
    vi.mocked(useHomeStore).mockReturnValue({
      selectedWorkspace: null,
      setSelectedWorkspace: vi.fn(),
    })
    pickAttachmentsMock.mockResolvedValue([])
  })

  it('shows 测试默认项目 after loading default folder', async () => {
    render(<HomeTaskComposerCard />)
    await waitFor(() => {
      expect(screen.getByText('测试默认项目')).toBeInTheDocument()
    })
  })

  it('updates project label after user picks a directory', async () => {
    const { pickLocalDirectory } = await import('@/lib/tauri')
    vi.mocked(pickLocalDirectory).mockResolvedValueOnce('/Users/test/myproject')

    render(<HomeTaskComposerCard />)
    await waitFor(() => screen.getByText('测试默认项目'))

    fireEvent.click(screen.getByText('测试默认项目'))
    await waitFor(() => {
      expect(screen.getByText('myproject')).toBeInTheDocument()
    })
    expect(vi.mocked(pickLocalDirectory)).toHaveBeenCalledOnce()
  })

  it('persists workspace to homeStore on pick', async () => {
    const setSelectedWorkspace = vi.fn()
    vi.mocked(useHomeStore).mockReturnValue({
      selectedWorkspace: null,
      setSelectedWorkspace,
    })

    const { pickLocalDirectory } = await import('@/lib/tauri')
    vi.mocked(pickLocalDirectory).mockResolvedValueOnce('/Users/test/myproject')

    render(<HomeTaskComposerCard />)
    await waitFor(() => screen.getByText('测试默认项目'))
    fireEvent.click(screen.getByText('测试默认项目'))

    await waitFor(() => {
      expect(setSelectedWorkspace).toHaveBeenCalledWith({
        id: 'myproject',
        rootPath: '/Users/test/myproject',
        displayName: 'myproject',
      })
    })
  })

  it('uses the attachment button to attach a local path and sends that path without uploading', async () => {
    pickAttachmentsMock.mockResolvedValueOnce([
      {
        id: '/Users/test/report.csv',
        fileName: 'report.csv',
        path: '/Users/test/report.csv',
        kind: 'file',
        fileType: 'csv',
        fileSize: 0,
        source: 'picker',
      },
    ])

    render(<HomeTaskComposerCard />)

    fireEvent.click(screen.getByRole('button', { name: '添加附件' }))

    await waitFor(() => {
      expect(pickAttachmentsMock).toHaveBeenCalledOnce()
    })
    expect(await screen.findByText('report.csv')).toBeInTheDocument()

    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: '请分析' },
    })
    fireEvent.click(screen.getByRole('button', { name: '发送' }))

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('请分析', [
        {
          id: '/Users/test/report.csv',
          fileName: 'report.csv',
          filePath: '/Users/test/report.csv',
          kind: 'file',
          fileSize: 0,
          fileType: 'csv',
          mimeType: undefined,
        },
      ])
    })
  })

  it('sends attachment-only home tasks with the local path payload', async () => {
    pickAttachmentsMock.mockResolvedValueOnce([
      {
        id: '/Users/test/only.csv',
        fileName: 'only.csv',
        path: '/Users/test/only.csv',
        kind: 'file',
        fileType: 'csv',
        fileSize: 0,
        source: 'picker',
      },
    ])

    render(<HomeTaskComposerCard />)

    fireEvent.click(screen.getByRole('button', { name: '添加附件' }))
    expect(await screen.findByText('only.csv')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '发送' }))

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('请分析附件', [
        expect.objectContaining({
          id: '/Users/test/only.csv',
          filePath: '/Users/test/only.csv',
        }),
      ])
    })
  })
})
