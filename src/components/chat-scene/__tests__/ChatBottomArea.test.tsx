import '@testing-library/jest-dom'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, waitFor, fireEvent, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ChatBottomArea } from '../ChatBottomArea'
import { useChatStore } from '@/stores/chatStore'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

const mockSendUserMessage = vi.fn()
const mockStopCurrentStream = vi.fn()
let mockIsStreaming = false
const mockPickAttachments = vi.fn()

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: mockSendUserMessage,
    isStreaming: mockIsStreaming,
    stopCurrentStream: mockStopCurrentStream,
  }),
}))

vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: mockPickAttachments,
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

beforeEach(() => {
  mockSendUserMessage.mockReset().mockResolvedValue(undefined)
  mockStopCurrentStream.mockReset()
  mockPickAttachments.mockReset().mockResolvedValue([])
  mockIsStreaming = false
  useChatStore.setState({ activeConversationId: 'conv-1' })
})

describe('ChatBottomArea', () => {
  it('renders RichComposer', async () => {
    render(<ChatBottomArea />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  })

  it('typing + Enter calls sendUserMessage with markdown text and no attachments', async () => {
    const user = userEvent.setup()
    render(<ChatBottomArea />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'hello')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalledTimes(1))
    expect(mockSendUserMessage.mock.calls[0][0]).toBe('hello')
    expect(mockSendUserMessage.mock.calls[0][1]).toBeUndefined()
  })

  it('attachment-only Enter sends markdown with file:// link and attachment array', async () => {
    mockPickAttachments.mockResolvedValueOnce([
      {
        id: 'a',
        fileName: 'a.pdf',
        path: '/p/a.pdf',
        kind: 'file',
        fileType: 'pdf',
        fileSize: 0,
        mimeType: undefined,
        source: 'picker',
      },
    ])
    const { container } = render(<ChatBottomArea />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const attachBtn = container.querySelector('[aria-label="添加附件"]') as HTMLElement
    await act(async () => {
      attachBtn.click()
    })
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('a.pdf')
    })
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    const [text, files] = mockSendUserMessage.mock.calls[0]
    expect(text).toContain('[附件: a.pdf](<file:///p/a.pdf>)')
    expect(files).toHaveLength(1)
    expect(files[0].id).toBe('a')
  })

  it('isStreaming → shows stop button, click calls stopCurrentStream', async () => {
    mockIsStreaming = true
    const { container } = render(<ChatBottomArea />)
    const stopBtn = await waitFor(() => container.querySelector('[aria-label="停止"]') as HTMLElement)
    fireEvent.click(stopBtn)
    expect(mockStopCurrentStream).toHaveBeenCalledTimes(1)
    expect(mockSendUserMessage).not.toHaveBeenCalled()
  })

  it('empty Enter does not call sendUserMessage', async () => {
    render(<ChatBottomArea />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    expect(mockSendUserMessage).not.toHaveBeenCalled()
  })
})
