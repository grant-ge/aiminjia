import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('./FilePreviewPane', () => ({
  FilePreviewPane: ({
    target,
    onClosePreview,
  }: {
    target: { fileName?: string } | null
    onClosePreview?: () => void
  }) => (
    <div data-testid="mock-file-preview-pane">
      <span>{target?.fileName}</span>
      {onClosePreview && (
        <button type="button" aria-label="Close preview" onClick={onClosePreview}>
          Close
        </button>
      )}
    </div>
  ),
}))

import { RightPanel } from './RightPanel'
import { useChatStore } from '@/stores/chatStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import type { GeneratedFile, Message } from '@/types/message'

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

function messageWithFile(conversationId: string, file: GeneratedFile): Message {
  return {
    id: `${conversationId}-${file.id}`,
    conversationId,
    role: 'assistant',
    createdAt: '2026-04-28T00:00:01Z',
    content: { text: 'done', generatedFiles: [file] },
  }
}

function resetStores() {
  useChatStore.setState({
    conversations: [],
    activeConversationId: 'conv-1',
    messages: [],
    busyConversations: new Set(),
    streamStates: {},
    taskStates: {},
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],
  })
  useGeneratedFilePreviewStore.setState({ target: null })
}

beforeEach(() => {
  vi.clearAllMocks()
  resetStores()
})

describe('RightPanel preview workspace', () => {
  it('renders the default narrow panel without empty preview', () => {
    render(<RightPanel conversationId="conv-1" />)

    expect(screen.getByTestId('right-panel')).toHaveClass('w-[260px]')
    expect(screen.queryByText('选择一个产物进行预览')).not.toBeInTheDocument()
  })

  it('hides the skills and MCP section in task monitor', () => {
    render(<RightPanel conversationId="conv-1" />)

    expect(screen.queryByText('技能与 MCP')).not.toBeInTheDocument()
    expect(screen.queryByText('暂无调用')).not.toBeInTheDocument()
  })

  it('renders preview mode when the target belongs to the conversation', () => {
    useGeneratedFilePreviewStore.getState().openPreview({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })

    render(<RightPanel conversationId="conv-1" />)

    expect(screen.getByTestId('right-panel')).toHaveClass('w-[600px]')
    expect(screen.getByText('summary.md')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close preview' })).toBeInTheDocument()
  })

  it('filters the artifact list by conversation', () => {
    useChatStore.setState({
      messages: [
        messageWithFile('conv-1', generatedFile({ id: 'gf-1', fileName: 'summary.md' })),
        messageWithFile('conv-2', generatedFile({ id: 'gf-2', fileName: 'other.md' })),
      ],
    })

    render(<RightPanel conversationId="conv-1" />)

    expect(screen.getByText('summary.md')).toBeInTheDocument()
    expect(screen.queryByText('other.md')).not.toBeInTheDocument()
  })

  it('switches the preview target when clicking a previewable artifact', () => {
    useChatStore.setState({
      messages: [messageWithFile('conv-1', generatedFile({ id: 'gf-1', fileName: 'summary.md' }))],
    })

    render(<RightPanel conversationId="conv-1" />)

    fireEvent.click(screen.getByRole('button', { name: '预览 summary.md' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
  })

  it('previews image artifacts when legacy actions omit preview', () => {
    const onOpenExternal = vi.fn()
    useChatStore.setState({
      messages: [
        messageWithFile('conv-1', generatedFile({
          id: 'gf-legacy-chart',
          fileName: 'mock-status-chart.png',
          fileType: 'png',
          actions: [
            { type: 'open', label: 'Open', enabled: true },
            { type: 'reveal', label: 'Open Folder', enabled: true },
          ],
        })),
      ],
    })

    render(<RightPanel conversationId="conv-1" onOpenExternal={onOpenExternal} />)

    fireEvent.click(screen.getByRole('button', { name: '预览 mock-status-chart.png' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-legacy-chart',
      conversationId: 'conv-1',
      fileName: 'mock-status-chart.png',
      fileType: 'png',
    })
    expect(onOpenExternal).not.toHaveBeenCalled()
  })

  it('switches the preview target when clicking an image artifact', () => {
    const onOpenExternal = vi.fn()
    useChatStore.setState({
      messages: [
        messageWithFile('conv-1', generatedFile({
          id: 'gf-chart',
          fileName: 'mock-status-chart.png',
          fileType: 'png',
        })),
      ],
    })

    render(<RightPanel conversationId="conv-1" onOpenExternal={onOpenExternal} />)

    fireEvent.click(screen.getByRole('button', { name: '预览 mock-status-chart.png' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-chart',
      conversationId: 'conv-1',
      fileName: 'mock-status-chart.png',
      fileType: 'png',
    })
    expect(onOpenExternal).not.toHaveBeenCalled()
  })

  it('keeps non-previewable artifacts disabled when no default-app opener is available', () => {
    useChatStore.setState({
      messages: [
        messageWithFile('conv-1', generatedFile({ id: 'gf-1', fileName: 'summary.md' })),
        messageWithFile('conv-1', generatedFile({
          id: 'gf-2',
          fileName: 'table.xlsx',
          fileType: 'excel',
        })),
      ],
    })

    render(<RightPanel conversationId="conv-1" />)

    expect(screen.getByText('table.xlsx')).toBeInTheDocument()
    const tableButton = screen.getByRole('button', { name: '打开 table.xlsx' })
    expect(tableButton).toBeDisabled()

    fireEvent.click(tableButton)

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('opens non-previewable artifacts with the default app instead of disabling them', () => {
    const onOpenExternal = vi.fn()
    useChatStore.setState({
      messages: [
        messageWithFile('conv-1', generatedFile({
          id: 'gf-2',
          fileName: 'table.xlsx',
          fileType: 'excel',
        })),
      ],
    })

    render(<RightPanel conversationId="conv-1" onOpenExternal={onOpenExternal} />)

    const tableButton = screen.getByRole('button', { name: '打开 table.xlsx' })
    expect(tableButton).toBeEnabled()

    fireEvent.click(tableButton)

    expect(onOpenExternal).toHaveBeenCalledWith({
      fileId: 'gf-2',
      conversationId: 'conv-1',
      fileName: 'table.xlsx',
      fileType: 'excel',
    })
    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('previews previewable artifacts even when preview action is disabled', () => {
    const onOpenExternal = vi.fn()
    useChatStore.setState({
      messages: [
        messageWithFile('conv-1', generatedFile({
          id: 'gf-3',
          fileName: 'locked.md',
          fileType: 'markdown',
          actions: [
            { type: 'preview', label: 'Preview', enabled: false },
            { type: 'open', label: 'Open', enabled: true },
          ],
        })),
      ],
    })

    render(<RightPanel conversationId="conv-1" onOpenExternal={onOpenExternal} />)

    const lockedButton = screen.getByRole('button', { name: '预览 locked.md' })
    expect(lockedButton).toBeEnabled()

    fireEvent.click(lockedButton)

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-3',
      conversationId: 'conv-1',
      fileName: 'locked.md',
      fileType: 'markdown',
    })
    expect(onOpenExternal).not.toHaveBeenCalled()
  })

  it('previews json and csv artifacts even when preview action is disabled', () => {
    const onOpenExternal = vi.fn()
    useChatStore.setState({
      messages: [
        messageWithFile('conv-1', generatedFile({
          id: 'gf-json',
          fileName: 'fallback.json',
          fileType: 'json',
          actions: [{ type: 'preview', label: 'Preview', enabled: false }],
        })),
        messageWithFile('conv-1', generatedFile({
          id: 'gf-csv',
          fileName: 'matrix.csv',
          fileType: 'csv',
          actions: [{ type: 'preview', label: 'Preview', enabled: false }],
        })),
      ],
    })

    render(<RightPanel conversationId="conv-1" onOpenExternal={onOpenExternal} />)

    const jsonButton = screen.getByRole('button', { name: '预览 fallback.json' })
    const csvButton = screen.getByRole('button', { name: '预览 matrix.csv' })
    expect(jsonButton).toBeEnabled()
    expect(csvButton).toBeEnabled()

    fireEvent.click(jsonButton)
    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-json',
      conversationId: 'conv-1',
      fileName: 'fallback.json',
      fileType: 'json',
    })

    expect(screen.getByRole('button', { name: '预览 matrix.csv' })).toBeEnabled()
    expect(onOpenExternal).not.toHaveBeenCalled()
  })

  it('keeps preview-disabled markdown artifacts previewable by type', () => {
    useChatStore.setState({
      messages: [
        messageWithFile('conv-1', generatedFile({ id: 'gf-1', fileName: 'summary.md' })),
        messageWithFile('conv-1', generatedFile({
          id: 'gf-3',
          fileName: 'locked.md',
          fileType: 'markdown',
          actions: [{ type: 'preview', label: 'Preview', enabled: false }],
        })),
      ],
    })

    render(<RightPanel conversationId="conv-1" />)

    expect(screen.getByText('locked.md')).toBeInTheDocument()
    const lockedButton = screen.getByRole('button', { name: '预览 locked.md' })
    expect(lockedButton).toBeEnabled()

    fireEvent.click(lockedButton)

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-3',
      conversationId: 'conv-1',
      fileName: 'locked.md',
      fileType: 'markdown',
    })

    fireEvent.click(screen.getByRole('button', { name: '预览 summary.md' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
  })

  it('closes preview mode by clearing the target', () => {
    useGeneratedFilePreviewStore.getState().openPreview({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })

    render(<RightPanel conversationId="conv-1" />)

    fireEvent.click(screen.getByRole('button', { name: 'Close preview' }))

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })
})
