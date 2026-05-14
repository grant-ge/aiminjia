import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { MessageList } from './MessageList'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { openGeneratedFile, revealFileInFolder } from '@/lib/tauri'
import type { GeneratedFile, Message } from '@/types/message'

vi.mock('@/lib/tauri', () => ({
  openGeneratedFile: vi.fn().mockResolvedValue(undefined),
  revealFileInFolder: vi.fn().mockResolvedValue(undefined),
  getTeamOverview: vi.fn().mockResolvedValue({ conversationId: '', teams: [] }),
  getTeammateTranscript: vi.fn().mockResolvedValue([]),
  onMessageUpdated: vi.fn().mockResolvedValue(() => {}),
  onToolCompleted: vi.fn().mockResolvedValue(() => {}),
}))

const openGeneratedFileMock = vi.mocked(openGeneratedFile)
const revealFileInFolderMock = vi.mocked(revealFileInFolder)

function generatedFile(overrides: Partial<GeneratedFile> = {}): GeneratedFile {
  return {
    id: 'gf-1',
    title: 'Summary',
    fileName: 'summary.md',
    filePath: '/tmp/summary.md',
    fileType: 'markdown',
    fileSize: 1024,
    category: 'report',
    version: 1,
    isLatest: true,
    createdAt: '2026-04-28T00:00:00Z',
    description: 'Generated summary',
    ...overrides,
  }
}

function messageWithFile(file: GeneratedFile): Message[] {
  return [
    {
      id: 'u-1',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-28T00:00:00Z',
      content: { text: 'create file' },
    },
    {
      id: 'a-1',
      conversationId: 'conv-1',
      role: 'assistant',
      createdAt: '2026-04-28T00:00:01Z',
      content: { text: 'done', generatedFiles: [file] },
    },
  ]
}

function resetStores(activeConversationId: string | null = 'conv-1') {
  useChatStore.setState({
    conversations: [],
    activeConversationId,
    messages: [],
    busyConversations: new Set(),
    streamStates: {},
    taskStates: {},
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],
  })
  useGeneratedFilePreviewStore.setState({ target: null })
  useNotificationStore.setState({ notifications: [] })
}

function renderWithFile(file: GeneratedFile, activeConversationId: string | null = 'conv-1') {
  resetStores(activeConversationId)
  useChatStore.setState({ messages: messageWithFile(file) })
  render(<MessageList />)
}

function openActionsMenu() {
  fireEvent.pointerDown(screen.getByRole('button', { name: '更多操作：Summary' }))
}

beforeEach(() => {
  vi.clearAllMocks()
  openGeneratedFileMock.mockResolvedValue(undefined)
  revealFileInFolderMock.mockResolvedValue(undefined)
  resetStores()
})

describe('MessageList generated file actions', () => {
  it('uses the generated file owner conversation for preview/open/reveal when active conversation differs', async () => {
    renderWithFile(generatedFile(), 'conv-2')

    fireEvent.click(screen.getByRole('button', { name: '预览 Summary' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })

    openActionsMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '用默认应用打开' }))
    await waitFor(() => expect(openGeneratedFileMock).toHaveBeenCalledWith('gf-1', 'conv-1'))

    openActionsMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '在文件夹中显示' }))
    await waitFor(() => expect(revealFileInFolderMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
  })

  it('clears stale preview target when rendering a different active conversation', () => {
    resetStores('conv-1')
    useGeneratedFilePreviewStore.getState().openPreview({
      fileId: 'old-file',
      conversationId: 'conv-old',
      fileName: 'old.md',
      fileType: 'markdown',
    })

    render(<MessageList />)

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('opens previewable file in the generated file preview store from primary action', () => {
    renderWithFile(generatedFile())

    fireEvent.click(screen.getByRole('button', { name: '预览 Summary' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
    expect(openGeneratedFileMock).not.toHaveBeenCalled()
  })

  it('opens non-previewable excel file externally from primary action', async () => {
    renderWithFile(
      generatedFile({
        fileName: 'summary.xlsx',
        fileType: 'excel',
        actions: [
          { type: 'preview', enabled: false, label: 'Preview' },
          { type: 'open', enabled: true, label: 'Open' },
          { type: 'reveal', enabled: true, label: 'Reveal' },
        ],
      }),
    )

    fireEvent.click(screen.getByRole('button', { name: '打开 Summary' }))

    await waitFor(() => expect(openGeneratedFileMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('reveals a generated file from the dropdown menu', async () => {
    renderWithFile(generatedFile())

    openActionsMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '在文件夹中显示' }))

    await waitFor(() => expect(revealFileInFolderMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
  })

  it('pushes an error toast when open external fails', async () => {
    openGeneratedFileMock.mockRejectedValueOnce(new Error('boom'))
    renderWithFile(generatedFile({ fileName: 'summary.xlsx', fileType: 'excel' }))

    fireEvent.click(screen.getByRole('button', { name: '打开 Summary' }))

    await waitFor(() => {
      expect(useNotificationStore.getState().notifications.at(-1)).toMatchObject({
        level: 'error',
        title: '无法打开文件',
      })
    })
  })

  it('previews using file owner conversation even when active conversation is missing', () => {
    renderWithFile(generatedFile(), null)

    fireEvent.click(screen.getByRole('button', { name: '预览 Summary' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
    expect(openGeneratedFileMock).not.toHaveBeenCalled()
    expect(revealFileInFolderMock).not.toHaveBeenCalled()
    expect(useNotificationStore.getState().notifications).toEqual([])
  })

  it('opens using file owner conversation even when active conversation is missing', async () => {
    renderWithFile(generatedFile({ fileName: 'summary.xlsx', fileType: 'excel' }), null)

    fireEvent.click(screen.getByRole('button', { name: '打开 Summary' }))

    await waitFor(() => expect(openGeneratedFileMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
    expect(useNotificationStore.getState().notifications).toEqual([])
  })

  it('reveals using file owner conversation even when active conversation is missing', async () => {
    renderWithFile(generatedFile(), null)

    openActionsMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '在文件夹中显示' }))

    await waitFor(() => expect(revealFileInFolderMock).toHaveBeenCalledWith('gf-1', 'conv-1'))
    expect(useNotificationStore.getState().notifications).toEqual([])
  })
})
