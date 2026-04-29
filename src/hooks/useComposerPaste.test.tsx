import { describe, expect, it, vi, beforeEach } from 'vitest'
import { renderHook } from '@testing-library/react'

import { useComposerPaste } from './useComposerPaste'
import type { PendingAttachment } from './useChatAttachments'

const saveClipboardImageMock = vi.fn()
const resolvePastedPathsMock = vi.fn()

vi.mock('./useChatAttachments', () => ({
  useChatAttachments: () => ({
    saveClipboardImage: saveClipboardImageMock,
    resolvePastedPaths: resolvePastedPathsMock,
    pickAttachments: vi.fn(),
    isPickingAttachments: false,
  }),
}))

const readClipboardFilePathsMock = vi.fn()
vi.mock('@/lib/tauri', () => ({
  readClipboardFilePaths: () => readClipboardFilePathsMock(),
}))

function makeImagePasteEvent(file: File) {
  const item = {
    kind: 'file',
    type: file.type,
    getAsFile: () => file,
  } as unknown as DataTransferItem

  return {
    preventDefault: vi.fn(),
    clipboardData: {
      items: [item] as unknown as DataTransferItemList,
      getData: () => '',
    },
  } as unknown as React.ClipboardEvent<HTMLTextAreaElement>
}

function makeTextPasteEvent(text: string, types: string[] = []) {
  return {
    preventDefault: vi.fn(),
    clipboardData: {
      items: [] as unknown as DataTransferItemList,
      getData: (kind: string) => (kind === 'text/plain' ? text : ''),
      types,
    },
  } as unknown as React.ClipboardEvent<HTMLTextAreaElement>
}

const samplePending: PendingAttachment = {
  id: '/tmp/x.png',
  fileName: 'x.png',
  path: '/tmp/x.png',
  kind: 'image',
  fileType: 'image',
  fileSize: 1,
  mimeType: 'image/png',
  source: 'clipboard-image',
}

describe('useComposerPaste', () => {
  beforeEach(() => {
    saveClipboardImageMock.mockReset()
    resolvePastedPathsMock.mockReset()
    readClipboardFilePathsMock.mockReset()
  })

  it('saves clipboard image and emits attachment', async () => {
    saveClipboardImageMock.mockResolvedValue(samplePending)
    const onResolved = vi.fn()
    const { result } = renderHook(() => useComposerPaste({ onAttachmentsResolved: onResolved }))

    const file = new File([new Uint8Array([1, 2, 3])], 'paste.png', { type: 'image/png' })
    const event = makeImagePasteEvent(file)
    result.current.handlePaste(event)

    await new Promise((r) => setTimeout(r, 0))
    expect(event.preventDefault).toHaveBeenCalled()
    expect(saveClipboardImageMock).toHaveBeenCalledTimes(1)
    expect(onResolved).toHaveBeenCalledWith([samplePending])
  })

  it('resolves absolute paths in pasted text', async () => {
    resolvePastedPathsMock.mockResolvedValue([samplePending])
    const onResolved = vi.fn()
    const { result } = renderHook(() => useComposerPaste({ onAttachmentsResolved: onResolved }))

    const event = makeTextPasteEvent('/Users/me/a.png\n/Users/me/dir')
    result.current.handlePaste(event)

    await new Promise((r) => setTimeout(r, 0))
    expect(event.preventDefault).toHaveBeenCalled()
    expect(resolvePastedPathsMock).toHaveBeenCalledWith(['/Users/me/a.png', '/Users/me/dir'])
    expect(onResolved).toHaveBeenCalledWith([samplePending])
  })

  it('does not call onResolved when image save throws', async () => {
    saveClipboardImageMock.mockRejectedValue(new Error('ipc fail'))
    const onResolved = vi.fn()
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const { result } = renderHook(() => useComposerPaste({ onAttachmentsResolved: onResolved }))

    const file = new File([new Uint8Array([1])], 'paste.png', { type: 'image/png' })
    const event = makeImagePasteEvent(file)
    result.current.handlePaste(event)

    await new Promise((r) => setTimeout(r, 0))
    expect(saveClipboardImageMock).toHaveBeenCalled()
    expect(onResolved).not.toHaveBeenCalled()
    expect(errorSpy).toHaveBeenCalled()
    errorSpy.mockRestore()
  })

  it('falls back to native clipboard file paths', async () => {
    readClipboardFilePathsMock.mockResolvedValue(['/Users/me/native.png'])
    resolvePastedPathsMock.mockResolvedValue([samplePending])
    const onResolved = vi.fn()
    const { result } = renderHook(() => useComposerPaste({ onAttachmentsResolved: onResolved }))

    const event = makeTextPasteEvent('foo.zip', ['Files', 'text/plain'])
    result.current.handlePaste(event)

    await new Promise((r) => setTimeout(r, 0))
    expect(event.preventDefault).toHaveBeenCalled()
    expect(readClipboardFilePathsMock).toHaveBeenCalled()
    expect(resolvePastedPathsMock).toHaveBeenCalledWith(['/Users/me/native.png'])
    expect(onResolved).toHaveBeenCalledWith([samplePending])
  })

  it('lets plain text paste through without preventDefault', async () => {
    const onResolved = vi.fn()
    const { result } = renderHook(() => useComposerPaste({ onAttachmentsResolved: onResolved }))

    const event = makeTextPasteEvent('hello world')
    result.current.handlePaste(event)

    await new Promise((r) => setTimeout(r, 0))
    expect(event.preventDefault).not.toHaveBeenCalled()
    expect(readClipboardFilePathsMock).not.toHaveBeenCalled()
    expect(onResolved).not.toHaveBeenCalled()
  })
})
