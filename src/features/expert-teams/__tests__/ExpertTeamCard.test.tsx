// code/src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ExpertTeamCard } from '../ExpertTeamCard'
import { EXPERT_TEAMS } from '../teams'

describe('ExpertTeamCard', () => {
  it('renders team emoji, name and tagline', () => {
    const team = EXPERT_TEAMS[0]
    render(<ExpertTeamCard team={team} onStart={() => {}} />)
    expect(screen.getByText(team.name)).toBeInTheDocument()
    expect(screen.getByText(team.tagline)).toBeInTheDocument()
    expect(screen.getByText(team.emoji)).toBeInTheDocument()
  })

  it('shows expert count for staffed teams', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'strategy')!
    render(<ExpertTeamCard team={team} onStart={() => {}} />)
    expect(screen.getByText(/4 位专家/)).toBeInTheDocument()
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
