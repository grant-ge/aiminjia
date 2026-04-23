import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

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
  it('renders mascot title, category chips and skill-center pill', () => {
    render(<HomePage />)
    // HomeMascotHero title and HomeTaskComposerCard h1 both render this text
    expect(screen.getAllByText('创建你的下一条任务').length).toBeGreaterThanOrEqual(1)
    expect(screen.getByRole('button', { name: /为你推荐/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /前往技能中心/ })).toBeInTheDocument()
  })
})
