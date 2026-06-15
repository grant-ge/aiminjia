import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { useSettingsStore } from '@/stores/settingsStore'
import { DEFAULT_SETTINGS } from '@/types/settings'
import { EXPERT_TEAMS } from '@/features/expert-teams/teams'
import { ExpertTeamWelcome } from './ExpertTeamWelcome'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: vi.fn().mockResolvedValue(undefined) }),
}))

describe('ExpertTeamWelcome', () => {
  it('renders the team avatar stack and expert avatars', () => {
    useSettingsStore.setState({ ...DEFAULT_SETTINGS, chatWidthMode: 'full' })
    const team = EXPERT_TEAMS.find((t) => t.id === 'marketing')!
    const { container } = render(<ExpertTeamWelcome team={team} />)

    expect(container.querySelector('[data-aijia-expert-team-avatar-stack]')).toBeInTheDocument()
    expect(container.querySelector('img[src="/expert-avatars/marketing/品牌负责人.svg"]')).toBeInTheDocument()
    expect(container).toHaveTextContent('品牌负责人')
    expect(container).toHaveTextContent('多角色轮询')
  })

  it('uses centered width when chat width mode is centered', () => {
    useSettingsStore.setState({ ...DEFAULT_SETTINGS, chatWidthMode: 'centered' })
    const team = EXPERT_TEAMS.find((t) => t.id === 'marketing')!
    render(<ExpertTeamWelcome team={team} />)

    const shell = screen.getByTestId('expert-team-welcome-shell')
    expect(shell).toHaveClass('mx-auto')
    expect(shell).toHaveClass('max-w-[680px]')
  })
})
