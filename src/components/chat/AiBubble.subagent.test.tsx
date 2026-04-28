import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { Message } from '@/types/message'

vi.mock('@/lib/tauri', () => ({
  sendMessage: vi.fn(),
  openGeneratedFile: vi.fn(),
  revealFileInFolder: vi.fn(),
  getSubagentTranscript: vi.fn(),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn(
    (selector: (state: { activeConversationId: string | null }) => unknown) =>
      selector({ activeConversationId: 'conv-1' }),
  ),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: {
    getState: () => ({
      push: vi.fn(),
    }),
  },
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (
      key: string,
      fallbackOrOptions?: string | { defaultValue?: string; count?: number },
    ) => {
      if (typeof fallbackOrOptions === 'string') return fallbackOrOptions
      return fallbackOrOptions?.defaultValue ?? key
    },
  }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}))

vi.mock('@/components/rich-content/SubAgentTranscriptViewer', () => ({
  SubAgentTranscriptViewer: () => <div data-testid="transcript-viewer-stub" />,
}))

import { AiBubble } from './AiBubble'

const envelopeMessage: Message = {
  id: 'msg-1',
  conversationId: 'conv-1',
  role: 'assistant',
  createdAt: '2026-04-18T00:00:00Z',
  content: {
    subagentEnvelope: {
      schemaVersion: 1,
      output: 'Subagent finished the task successfully.',
      iterationsUsed: 5,
      generatedFiles: ['analysis.xlsx'],
      transcriptRef: 'subagent://child-run-99',
    },
  },
}

describe('AiBubble — subagentEnvelope integration', () => {
  it('renders SubAgentResultCard when message has subagentEnvelope', () => {
    render(<AiBubble message={envelopeMessage} />)

    expect(
      screen.getByText('Subagent finished the task successfully.'),
    ).toBeInTheDocument()
    expect(screen.getByText('analysis.xlsx')).toBeInTheDocument()
    expect(screen.getByText(/5/)).toBeInTheDocument()
    expect(screen.getByTestId('transcript-viewer-stub')).toBeInTheDocument()
  })

  it('does not render an empty bubble for an envelope-only message', () => {
    const { container } = render(<AiBubble message={envelopeMessage} />)
    expect(container.firstChild).not.toBeNull()
  })

  it('renders both text and envelope when both are present', () => {
    render(
      <AiBubble
        message={{
          ...envelopeMessage,
          content: {
            text: 'Some preamble text.',
            subagentEnvelope: envelopeMessage.content.subagentEnvelope,
          },
        }}
      />,
    )

    expect(screen.getByText('Some preamble text.')).toBeInTheDocument()
    expect(
      screen.getByText('Subagent finished the task successfully.'),
    ).toBeInTheDocument()
  })


  it('ignores legacy progress content without rendering the progress block', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-progress',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            progress: {
              title: '渲染样本全集构建完成',
              currentStep: 3,
              steps: [
                { label: '收集现有样本', status: 'done' },
                { label: '汇总到同一回合', status: 'done' },
                { label: '保留原始渲染形态', status: 'done' },
              ],
            },
          },
        }}
      />,
    )

    expect(screen.queryByText('渲染样本全集构建完成')).not.toBeInTheDocument()
    expect(screen.queryByText('收集现有样本')).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })

  it('does not render the old avatar-offset layout for history messages', () => {
    const { container } = render(<AiBubble message={envelopeMessage} />)

    expect(container.querySelector('.pl-9')).toBeNull()
  })
})
