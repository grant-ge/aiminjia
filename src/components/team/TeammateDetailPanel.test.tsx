import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { TeammateDetailPanel } from './TeammateDetailPanel'

vi.mock('@/hooks/useTeamOverview', () => ({
  useTeammateTranscript: () => ({ entries: [], loading: false }),
}))

describe('TeammateDetailPanel', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
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
})
