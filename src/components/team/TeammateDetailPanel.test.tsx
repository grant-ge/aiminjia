import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { TeammateDetailPanel } from './TeammateDetailPanel'

const transcriptState = vi.hoisted(() => ({
  entries: [] as unknown[],
  loading: false,
}))

vi.mock('@/hooks/useTeamOverview', () => ({
  useTeammateTranscript: () => ({
    entries: transcriptState.entries,
    loading: transcriptState.loading,
  }),
}))

describe('TeammateDetailPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    transcriptState.entries = []
    transcriptState.loading = false
  })

  it('localizes teammate detail chrome in English', async () => {
    await i18n.changeLanguage('en-US')
    render(
      <TeammateDetailPanel
        conversationId="conv-1"
        agentId="ceo-agent"
        agentName="ceo"
        onBack={vi.fn()}
      />,
    )

    expect(screen.getByRole('button', { name: '← Back' })).toBeInTheDocument()
    expect(screen.getByText('Full internal process')).toBeInTheDocument()
    expect(screen.getByText('No visible records for this member')).toBeInTheDocument()
    expect(screen.queryByText('完整内部过程')).not.toBeInTheDocument()
  })

  it('uses the compact fixed drawer header height', () => {
    render(
      <TeammateDetailPanel
        conversationId="conv-1"
        agentId="pro-agent"
        agentName="pro"
        onBack={vi.fn()}
      />,
    )

    const header = screen.getByTestId('teammate-detail-header')

    expect(header).toHaveClass('h-12')
    expect(header).not.toHaveClass('py-3')
  })

  it('renders teammate records as a timeline', () => {
    transcriptState.entries = [
      {
        role: 'user',
        from: 'team-lead',
        content: '请先整理你的观点。',
      },
      {
        role: 'assistant',
        content: '我先总结关键分歧。',
        tool_calls: [
          {
            id: 'tool-1',
            name: 'SendMessage',
            arguments: {
              to: 'brand-lead',
              message: '这里是我的观点。',
            },
          },
        ],
      },
      {
        role: 'tool',
        tool_call_id: 'tool-1',
        tool_name: 'SendMessage',
        content: 'ok',
      },
    ]

    render(
      <TeammateDetailPanel
        conversationId="conv-1"
        agentId="growth-agent"
        agentName="growth-hacker"
        onBack={vi.fn()}
      />,
    )

    const timeline = screen.getByTestId('teammate-detail-timeline')
    expect(timeline).toBeInTheDocument()
    expect(screen.getAllByTestId('teammate-detail-timeline-item')).toHaveLength(2)
    expect(screen.getByText('收到消息')).toBeInTheDocument()
    expect(screen.getByText('思考与行动')).toBeInTheDocument()
    expect(screen.getByText('接收到 Lead')).toBeInTheDocument()
    expect(screen.getByText('发送给 brand-lead')).toBeInTheDocument()
  })

  it('received message cards fill the available width', () => {
    transcriptState.entries = [
      {
        role: 'user',
        from: 'team-lead',
        content: '这是一条很长的角色设定消息。',
      },
    ]

    render(
      <TeammateDetailPanel
        conversationId="conv-1"
        agentId="channel-agent"
        agentName="channel-manager"
        onBack={vi.fn()}
      />,
    )

    const card = screen.getByText('接收到 Lead').closest('[data-teammate-message-card]')
    expect(card).toHaveClass('w-full')
    expect(card).not.toHaveClass('w-fit')
  })
})
