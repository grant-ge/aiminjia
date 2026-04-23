import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { HomeTaskComposerCard } from '../HomeTaskComposerCard'

vi.mock('@/lib/tauri', () => ({
  getDefaultFolder: vi.fn().mockResolvedValue({
    id: 'default',
    rootPath: '/Users/test/.renlijia/defaultFolder',
    displayName: '默认项目',
  }),
  pickLocalDirectory: vi.fn(),
  authorizeLocalDirectory: vi.fn().mockResolvedValue({ id: 'ws1', rootPath: '/tmp/proj', displayName: 'proj' }),
  createConversation: vi.fn().mockResolvedValue('conv-123'),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: vi.fn().mockResolvedValue(undefined) }),
}))

vi.mock('@/hooks/useSkillComposer', () => ({
  useSkillComposer: () => ({
    showSkillPopover: false,
    setShowSkillPopover: vi.fn(),
    slashMatch: null,
    slashOpen: false,
    handleSkillPick: vi.fn(),
    handleSlashSelect: vi.fn(),
    handleSlashClose: vi.fn(),
  }),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: {
    getState: () => ({
      conversations: [],
      setConversations: vi.fn(),
      setActiveConversation: vi.fn(),
      setMessages: vi.fn(),
    }),
  },
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: {
    getState: () => ({ setRoute: vi.fn() }),
  },
}))

vi.mock('@/stores/homeStore', () => ({
  useHomeStore: () => ({
    selectedWorkspace: null,
    setSelectedWorkspace: vi.fn(),
  }),
}))

vi.mock('@/components/chat/SkillPopover', () => ({ SkillPopover: () => null }))
vi.mock('@/components/chat/SlashCommandPopover', () => ({ SlashCommandPopover: () => null }))

describe('HomeTaskComposerCard', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('shows 默认项目 after loading default folder', async () => {
    render(<HomeTaskComposerCard />)
    await waitFor(() => {
      expect(screen.getByText('默认项目')).toBeInTheDocument()
    })
  })

  it('updates project label after user picks a directory', async () => {
    const { pickLocalDirectory } = await import('@/lib/tauri')
    vi.mocked(pickLocalDirectory).mockResolvedValueOnce('/Users/test/myproject')

    render(<HomeTaskComposerCard />)
    await waitFor(() => screen.getByText('默认项目'))

    fireEvent.click(screen.getByText('默认项目'))
    await waitFor(() => {
      expect(screen.getByText('myproject')).toBeInTheDocument()
    })
  })
})
