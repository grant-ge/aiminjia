import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

const openGeneratedFileMock = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))

vi.mock('@/lib/tauri', () => ({
  openGeneratedFile: openGeneratedFileMock,
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
          appName: 'Open',
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
  it('opens generated files using the file id and owning conversation id', () => {
    render(<MessageList />)

    fireEvent.click(screen.getByRole('button', { name: 'Open open' }))

    expect(openGeneratedFileMock).toHaveBeenCalledWith(
      'gf-report-html-001',
      '6dcab8d4-cac3-476e-8c1b-930e14f12fe7',
    )
  })
})
