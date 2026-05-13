import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'

const openGeneratedFileMock = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
const openPreviewMock = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri', () => ({
  openGeneratedFile: openGeneratedFileMock,
  revealFileInFolder: vi.fn().mockResolvedValue(undefined),
  getTeamOverview: vi.fn().mockResolvedValue({ conversationId: '', teams: [] }),
  getTeammateTranscript: vi.fn().mockResolvedValue([]),
  onMessageUpdated: vi.fn().mockResolvedValue(() => {}),
  onToolCompleted: vi.fn().mockResolvedValue(() => {}),
}))

vi.mock('@/stores/generatedFilePreviewStore', () => ({
  useGeneratedFilePreviewStore: vi.fn((selector: (state: { openPreview: typeof openPreviewMock; clearIfConversationChanged: () => void }) => unknown) => selector({
    openPreview: openPreviewMock,
    clearIfConversationChanged: vi.fn(),
  })),
}))

vi.mock('@/hooks/useTurnRenderModel', () => ({
  useTurnRenderModel: () => [
    {
      aiSegments: [],
      generatedFiles: [
        {
          id: 'gf-report-html-001',
          conversationId: '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
          title: 'mock-coverage-report.html',
          sub: '82 B · 报告',
          appName: '打开',
        },
        {
          id: 'gf-report-md-001',
          conversationId: '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
          title: 'mock-markdown-brief.md',
          fileName: 'mock-markdown-brief.md',
          fileType: 'markdown',
          sub: '1 KB · 报告',
          appName: '预览',
          primaryAction: 'preview',
          canPreview: true,
        },
        {
          id: 'gf-chart-png-001',
          conversationId: '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
          title: 'mock-status-chart.png',
          fileName: 'mock-status-chart.png',
          fileType: 'png',
          sub: '68 B · 图表',
          appName: '预览',
          primaryAction: 'preview',
          canPreview: true,
        },
      ],
      suggestions: [],
    },
  ],
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn((selector: (state: { isStreaming: boolean; activeConversationId: string | null; streamStates: Record<string, { streamingContent?: string }> }) => unknown) => selector({
    isStreaming: false,
    activeConversationId: '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
    streamStates: {},
  })),
}))

import { MessageList } from './MessageList'

describe('MessageList generated files', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('opens generated files using the file id and owning conversation id', () => {
    render(<MessageList />)

    fireEvent.click(screen.getByRole('button', { name: '打开 mock-coverage-report.html' }))

    expect(openGeneratedFileMock).toHaveBeenCalledWith(
      'gf-report-html-001',
      '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
    )
  })

  it('previews markdown generated files from the primary action', () => {
    render(<MessageList />)

    fireEvent.click(screen.getByRole('button', { name: '预览 mock-markdown-brief.md' }))

    expect(openPreviewMock).toHaveBeenCalledWith({
      fileId: 'gf-report-md-001',
      conversationId: '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
      fileName: 'mock-markdown-brief.md',
      fileType: 'markdown',
    })
  })

  it('previews image generated files from the primary action without opening externally', () => {
    render(<MessageList />)

    fireEvent.click(screen.getByRole('button', { name: '预览 mock-status-chart.png' }))

    expect(openPreviewMock).toHaveBeenCalledWith({
      fileId: 'gf-chart-png-001',
      conversationId: '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
      fileName: 'mock-status-chart.png',
      fileType: 'png',
    })
    expect(openGeneratedFileMock).not.toHaveBeenCalledWith(
      'gf-chart-png-001',
      '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
    )
  })
})
