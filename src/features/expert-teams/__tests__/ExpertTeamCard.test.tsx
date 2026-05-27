// code/src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ExpertTeamCard } from '../ExpertTeamCard'
import { EXPERT_TEAMS } from '../teams'

describe('ExpertTeamCard', () => {
  it('renders team logo badge, name and tagline', () => {
    const team = EXPERT_TEAMS[0]
    const { container } = render(<ExpertTeamCard team={team} onStart={() => {}} />)
    expect(screen.getByText(team.name)).toBeInTheDocument()
    expect(screen.getByText(team.tagline)).toBeInTheDocument()
    expect(screen.getByTestId(`expert-team-logo-${team.id}`)).toBeInTheDocument()
    expect(container.querySelector('[data-testid^="expert-team-logo-"] svg')).toBeInTheDocument()
    expect(screen.queryByText(team.emoji)).toBeNull()
  })

  it('shows expert roster avatars for staffed teams', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'strategy')!
    render(<ExpertTeamCard team={team} onStart={() => {}} />)
    // The roster container exists and has one entry per expert.
    const roster = screen.getByTestId('expert-team-roster')
    // Each expert name appears as the label under its avatar.
    for (const expert of team.experts) {
      expect(roster).toHaveTextContent(expert.name)
    }
  })

  it('renders image avatars for every fixed-roster expert instead of emoji fallbacks', () => {
    for (const team of EXPERT_TEAMS.filter((t) => t.experts.length > 0)) {
      const { container, unmount } = render(<ExpertTeamCard team={team} onStart={() => {}} />)
      const roster = screen.getByTestId('expert-team-roster')
      expect(roster.querySelectorAll('img[src^="/expert-avatars/"]')).toHaveLength(team.experts.length)
      for (const expert of team.experts) {
        expect(container.querySelector(`img[title="${expert.name}"]`)).not.toBeInTheDocument()
        expect(roster).not.toHaveTextContent(expert.emoji)
      }
      unmount()
    }
  })

  it('falls back to dynamic-roster hint when experts is empty', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'roundtable')!
    render(<ExpertTeamCard team={team} onStart={() => {}} />)
    expect(screen.queryByTestId('expert-team-roster')).toBeNull()
    expect(screen.getByText(/主持人按议题召集/)).toBeInTheDocument()
  })

  it('shows example chips', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'strategy')!
    render(<ExpertTeamCard team={team} onStart={() => {}} />)
    for (const example of team.examples) {
      expect(screen.getByText(example)).toBeInTheDocument()
    }
  })

  it('calls onStart with team.id when clicked', () => {
    const team = EXPERT_TEAMS[0]
    const onStart = vi.fn()
    render(<ExpertTeamCard team={team} onStart={onStart} />)
    fireEvent.click(screen.getByRole('button', { name: new RegExp(team.name) }))
    expect(onStart).toHaveBeenCalledWith(team.id)
  })
})
