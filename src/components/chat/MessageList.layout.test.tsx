import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/hooks/useTurnRenderModel', () => ({
  useTurnRenderModel: () => [
    {
      peerBanners: [],
      userMessage: null,
      teamMarker: { kind: 'create', toolCallId: 'tool-team' },
      toolGroup: null,
      aiSegments: [],
      generatedFiles: [],
      suggestions: [],
    },
  ],
}))

vi.mock('@/hooks/useTeamOverview', () => ({
  useTeamOverview: () => ({
    overview: {
      conversationId: 'conv-layout',
      teams: [{
        teamId: 'team-1',
        teamName: '市场营销策划团',
        createdAt: '2026-05-19T00:00:00Z',
        deletedAt: null,
        members: [
          { agentId: 'a1', agentName: 'brand-lead', spawnedAt: '2026-05-19T00:00:01Z', isAsync: false, hasTranscript: true },
        ],
        events: [],
      }],
    },
  }),
}))

vi.mock('@/stores/teamStore', () => ({
  useConversationTeamState: () => ({ userClosedDrawer: false }),
  useTeamStore: vi.fn((selector: (state: { openDrawer: () => void }) => unknown) => selector({ openDrawer: vi.fn() })),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn((selector: (state: { isStreaming: boolean; activeConversationId: string | null; busyConversations: Set<string>; streamStates: Record<string, { streamingContent?: string; isStreaming?: boolean }> }) => unknown) => selector({
    isStreaming: false,
    activeConversationId: 'conv-layout',
    busyConversations: new Set(),
    streamStates: {},
  })),
}))

import { MessageList } from './MessageList'

describe('MessageList layout', () => {
  it('does not add horizontal padding inside the shared chat width container', () => {
    const { container } = render(<MessageList />)
    const root = container.firstElementChild

    expect(root).toHaveClass('py-3')
    expect(root).not.toHaveClass('px-6')
  })

  it('passes the active expert team to the inline team progress block for avatar lookup', () => {
    const { container } = render(<MessageList expertTeamId="marketing" />)

    expect(container.querySelector('img[src="/expert-avatars/marketing/品牌负责人.svg"]')).toBeInTheDocument()
  })
})
