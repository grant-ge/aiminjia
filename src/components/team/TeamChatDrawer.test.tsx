import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import i18n from '@/i18n'
import { getExpertTeam } from '@/features/expert-teams/teams'
import { useTeamStore } from '@/stores/teamStore'
import type { TeamOverview } from '@/types/team'
import { TeamChatDrawer } from './TeamChatDrawer'
import { TeamVisualProvider } from './TeamVisualContext'

const overview: TeamOverview = {
  conversationId: 'conv-1',
  teams: [{
    teamId: 'expert-team-operations',
    teamName: '经营决策团',
    createdAt: '2026-06-01T10:00:00.000Z',
    deletedAt: null,
    members: [
      { agentId: 'lead', agentName: 'team-lead', spawnedAt: '2026-06-01T10:00:00.000Z', isAsync: false, hasTranscript: false },
      { agentId: 'ceo', agentName: 'ceo', spawnedAt: '2026-06-01T10:00:01.000Z', isAsync: true, hasTranscript: true },
      { agentId: 'cfo', agentName: 'cfo', spawnedAt: '2026-06-01T10:00:01.000Z', isAsync: true, hasTranscript: true },
      { agentId: 'coo', agentName: 'coo', spawnedAt: '2026-06-01T10:00:01.000Z', isAsync: true, hasTranscript: true },
      { agentId: 'analyst', agentName: 'analyst', spawnedAt: '2026-06-01T10:00:01.000Z', isAsync: true, hasTranscript: true },
    ],
    events: [],
  }],
}

describe('TeamChatDrawer', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    useTeamStore.getState().reset()
  })

  it('localizes process drawer chrome and team title in English', async () => {
    await i18n.changeLanguage('en-US')
    useTeamStore.getState().openDrawer('conv-1')
    const team = getExpertTeam('operations', 'en-US')!

    render(
      <TeamVisualProvider value={team}>
        <TeamChatDrawer conversationId="conv-1" overview={overview} />
      </TeamVisualProvider>,
    )

    expect(screen.getByText('Team process')).toBeInTheDocument()
    expect(screen.getByText('1 team session')).toBeInTheDocument()
    expect(screen.getAllByText('4 members').length).toBeGreaterThan(0)
    expect(screen.getByText('Business Decision Team')).toBeInTheDocument()
    expect(screen.getByLabelText('Close team panel')).toBeInTheDocument()
    expect(screen.queryByText('团队过程')).not.toBeInTheDocument()
    expect(screen.queryByText('经营决策团')).not.toBeInTheDocument()
  })
})
