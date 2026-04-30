import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { Message, MessageContent } from '@/types/message'

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

  it('does not render legacy exec summary blocks inside AiBubble', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-exec-summary',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            execSummary: {
              title: '全集渲染完成度',
              boxes: [
                {
                  label: 'Markdown',
                  value: 'Ready',
                  subtitle: '纯表格 / 超宽 / 混排',
                  variant: 'good',
                },
              ],
            },
          } as unknown as MessageContent,
        }}
      />,
    )

    expect(screen.queryByText('全集渲染完成度')).not.toBeInTheDocument()
    expect(screen.queryByText('Markdown')).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })

  it('drops legacy incomplete command code blocks from historical messages', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-incomplete-tool-round',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            codeBlocks: [
              {
                id: 'legacy-command-block',
                language: 'bash',
                purpose: '命令型代码块',
                code: 'tail -n 20 messages.jsonl',
                status: 'running',
              },
            ],
            codeResults: [
              {
                id: 'legacy-command-result',
                codeBlockId: 'legacy-command-block',
                output: 'tool round intentionally left partially complete',
                isError: true,
              },
            ],
          },
        }}
      />,
    )

    expect(screen.queryByText('tail -n 20 messages.jsonl')).not.toBeInTheDocument()
    expect(screen.queryByText('tool round intentionally left partially complete')).not.toBeInTheDocument()
    expect(screen.queryByText('Running...')).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })


  it('does not render legacy structured code blocks from historical messages', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-legacy-code-block',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            codeBlocks: [
              {
                id: 'legacy-code-block',
                language: 'ts',
                purpose: '全集渲染代码块',
                code: "const sampleSet = ['markdown', 'tables', 'generatedFiles', 'toolGroup', 'subagent']",
                status: 'success',
              },
            ],
            codeResults: [
              {
                id: 'legacy-code-result',
                codeBlockId: 'legacy-code-block',
                output: 'sampleSet loaded',
                isError: false,
              },
            ],
          },
        }}
      />,
    )

    expect(screen.queryByText(/sampleSet/)).not.toBeInTheDocument()
    expect(screen.queryByText('全集渲染代码块')).not.toBeInTheDocument()
    expect(screen.queryByText('sampleSet loaded')).not.toBeInTheDocument()
    expect(screen.queryByText('Done')).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })

  it('ignores legacy metrics content without rendering metric cards', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-metrics',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            metrics: [
              {
                id: 'metric-1',
                label: '消息总数',
                value: '62',
                subtitle: '追加全集渲染后',
                state: 'good',
              },
            ],
          } as unknown as MessageContent,
        }}
      />,
    )

    expect(screen.queryByText('消息总数')).not.toBeInTheDocument()
    expect(screen.queryByText('62')).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })

  it('does not render legacy confirmation blocks inside AiBubble', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-confirmation',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            confirmations: [
              {
                id: 'confirm-1',
                title: '是否保留这一轮“全集渲染”作为最终验收样本',
                primaryLabel: '保留',
                primaryAction: 'keep',
                secondaryLabel: '继续补',
                secondaryAction: 'continue',
                status: 'pending',
              },
            ],
          } as unknown as MessageContent,
        }}
      />,
    )

    expect(screen.queryByText('是否保留这一轮“全集渲染”作为最终验收样本')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '保留' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '继续补' })).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })

  it('does not render legacy option cards inside AiBubble', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-options',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            options: [
              {
                id: 'render-scope',
                selectedId: 'all',
                columns: 3,
                options: [
                  {
                    id: 'markdown',
                    tag: '轻量',
                    tagColor: 'rgb(100, 116, 139)',
                    title: '只看 markdown',
                    description: '只验证文本渲染。',
                  },
                  {
                    id: 'all',
                    tag: '全集',
                    tagColor: 'rgb(22, 163, 74)',
                    title: '看全部样本',
                    description: '把所有渲染类型一口气看完。',
                  },
                  {
                    id: 'interrupted',
                    tag: '异常',
                    tagColor: 'rgb(220, 38, 38)',
                    title: '只看中断态',
                    description: '聚焦 error 与 running-like 样本。',
                  },
                ],
              },
            ],
          } as unknown as MessageContent,
        }}
      />,
    )

    expect(screen.queryByText('只看 markdown')).not.toBeInTheDocument()
    expect(screen.queryByText('看全部样本')).not.toBeInTheDocument()
    expect(screen.queryByText('只看中断态')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /看全部样本/ })).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })


  it('does not render validation insight or anomaly sample cards inside AiBubble', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-validation-samples',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            insights: [
              {
                id: 'insight-acceptance',
                title: 'Internal validation insight',
                content: 'Internal validation fixture should not appear in production chat.',
              },
              {
                id: 'insight-policy',
                title: 'Internal rendering policy',
                content: 'Internal rendering policy should not appear in production chat.',
              },
            ],
            anomalies: [
              {
                id: 'anomaly-summary-loss',
                priority: 'high',
                title: 'Internal anomaly warning',
                description: 'Internal anomaly fixture should not appear in production chat.',
              },
              {
                id: 'anomaly-running-gap',
                priority: 'medium',
                title: 'Internal running-state warning',
                description: 'Internal running-state fixture should not appear in production chat.',
              },
            ],
          },
        }}
      />,
    )

    expect(screen.queryByText('Internal validation insight')).not.toBeInTheDocument()
    expect(screen.queryByText('Internal rendering policy')).not.toBeInTheDocument()
    expect(screen.queryByText('Internal anomaly warning')).not.toBeInTheDocument()
    expect(screen.queryByText('Internal running-state warning')).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })

  it('ignores legacy root cause blocks without crashing or rendering the red card', () => {
    const { container } = render(
      <AiBubble
        message={{
          id: 'msg-root-cause',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            rootCauses: [
              {
                id: 'root-cause-1',
                title: '为什么要再补最后一轮',
                items: [
                  {
                    count: 1,
                    label: '目录/总结不够',
                    detail: '会稀释或遮盖真实渲染块。',
                    action: '直接汇总原始样本。',
                  },
                ],
              },
            ],
          } as unknown as MessageContent,
        }}
      />,
    )

    expect(screen.queryByText('为什么要再补最后一轮')).not.toBeInTheDocument()
    expect(screen.queryByText('目录/总结不够')).not.toBeInTheDocument()
    expect(container.firstChild).toBeNull()
  })

  it('renders normal text when malformed legacy rootCauses data is present', () => {
    expect(() => {
      render(
        <AiBubble
          message={{
            id: 'msg-root-cause-malformed',
            conversationId: 'conv-1',
            role: 'assistant',
            createdAt: '2026-04-18T00:00:00Z',
            content: {
              text: '正常回复仍然展示',
              rootCauses: [{ id: 'broken-root-cause', title: '坏数据' }],
            } as unknown as MessageContent,
          }}
        />,
      )
    }).not.toThrow()

    expect(screen.getByText('正常回复仍然展示')).toBeInTheDocument()
    expect(screen.queryByText('坏数据')).not.toBeInTheDocument()
  })


  it('does not render generated file cards inside AiBubble', () => {
    render(
      <AiBubble
        message={{
          id: 'msg-generated-file',
          conversationId: 'conv-1',
          role: 'assistant',
          createdAt: '2026-04-18T00:00:00Z',
          content: {
            generatedFiles: [
              {
                id: 'file-1',
                fileName: 'mock-data-matrix.csv',
                filePath: '/tmp/mock-data-matrix.csv',
                fileType: 'csv',
                fileSize: 128,
                category: 'data',
                version: 1,
                isLatest: true,
                createdAt: '2026-04-18T00:00:00Z',
                description: 'matrix export',
                actions: [],
              },
            ],
          },
        }}
      />,
    )

    expect(screen.queryByText('mock-data-matrix.csv')).not.toBeInTheDocument()
  })

  it('does not render the old avatar-offset layout for history messages', () => {
    const { container } = render(<AiBubble message={envelopeMessage} />)

    expect(container.querySelector('.pl-9')).toBeNull()
  })
})
