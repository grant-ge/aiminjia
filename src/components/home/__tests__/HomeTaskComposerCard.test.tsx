import '@testing-library/jest-dom'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, waitFor, fireEvent, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { HomeTaskComposerCard } from '../HomeTaskComposerCard'
import { useChatStore } from '@/stores/chatStore'
import { useUiStore } from '@/stores/uiStore'
import { useHomeStore } from '@/stores/homeStore'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

const mockSendUserMessage = vi.fn()
const mockCreateConversation = vi.fn()
const mockAuthorizeLocalDirectory = vi.fn()
const mockGetDefaultFolder = vi.fn()
const mockPickLocalDirectory = vi.fn()
const mockPickAttachments = vi.fn()

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: mockSendUserMessage, isStreaming: false, stopCurrentStream: vi.fn() }),
}))

vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: mockPickAttachments,
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}))

vi.mock('@/lib/tauri', () => ({
  authorizeLocalDirectory: (...args: unknown[]) => mockAuthorizeLocalDirectory(...args),
  createConversation: () => mockCreateConversation(),
  getDefaultFolder: () => mockGetDefaultFolder(),
  pickLocalDirectory: (opts: unknown) => mockPickLocalDirectory(opts),
  readClipboardFilePaths: vi.fn().mockResolvedValue([]),
  saveClipboardImageToWorkspaceStaging: vi.fn(),
}))

beforeEach(() => {
  mockSendUserMessage.mockReset().mockResolvedValue(undefined)
  mockCreateConversation.mockReset().mockResolvedValue('new-conv-1')
  mockAuthorizeLocalDirectory.mockReset().mockResolvedValue(undefined)
  mockGetDefaultFolder.mockReset().mockResolvedValue({ id: 'default', rootPath: '/home', displayName: '默认' })
  mockPickLocalDirectory.mockReset()
  mockPickAttachments.mockReset().mockResolvedValue([])
  useChatStore.setState({ activeConversationId: null, conversations: [], messages: [] })
  useUiStore.setState({ route: { kind: 'home' }, prefillText: undefined })
  useHomeStore.setState({ selectedWorkspace: null })
})

describe('HomeTaskComposerCard', () => {
  it('renders RichComposer', async () => {
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  })

  it('Enter with text → creates conversation, switches route, sends message', async () => {
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'analyze sales')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockCreateConversation).toHaveBeenCalled()
    expect(useChatStore.getState().activeConversationId).toBe('new-conv-1')
    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'new-conv-1' })
    expect(mockSendUserMessage.mock.calls[0][0]).toBe('analyze sales')
  })

  it('attachment via picker shows token + Enter sends with file refs', async () => {
    mockPickAttachments.mockResolvedValueOnce([
      {
        id: 'p1',
        fileName: 'plan.pdf',
        path: '/p/plan.pdf',
        kind: 'file',
        fileType: 'pdf',
        fileSize: 0,
        mimeType: undefined,
        source: 'picker',
      },
    ])
    const { container } = render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const attachBtn = container.querySelector('[aria-label="添加附件"]') as HTMLElement
    await act(async () => {
      attachBtn.click()
    })
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('plan.pdf')
    })
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    const [text, files] = mockSendUserMessage.mock.calls[0]
    expect(text).toContain('[附件: plan.pdf](<file:///p/plan.pdf>)')
    expect(files).toHaveLength(1)
    expect(files[0].filePath).toBe('/p/plan.pdf')
  })

  it('non-default workspace → authorizeLocalDirectory called before send', async () => {
    useHomeStore.setState({
      selectedWorkspace: { id: 'proj', rootPath: '/Users/me/proj', displayName: 'proj' },
    })
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'go')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockAuthorizeLocalDirectory).toHaveBeenCalledWith('/Users/me/proj', 'new-conv-1')
  })

  it('default workspace → authorizeLocalDirectory NOT called', async () => {
    // selectedWorkspace null → getDefaultFolder returns id=default
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'go')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockAuthorizeLocalDirectory).not.toHaveBeenCalled()
  })
})
