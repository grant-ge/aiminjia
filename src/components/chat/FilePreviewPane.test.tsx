import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, beforeEach, vi } from 'vitest'

import { FilePreviewPane } from './FilePreviewPane'
import type { PreviewTarget } from './generatedFileActions'

const previewMock = vi.hoisted(() => ({
  getFilePreview: vi.fn(),
}))

vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    getFilePreview: previewMock.getFilePreview,
  }
})

const target: PreviewTarget = {
  fileId: 'gf-1',
  conversationId: 'conv-1',
  fileName: 'summary.md',
  fileType: 'markdown',
}

describe('FilePreviewPane', () => {
  beforeEach(() => {
    previewMock.getFilePreview.mockReset()
  })

  it('shows an empty state when no target is selected without loading preview content', () => {
    render(<FilePreviewPane target={null} onOpenExternal={vi.fn()} />)

    expect(screen.getByText('选择一个产物进行预览')).toBeInTheDocument()
    expect(previewMock.getFilePreview).not.toHaveBeenCalled()
  })

  it('loads and renders markdown content', async () => {
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'markdown',
      fileName: 'summary.md',
      mimeType: 'text/markdown',
      content: '# Summary',
    })

    render(<FilePreviewPane target={target} onOpenExternal={() => {}} />)

    expect(screen.getByText('正在加载预览')).toBeInTheDocument()
    expect(await screen.findByRole('heading', { name: 'Summary' })).toBeInTheDocument()
    expect(previewMock.getFilePreview).toHaveBeenCalledWith('gf-1', 'conv-1')
  })

  it('renders text, json, and csv preview responses as preformatted content', async () => {
    previewMock.getFilePreview.mockResolvedValueOnce({
      kind: 'text',
      fileName: 'notes.txt',
      mimeType: 'text/plain',
      content: 'line 1\nline 2',
    })
    const { rerender } = render(<FilePreviewPane target={{ ...target, fileId: 'gf-text', fileName: 'notes.txt' }} onOpenExternal={() => {}} />)
    const textPreview = await screen.findByText((_content, element) => element?.tagName === 'PRE' && element.textContent === 'line 1\nline 2')
    expect(textPreview).toBeInTheDocument()
    expect(textPreview.tagName).toBe('PRE')

    previewMock.getFilePreview.mockResolvedValueOnce({
      kind: 'json',
      fileName: 'data.json',
      mimeType: 'application/json',
      content: '{"ok":true}',
    })
    rerender(<FilePreviewPane target={{ ...target, fileId: 'gf-json', fileName: 'data.json' }} onOpenExternal={() => {}} />)
    expect(await screen.findByText('{"ok":true}')).toBeInTheDocument()
    expect(screen.getByText('{"ok":true}').tagName).toBe('PRE')

    previewMock.getFilePreview.mockResolvedValueOnce({
      kind: 'csv',
      fileName: 'data.csv',
      mimeType: 'text/csv',
      content: 'name,value\na,1',
    })
    rerender(<FilePreviewPane target={{ ...target, fileId: 'gf-csv', fileName: 'data.csv' }} onOpenExternal={() => {}} />)
    const csvPreview = await screen.findByText((_content, element) => element?.tagName === 'PRE' && element.textContent === 'name,value\na,1')
    expect(csvPreview).toBeInTheDocument()
    expect(csvPreview.tagName).toBe('PRE')
  })

  it('renders html preview responses in a sandboxed iframe', async () => {
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'html',
      fileName: 'report.html',
      mimeType: 'text/html',
      content: '<h1>Report</h1>',
      sandbox: true,
    })

    render(<FilePreviewPane target={{ ...target, fileName: 'report.html', fileType: 'html' }} onOpenExternal={() => {}} />)

    const frame = await screen.findByTitle('report.html')
    expect(frame).toHaveAttribute('sandbox', '')
    expect(frame).toHaveAttribute('srcdoc', '<h1>Report</h1>')
  })

  it('renders unsupported preview responses and keeps external open available', async () => {
    const onOpenExternal = vi.fn()
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'unsupported',
      fileName: 'table.xlsx',
      reason: 'Preview for excel files is not supported',
    })

    render(<FilePreviewPane target={{ ...target, fileName: 'table.xlsx', fileType: 'excel' }} onOpenExternal={onOpenExternal} />)

    expect(await screen.findByText('Preview for excel files is not supported')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Open with default app' }))

    expect(onOpenExternal).toHaveBeenCalledWith({ ...target, fileName: 'table.xlsx', fileType: 'excel' })
  })

  it('renders preview loading errors with retry and external open actions', async () => {
    const onOpenExternal = vi.fn()
    previewMock.getFilePreview
      .mockRejectedValueOnce(new Error('not found'))
      .mockResolvedValueOnce({
        kind: 'text',
        fileName: 'summary.md',
        mimeType: 'text/plain',
        content: 'retried content',
      })

    render(<FilePreviewPane target={target} onOpenExternal={onOpenExternal} />)

    expect(await screen.findByText('not found')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(await screen.findByText('retried content')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Open with default app' }))
    expect(onOpenExternal).toHaveBeenCalledWith(target)
    expect(previewMock.getFilePreview).toHaveBeenCalledTimes(2)
  })

  it('loads new content when the target changes without allowing stale responses to overwrite it', async () => {
    let resolveOldPreview: (value: unknown) => void = () => {}
    previewMock.getFilePreview
      .mockReturnValueOnce(new Promise((resolve) => {
        resolveOldPreview = resolve
      }))
      .mockResolvedValueOnce({
        kind: 'text',
        fileName: 'new.txt',
        mimeType: 'text/plain',
        content: 'new content',
      })

    const { rerender } = render(<FilePreviewPane target={target} onOpenExternal={() => {}} />)

    const nextTarget = { ...target, fileId: 'gf-2', fileName: 'new.txt', fileType: 'text' }
    rerender(<FilePreviewPane target={nextTarget} onOpenExternal={() => {}} />)

    expect(await screen.findByText('new content')).toBeInTheDocument()

    resolveOldPreview({
      kind: 'text',
      fileName: 'old.txt',
      mimeType: 'text/plain',
      content: 'old content',
    })

    await waitFor(() => {
      expect(screen.queryByText('old content')).not.toBeInTheDocument()
    })
    expect(previewMock.getFilePreview).toHaveBeenNthCalledWith(1, 'gf-1', 'conv-1')
    expect(previewMock.getFilePreview).toHaveBeenNthCalledWith(2, 'gf-2', 'conv-1')
  })
})
