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

    expect(screen.getByText('选择左侧文件以预览')).toBeInTheDocument()
    expect(previewMock.getFilePreview).not.toHaveBeenCalled()
  })

  it('renders the close action in the preview header', () => {
    const onClosePreview = vi.fn()
    previewMock.getFilePreview.mockReturnValue(new Promise(() => {}))

    render(<FilePreviewPane target={target} onOpenExternal={() => {}} onClosePreview={onClosePreview} />)

    const header = screen.getByTestId('file-preview-header')
    const closeButton = screen.getByRole('button', { name: 'Close preview' })
    expect(header).toContainElement(closeButton)

    fireEvent.click(closeButton)

    expect(onClosePreview).toHaveBeenCalledTimes(1)
  })

  it('uses the compact fixed app chrome height for the preview header', () => {
    previewMock.getFilePreview.mockReturnValue(new Promise(() => {}))

    render(<FilePreviewPane target={target} onOpenExternal={() => {}} />)

    const header = screen.getByTestId('file-preview-header')
    expect(header).toHaveClass('h-12')
    expect(header).not.toHaveClass('py-2')
  })

  it('keeps the preview header as clickable app chrome above the macOS overlay drag strip', () => {
    previewMock.getFilePreview.mockReturnValue(new Promise(() => {}))

    render(<FilePreviewPane target={target} onOpenExternal={() => {}} />)

    const header = screen.getByTestId('file-preview-header')
    expect(header).toHaveClass('relative', 'z-20')
    expect(header).toHaveAttribute('data-tauri-drag-region')
  })

  it('loads and renders markdown content', async () => {
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'markdown',
      fileName: 'summary.md',
      mimeType: 'text/markdown',
      content: '# Summary',
    })

    render(<FilePreviewPane target={target} onOpenExternal={() => {}} />)

    expect(screen.getByText('正在加载预览...')).toBeInTheDocument()
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

  it('renders image preview responses inside the app', async () => {
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'image',
      fileName: 'mock-status-chart.png',
      mimeType: 'image/png',
      dataUrl: 'data:image/png;base64,iVBORw==',
    })

    render(<FilePreviewPane target={{ ...target, fileName: 'mock-status-chart.png', fileType: 'png' }} onOpenExternal={() => {}} />)

    const image = await screen.findByRole('img', { name: 'mock-status-chart.png' })
    expect(image).toHaveAttribute('src', 'data:image/png;base64,iVBORw==')
  })

  it('offers download from the image preview context menu', async () => {
    const onDownload = vi.fn()
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'image',
      fileName: 'mock-status-chart.png',
      mimeType: 'image/png',
      dataUrl: 'data:image/png;base64,iVBORw==',
    })

    render(
      <FilePreviewPane
        target={{ ...target, fileName: 'mock-status-chart.png', fileType: 'png' }}
        onOpenExternal={() => {}}
        onDownload={onDownload}
      />,
    )

    const image = await screen.findByRole('img', { name: 'mock-status-chart.png' })
    fireEvent.contextMenu(image)
    fireEvent.click(await screen.findByRole('menuitem', { name: '下载到...' }))

    expect(onDownload).toHaveBeenCalledWith({ ...target, fileName: 'mock-status-chart.png', fileType: 'png' })
  })

  it('allows scripts inside generated html preview while keeping it sandboxed', async () => {
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'html',
      fileName: 'report.html',
      mimeType: 'text/html',
      content: '<h1>Report</h1><script>document.body.dataset.ready = "true"</script>',
      sandbox: true,
    })

    render(<FilePreviewPane target={{ ...target, fileName: 'report.html', fileType: 'html' }} onOpenExternal={() => {}} />)

    const frame = await screen.findByTitle('report.html')
    expect(frame).toHaveAttribute('sandbox', 'allow-scripts')
    expect(frame).toHaveAttribute('srcdoc', '<h1>Report</h1><script>document.body.dataset.ready = "true"</script>')
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

    fireEvent.click(screen.getByRole('button', { name: '用默认应用打开' }))

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

    fireEvent.click(screen.getByRole('button', { name: '重试' }))
    expect(await screen.findByText('retried content')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '用默认应用打开' }))
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
