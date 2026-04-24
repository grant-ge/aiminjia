import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const sendUserMessage = vi.fn(async () => undefined)

// Mock stores that HomePage's children touch
vi.mock('@/stores/uiStore', () => ({
  useUiStore: (sel: (s: unknown) => unknown) =>
    sel({ route: { kind: 'home' }, setRoute: vi.fn(), openSettings: vi.fn() }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: (s: unknown) => unknown) =>
    sel({ productName: 'AI 小家', logoUrl: '/app-icon.png', accentColor: '#DBAA22' }),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: (sel: (s: unknown) => unknown) =>
    sel({ messages: [], isStreaming: false, streamingContent: '', activeConversationId: null, streamStates: {}, busyConversations: new Set() }),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage,
    conversations: [],
    activeConversationId: null,
    switchConversation: vi.fn(),
    createNewConversation: vi.fn(),
    sendMessage: vi.fn(),
    isStreaming: false,
  }),
}))

import { useSkillStore } from '@/stores/skillStore'
import { HomePage } from './HomePage'

describe('HomePage', () => {
  beforeEach(() => {
    sendUserMessage.mockClear()
    useSkillStore.setState({
      skills: [
        {
          id: 'writing-plans',
          displayName: '写计划',
          displayNameEn: 'Writing Plans',
          description: 'plan things',
          source: 'builtin',
          hasWorkflow: true,
          icon: 'file-text',
          shortDescription: '写计划',
          shortDescriptionEn: 'Write plans',
          triggerText: '/writing-plans',
          category: 'general',
        },
        {
          id: 'research-brief',
          displayName: '研究摘要',
          displayNameEn: 'Research Brief',
          description: 'research',
          source: 'builtin',
          hasWorkflow: true,
          icon: 'search',
          shortDescription: '做研究',
          shortDescriptionEn: 'Research',
          triggerText: '/research-brief',
          category: 'general',
        },
      ],
    })
  })

  it('renders mascot title, category chips and skill-center pill', () => {
    render(<HomePage />)
    // HomeMascotHero title and HomeTaskComposerCard h1 both render this text
    expect(screen.getAllByText('创建你的下一条任务').length).toBeGreaterThanOrEqual(1)
    expect(screen.getByRole('button', { name: /为你推荐/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /前往技能中心/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /实施计划/ })).toBeInTheDocument()
  })

  it('vertically centers the main home content column', () => {
    const { container } = render(<HomePage />)
    const pageWrapper = container.querySelector('.mx-auto.max-w-\\[1032px\\]')
    expect(pageWrapper?.className).toMatch(/justify-center/)
  })

  it('adds extra top spacing above the category chip row', () => {
    const { container } = render(<HomePage />)
    const categoryRowWrap = container.querySelector('.mt-3.w-full')
    expect(categoryRowWrap?.className).toMatch(/mt-3/)
  })

  it('uses the dedicated home mascot svg', () => {
    const { container } = render(<HomePage />)
    const mascotImg = container.querySelector('[data-testid="home-mascot"] img')
    expect(mascotImg).toHaveAttribute('src', '/home-mascot-fill-13.svg')
  })

  it('switches suggestion list when selecting another expert tab', () => {
    render(<HomePage />)

    fireEvent.click(screen.getByRole('button', { name: /研究专家/ }))

    expect(screen.getByRole('button', { name: /竞品对比/ })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /实施计划/ })).not.toBeInTheDocument()
  })

  it('sends slash-prefixed prompt when selecting a suggested task with a bound skill', () => {
    render(<HomePage />)

    fireEvent.click(screen.getByRole('button', { name: /实施计划/ }))

    expect(sendUserMessage).toHaveBeenCalledWith(
      expect.stringMatching(/^\/writing-plans /),
    )
  })
})
