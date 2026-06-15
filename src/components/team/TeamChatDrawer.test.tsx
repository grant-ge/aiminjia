import '@testing-library/jest-dom'
import { act, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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
  let originalScrollIntoView: typeof window.HTMLElement.prototype.scrollIntoView | undefined

  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    useTeamStore.getState().reset()
    originalScrollIntoView = window.HTMLElement.prototype.scrollIntoView
  })

  afterEach(() => {
    vi.restoreAllMocks()
    if (originalScrollIntoView) {
      window.HTMLElement.prototype.scrollIntoView = originalScrollIntoView
    } else {
      delete (window.HTMLElement.prototype as Partial<HTMLElement>).scrollIntoView
    }
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

  it('bounds the process timeline to the drawer and makes it vertically scrollable', () => {
    useTeamStore.getState().openDrawer('conv-1')
    const team = getExpertTeam('operations', 'zh-CN')!

    render(
      <TeamVisualProvider value={team}>
        <TeamChatDrawer conversationId="conv-1" overview={overview} />
      </TeamVisualProvider>,
    )

    const panel = screen.getByTestId('team-split-panel')
    const scrollRegion = screen.getByTestId('team-process-scroll-region')

    expect(panel).toHaveClass('h-full', 'min-h-0', 'overflow-hidden')
    expect(scrollRegion).toHaveClass('min-h-0', 'flex-1', 'overflow-y-auto')
  })

  it('renders the split panel as a neutral block container', () => {
    useTeamStore.getState().openDrawer('conv-1')

    render(
      <TeamVisualProvider value={getExpertTeam('operations', 'zh-CN')!}>
        <TeamChatDrawer conversationId="conv-1" overview={overview} />
      </TeamVisualProvider>,
    )

    expect(screen.getByTestId('team-split-panel').tagName).toBe('DIV')
  })

  it('keeps the process panel structure flat and readable', () => {
    useTeamStore.getState().openDrawer('conv-1')

    render(
      <TeamVisualProvider value={getExpertTeam('operations', 'zh-CN')!}>
        <TeamChatDrawer conversationId="conv-1" overview={overview} />
      </TeamVisualProvider>,
    )

    const panel = screen.getByTestId('team-split-panel')
    const header = screen.getByTestId('team-process-header')
    const scrollRegion = screen.getByTestId('team-process-scroll-region')
    const content = screen.getByTestId('team-process-content')

    expect(panel.children[0]).toBe(header)
    expect(panel.children[1]).toBe(scrollRegion)
    expect(header).toHaveClass('h-12')
    expect(header).not.toHaveClass('py-3')
    expect(content.parentElement).toBe(scrollRegion)
    expect(content).toHaveClass('flex', 'flex-col')
  })

  it('renders the session events without an extra width wrapper', () => {
    useTeamStore.getState().openDrawer('conv-1')

    render(
      <TeamVisualProvider value={getExpertTeam('operations', 'zh-CN')!}>
        <TeamChatDrawer conversationId="conv-1" overview={overview} />
      </TeamVisualProvider>,
    )

    const events = screen.getByTestId('team-chat-events')
    const section = events.closest('section')

    expect(section).not.toBeNull()
    expect(events.parentElement).toBe(section)
    expect(events).toHaveClass('w-full')
  })

  it('focuses a team by scrolling only the drawer body and keeps the drawer header mounted', async () => {
    const rafCallbacks: FrameRequestCallback[] = []
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      rafCallbacks.push(callback)
      return rafCallbacks.length
    })
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined)
    const scrollIntoView = vi.fn()
    Object.defineProperty(window.HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: scrollIntoView,
    })

    useTeamStore.getState().openDrawer('conv-1', 'expert-team-operations')

    render(
      <TeamVisualProvider value={getExpertTeam('operations', 'zh-CN')!}>
        <TeamChatDrawer conversationId="conv-1" overview={overview} />
      </TeamVisualProvider>,
    )

    const panel = screen.getByTestId('team-split-panel')
    const header = screen.getByTestId('team-process-header')
    const scrollRegion = screen.getByTestId('team-process-scroll-region')
    const target = panel.querySelector<HTMLElement>('[data-team-id="expert-team-operations"]')

    expect(target).not.toBeNull()
    Object.defineProperty(target!, 'offsetTop', { configurable: true, value: 240 })

    await act(async () => {
      for (const callback of rafCallbacks) callback(0)
    })

    expect(panel.children[0]).toBe(header)
    expect(scrollRegion.scrollTop).toBe(240)
    expect(scrollIntoView).not.toHaveBeenCalled()
  })
})
