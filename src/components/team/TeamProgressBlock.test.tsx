import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import type { TeamSession } from '@/types/team'
import { TeamProgressBlock } from './TeamProgressBlock'

function makeSession(overrides: Partial<TeamSession> = {}): TeamSession {
  return {
    teamId: 'expert-team-operations',
    teamName: 'Business Decision Team',
    createdAt: '2026-06-01T10:00:00.000Z',
    deletedAt: null,
    members: [
      { agentId: 'lead', agentName: 'team-lead', spawnedAt: '2026-06-01T10:00:00.000Z', isAsync: false, hasTranscript: false },
      { agentId: 'ceo', agentName: 'ceo', spawnedAt: '2026-06-01T10:00:01.000Z', isAsync: true, hasTranscript: true },
      { agentId: 'cfo', agentName: 'cfo', spawnedAt: '2026-06-01T10:00:01.000Z', isAsync: true, hasTranscript: true },
      { agentId: 'coo', agentName: 'coo', spawnedAt: '2026-06-01T10:00:01.000Z', isAsync: true, hasTranscript: true },
      { agentId: 'analyst', agentName: 'analyst', spawnedAt: '2026-06-01T10:00:01.000Z', isAsync: true, hasTranscript: true },
    ],
    events: [
      { kind: 'send_message', ts: '2026-06-01T10:00:02.000Z', from: 'ceo', to: 'team-lead', text: 'one', isError: false, toolCallId: 't1', variant: 'text' },
      { kind: 'send_message', ts: '2026-06-01T10:00:03.000Z', from: 'cfo', to: 'team-lead', text: 'two', isError: false, toolCallId: 't2', variant: 'text' },
      { kind: 'send_message', ts: '2026-06-01T10:00:04.000Z', from: 'coo', to: 'team-lead', text: 'three', isError: false, toolCallId: 't3', variant: 'text' },
      { kind: 'send_message', ts: '2026-06-01T10:00:05.000Z', from: 'analyst', to: 'team-lead', text: 'four', isError: false, toolCallId: 't4', variant: 'text' },
    ],
    ...overrides,
  }
}

describe('TeamProgressBlock', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
  })

  it('localizes team process status and counts in English', async () => {
    await i18n.changeLanguage('en-US')
    render(<TeamProgressBlock session={makeSession()} onOpen={vi.fn()} />)

    expect(screen.getByText('Live')).toBeInTheDocument()
    expect(screen.getByText(/4 members/)).toBeInTheDocument()
    expect(screen.getByText(/4 messages/)).toBeInTheDocument()
    expect(screen.getByText('View process →')).toBeInTheDocument()
    expect(screen.queryByText(/位成员/)).not.toBeInTheDocument()
    expect(screen.queryByText('进行中')).not.toBeInTheDocument()
  })
})
