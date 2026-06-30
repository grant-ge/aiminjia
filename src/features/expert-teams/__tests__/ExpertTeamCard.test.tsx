// code/src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ExpertTeamCard } from '../ExpertTeamCard'
import { EXPERT_TEAMS } from '../teams'

describe('ExpertTeamCard', () => {
  it('renders compact directory card matching the employee catalog density', () => {
    const team = EXPERT_TEAMS[0]
    const { container } = render(<ExpertTeamCard team={team} onStart={() => {}} />)
    const card = screen.getByRole('button', { name: new RegExp(team.name) })

    expect(card).toHaveClass('h-[154px]', 'p-3', 'border-[rgba(var(--border-rgb),0.50)]', 'shadow-[0_1px_3px_rgba(0,0,0,0.035)]')
    expect(screen.getByText(team.name)).toBeInTheDocument()
    expect(screen.getByText(team.tagline)).toBeInTheDocument()
    expect(screen.getByText(`${team.experts.length} 位专家 / 多角色轮询`)).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-expert-team-avatar-stack]')).toBeInTheDocument()
    expect(container.querySelectorAll('img[src^="/expert-avatars/"]')).toHaveLength(3)
    expect(screen.queryByText(team.emoji)).toBeNull()
  })

  it('keeps expert roster out of the card surface', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'strategy')!
    render(<ExpertTeamCard team={team} onStart={() => {}} />)
    expect(screen.queryByTestId('expert-team-roster')).toBeNull()
    for (const expert of team.experts) {
      expect(screen.queryByText(expert.name)).toBeNull()
    }
  })

  it('renders image avatars without leaking emoji fallbacks in the card surface', () => {
    for (const team of EXPERT_TEAMS.filter((t) => t.experts.length > 0)) {
      const { container, unmount } = render(<ExpertTeamCard team={team} onStart={() => {}} />)
      expect(container.querySelector('[data-aijia-expert-team-avatar-stack]')).toBeInTheDocument()
      expect(container.querySelectorAll('img[src^="/expert-avatars/"]')).toHaveLength(Math.min(team.experts.length, 3))
      for (const expert of team.experts) {
        expect(container).not.toHaveTextContent(expert.emoji)
      }
      unmount()
    }
  })

  it('falls back to dynamic-roster hint when experts is empty', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'roundtable')!
    render(<ExpertTeamCard team={team} onStart={() => {}} />)
    expect(screen.queryByTestId('expert-team-roster')).toBeNull()
    expect(screen.getByText(/主持人按议题召集/)).toBeInTheDocument()
    const stack = document.querySelector('[data-aijia-expert-team-avatar-stack]')
    expect(stack).not.toHaveTextContent('?')
    expect(stack?.querySelectorAll('.border-dashed')).toHaveLength(0)
    expect(stack?.querySelectorAll('img[src^="/expert-avatars/roundtable/"]')).toHaveLength(3)
    expect(stack?.querySelectorAll('.lucide-circle-question-mark')).toHaveLength(3)
  })

  it('shows example chips', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'strategy')!
    render(<ExpertTeamCard team={team} onStart={() => {}} />)
    for (const example of team.examples.slice(0, 3)) {
      expect(screen.getByText(example)).toBeInTheDocument()
    }
    const firstChip = screen.getByText(team.examples[0])
    expect(firstChip).toHaveClass('text-2xs', 'rounded-[2px]')
    expect(screen.queryByText('查看详情')).toBeNull()
  })

  it('calls onStart with team.id when clicked', () => {
    const team = EXPERT_TEAMS[0]
    const onStart = vi.fn()
    render(<ExpertTeamCard team={team} onStart={onStart} />)
    fireEvent.click(screen.getByRole('button', { name: new RegExp(team.name) }))
    expect(onStart).toHaveBeenCalledWith(team.id)
  })
})
