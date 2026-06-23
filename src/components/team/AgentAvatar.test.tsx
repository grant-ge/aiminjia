import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { getExpertTeam } from '@/features/expert-teams/teams'
import { TeamVisualProvider } from './TeamVisualContext'
import { AgentAvatar } from './AgentAvatar'

describe('AgentAvatar', () => {
  it('uses a neutral container for committed expert image avatars', () => {
    const marketingTeam = getExpertTeam('marketing', 'zh-CN')

    render(
      <TeamVisualProvider value={marketingTeam ?? null}>
        <AgentAvatar name="brand-lead" />
      </TeamVisualProvider>,
    )

    const avatar = screen.getByLabelText('品牌负责人')
    expect(avatar.querySelector('img')?.getAttribute('src')).toBe(
      '/expert-avatars/marketing/品牌负责人.svg',
    )
    expect(avatar).toHaveClass('bg-muted')
    expect(avatar).toHaveClass('rounded-full')
    expect(avatar.querySelector('img')).toHaveClass('rounded-full')
    expect(avatar.className).not.toMatch(/\bbg-(blue|emerald|rose|amber|violet|cyan)-500\b/)
  })

  it('uses the committed lead avatar instead of the team-lead initials fallback', () => {
    render(<AgentAvatar name="team-lead" />)

    const avatar = screen.getByLabelText('Lead')
    expect(avatar.querySelector('img')?.getAttribute('src')).toBe('/expert-avatars/lead.svg')
    expect(avatar).toHaveClass('bg-muted')
    expect(avatar).toHaveClass('rounded-full')
    expect(avatar).not.toHaveTextContent('te')
  })
})
