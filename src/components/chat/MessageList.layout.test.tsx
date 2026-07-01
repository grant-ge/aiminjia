import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const turnsMock = vi.hoisted(() => ({
  value: [
    {
      peerBanners: [],
      userMessage: null,
      teamMarker: { kind: 'create', toolCallId: 'tool-team' },
      toolGroup: null,
      aiSegments: [],
      generatedFiles: [],
      suggestions: [],
    },
  ] as Array<Record<string, unknown>>,
}))

vi.mock('@/hooks/useTurnRenderModel', () => ({
  useTurnRenderModel: () => turnsMock.value,
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
  beforeEach(() => {
    turnsMock.value = [
      {
        peerBanners: [],
        userMessage: null,
        teamMarker: { kind: 'create', toolCallId: 'tool-team' },
        toolGroup: null,
        aiSegments: [],
        generatedFiles: [],
        suggestions: [],
      },
    ]
  })

  it('does not add horizontal padding inside the shared chat width container', () => {
    const { container } = render(<MessageList />)
    const root = container.firstElementChild

    expect(root).toHaveClass('py-3')
    expect(root).not.toHaveClass('px-6')
  })

  it('uses a roomy gap between top-level message turns', () => {
    const { container } = render(<MessageList />)
    const root = container.firstElementChild

    expect(root).toHaveClass('gap-10')
    expect(root).not.toHaveClass('gap-5')
  })

  it('keeps a larger vertical gap between user and assistant content inside a turn', () => {
    const { container } = render(<MessageList />)
    const root = container.firstElementChild
    const turn = root?.firstElementChild

    expect(turn).toHaveClass('gap-5')
    expect(turn).not.toHaveClass('gap-4')
  })

  it('passes the active expert team to the inline team progress block for avatar lookup', () => {
    const { container } = render(<MessageList expertTeamId="marketing" />)

    expect(container.querySelector('img[src="/expert-avatars/marketing/品牌负责人.svg"]')).toBeInTheDocument()
  })

  it('renders employee dispatch prompts as ordinary user bubble content', () => {
    turnsMock.value = [
      {
        peerBanners: [],
        userMessage: {
          id: 'dispatch-user',
          text: [
            '你现在是「小工」（技术支持）。',
            '负责处理技术支持请求。',
            '',
            '[按需派活]',
            '帮我看一下集成问题',
            '',
            '【本次工作配置】',
            '- 默认技能：tech-support',
            '',
            '请立即开始按职责执行，不要等待用户额外指示。',
          ].join('\n'),
          createdAt: '2026-06-25T00:00:00.000Z',
        },
        teamMarker: null,
        toolGroup: null,
        aiSegments: [],
        generatedFiles: [],
        suggestions: [],
      },
    ]

    render(<MessageList />)

    expect(screen.getByTestId('user-bubble')).toBeInTheDocument()
    expect(screen.getByText(/你现在是「小工」（技术支持）。/)).toBeInTheDocument()
    expect(screen.getByText(/请立即开始按职责执行/)).toBeInTheDocument()
    expect(screen.queryByText(/派活给 小工/)).not.toBeInTheDocument()
  })
})
