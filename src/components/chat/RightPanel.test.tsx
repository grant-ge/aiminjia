import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('./FilePreviewPane', () => ({
  FilePreviewPane: ({ target }: { target: { fileName?: string } | null }) => (
    <div data-testid="mock-file-preview-pane">{target?.fileName}</div>
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

  it('renders preview mode when the target belongs to the conversation', () => {
    useGeneratedFilePreviewStore.getState().openPreview({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })

    render(<RightPanel conversationId="conv-1" />)

    expect(screen.getByTestId('right-panel')).toHaveClass('w-[720px]')
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

    fireEvent.click(screen.getByRole('button', { name: 'Preview summary.md' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
  })

  it('keeps non-previewable artifacts visible but disabled', () => {
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
    const tableButton = screen.getByRole('button', { name: 'Preview table.xlsx' })
    expect(tableButton).toBeDisabled()

    fireEvent.click(tableButton)

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Preview summary.md' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
  })

  it('keeps preview-disabled markdown artifacts visible but disabled', () => {
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
    const lockedButton = screen.getByRole('button', { name: 'Preview locked.md' })
    expect(lockedButton).toBeDisabled()

    fireEvent.click(lockedButton)

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Preview summary.md' }))

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
