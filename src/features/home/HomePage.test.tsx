import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const sendUserMessage = vi.fn(async () => undefined)

// Mock stores that HomePage's children touch
const setRoute = vi.fn()
const consumePrefillText = vi.fn(() => null)
const uiState = { route: { kind: 'home' }, setRoute, openSettings: vi.fn(), consumePrefillText, consumePendingSkill: vi.fn(() => null) }

vi.mock('@/stores/uiStore', () => ({
  useUiStore: Object.assign(
    (sel: (s: unknown) => unknown) => sel(uiState),
    { getState: () => uiState },
  ),
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

import { HomePage } from './HomePage'

describe('HomePage', () => {
  beforeEach(() => {
    sendUserMessage.mockClear()
    setRoute.mockClear()
    consumePrefillText.mockClear()
  })

  it('renders mascot title and composer without secondary CTA or suggestions', () => {
    render(<HomePage />)
    expect(screen.getAllByText('千头万绪在前，先理一端').length).toBeGreaterThanOrEqual(1)
    expect(screen.queryByRole('button', { name: /前往技能中心/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /为你推荐/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /实施计划/ })).not.toBeInTheDocument()
  })

  it('vertically centers the main home content column', () => {
    const { container } = render(<HomePage />)
    const pageWrapper = container.querySelector('.mx-auto.max-w-\\[1032px\\]')
    expect(pageWrapper?.className).toMatch(/justify-center/)
  })

  it('uses tenant logoUrl from branding store as mascot', () => {
    const { container } = render(<HomePage />)
    const mascotImg = container.querySelector('[data-testid="home-mascot"] img')
    expect(mascotImg).toHaveAttribute('src', '/app-icon.png')
  })



})
