import '@testing-library/jest-dom'
import { useRef } from 'react'
import { describe, expect, it, beforeEach, vi } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { RichComposer } from '../RichComposer'
import type { RichComposerHandle } from '../RichComposer'
import {
  useComposerAttachmentPaste,
  __test_setLastPasteHasFile,
} from '../useComposerAttachmentPaste'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

const mockResolvePastedPaths = vi.fn()
const mockSaveClipboardImage = vi.fn()
const mockReadClipboardFilePaths = vi.fn()
const mockPushToast = vi.fn()

vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    resolvePastedPaths: mockResolvePastedPaths,
    saveClipboardImage: mockSaveClipboardImage,
  }),
}))

vi.mock('@/lib/tauri', () => ({
  readClipboardFilePaths: () => mockReadClipboardFilePaths(),
  saveClipboardImageToWorkspaceStaging: vi.fn(),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: { getState: () => ({ push: mockPushToast }) },
}))

function Harness() {
  const ref = useRef<RichComposerHandle>(null)
  useComposerAttachmentPaste(ref)
  return <RichComposer ref={ref} onSubmit={() => {}} />
}

function dispatchPaste(
  types: string[],
  items: Array<{ kind: string; type: string; getAsFile: () => File | null }> = [],
) {
  const editorDom = document.querySelector('.ProseMirror') as HTMLElement
  // Build a minimal ClipboardEvent — jsdom's ClipboardEvent constructor is limited.
  const itemsObj: Record<number, unknown> & { length: number } = { length: items.length }
  items.forEach((item, i) => {
    itemsObj[i] = item
  })
  const clipboardData = {
    types,
    items: itemsObj as unknown as DataTransferItemList,
    getData: () => '',
  }
  const event = new Event('paste', { bubbles: true, cancelable: true }) as Event & {
    clipboardData: typeof clipboardData
  }
  Object.defineProperty(event, 'clipboardData', { value: clipboardData, configurable: true })
  editorDom.dispatchEvent(event)
  return event
}

beforeEach(() => {
  mockResolvePastedPaths.mockReset()
  mockSaveClipboardImage.mockReset()
  mockReadClipboardFilePaths.mockReset()
  mockPushToast.mockReset()
  __test_setLastPasteHasFile(false)
})

describe('useComposerAttachmentPaste', () => {
  it('plain text paste → not intercepted (lastPasteHasFile=false)', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(false)
    const event = dispatchPaste(['text/plain'])
    expect(event.defaultPrevented).toBe(false)
    expect(mockReadClipboardFilePaths).not.toHaveBeenCalled()
    unmount()
  })

  it('file paths paste → preventDefault + insertAttachmentTokens', async () => {
    mockReadClipboardFilePaths.mockResolvedValue(['/abs/a.pdf'])
    mockResolvePastedPaths.mockResolvedValue([
      {
        id: 'a',
        fileName: 'a.pdf',
        path: '/abs/a.pdf',
        kind: 'file',
        fileType: 'pdf',
        fileSize: 0,
        mimeType: undefined,
        source: 'paste',
      },
    ])
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    const event = dispatchPaste(['Files'])
    expect(event.defaultPrevented).toBe(true)
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('a.pdf')
    })
    unmount()
  })

  it('multiple file paths → all tokens in order', async () => {
    mockReadClipboardFilePaths.mockResolvedValue(['/p/x.pdf', '/p/y.pdf'])
    mockResolvePastedPaths.mockResolvedValue([
      { id: 'x', fileName: 'x.pdf', path: '/p/x.pdf', kind: 'file', fileType: 'pdf', fileSize: 0, mimeType: undefined, source: 'paste' },
      { id: 'y', fileName: 'y.pdf', path: '/p/y.pdf', kind: 'file', fileType: 'pdf', fileSize: 0, mimeType: undefined, source: 'paste' },
    ])
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    dispatchPaste(['Files'])
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('x.pdf')
      expect(html).toContain('y.pdf')
    })
    unmount()
  })

  it('all paths rejected → toast, no token', async () => {
    mockReadClipboardFilePaths.mockResolvedValue(['/'])
    mockResolvePastedPaths.mockResolvedValue([])
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    dispatchPaste(['Files'])
    await waitFor(() => {
      expect(mockPushToast).toHaveBeenCalled()
    })
    expect(document.querySelector('.ProseMirror')?.innerHTML ?? '').not.toContain(
      'data-rich-composer-attachment-token',
    )
    unmount()
  })

  it('image blob paste → saveClipboardImage + insertAttachmentTokens', async () => {
    mockReadClipboardFilePaths.mockResolvedValue([])
    mockSaveClipboardImage.mockResolvedValue({
      id: 'img1',
      fileName: 'pasted.png',
      path: '/tmp/pasted.png',
      kind: 'image',
      fileType: 'image',
      fileSize: 100,
      mimeType: 'image/png',
      source: 'clipboard-image',
    })
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    const fakeFile = new File([new Uint8Array([1, 2, 3])], 'pasted.png', { type: 'image/png' })
    dispatchPaste(['Files'], [
      {
        kind: 'file',
        type: 'image/png',
        getAsFile: () => fakeFile,
      },
    ])
    await waitFor(() => {
      expect(mockSaveClipboardImage).toHaveBeenCalled()
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('pasted.png')
    })
    unmount()
  })

  it('paths take priority over image blob (matches existing useComposerPaste behavior)', async () => {
    mockReadClipboardFilePaths.mockResolvedValue(['/abs/x.pdf'])
    mockResolvePastedPaths.mockResolvedValue([
      { id: 'x', fileName: 'x.pdf', path: '/abs/x.pdf', kind: 'file', fileType: 'pdf', fileSize: 0, mimeType: undefined, source: 'paste' },
    ])
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    const fakeFile = new File([new Uint8Array([1, 2, 3])], 'pasted.png', { type: 'image/png' })
    dispatchPaste(['Files'], [
      {
        kind: 'file',
        type: 'image/png',
        getAsFile: () => fakeFile,
      },
    ])
    await waitFor(() => {
      expect(mockResolvePastedPaths).toHaveBeenCalled()
    })
    expect(mockSaveClipboardImage).not.toHaveBeenCalled()
    unmount()
  })
})
