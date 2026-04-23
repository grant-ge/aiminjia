import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useHomeStore } from '@/stores/homeStore'

import { HomeTaskComposerCard } from '../HomeTaskComposerCard'

vi.mock('@/lib/tauri', () => ({
  getDefaultFolder: vi.fn().mockResolvedValue({
    id: 'default',
    rootPath: '/Users/test/.renlijia/defaultFolder',
    displayName: '测试默认项目', // distinct from static fallback '默认项目'
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
  useHomeStore: vi.fn().mockReturnValue({
    selectedWorkspace: null,
    setSelectedWorkspace: vi.fn(),
  }),
}))

vi.mock('@/components/chat/SkillPopover', () => ({ SkillPopover: () => null }))
vi.mock('@/components/chat/SlashCommandPopover', () => ({ SlashCommandPopover: () => null }))

describe('HomeTaskComposerCard', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    // Restore default homeStore mock so each test starts clean
    vi.mocked(useHomeStore).mockReturnValue({
      selectedWorkspace: null,
      setSelectedWorkspace: vi.fn(),
    })
  })

  it('shows 测试默认项目 after loading default folder', async () => {
    render(<HomeTaskComposerCard />)
    await waitFor(() => {
      expect(screen.getByText('测试默认项目')).toBeInTheDocument()
    })
  })

  it('updates project label after user picks a directory', async () => {
    const { pickLocalDirectory } = await import('@/lib/tauri')
    vi.mocked(pickLocalDirectory).mockResolvedValueOnce('/Users/test/myproject')

    render(<HomeTaskComposerCard />)
    await waitFor(() => screen.getByText('测试默认项目'))

    fireEvent.click(screen.getByText('测试默认项目'))
    await waitFor(() => {
      expect(screen.getByText('myproject')).toBeInTheDocument()
    })
    expect(vi.mocked(pickLocalDirectory)).toHaveBeenCalledOnce()
  })

  it('persists workspace to homeStore on pick', async () => {
    const setSelectedWorkspace = vi.fn()
    vi.mocked(useHomeStore).mockReturnValue({
      selectedWorkspace: null,
      setSelectedWorkspace,
    })

    const { pickLocalDirectory } = await import('@/lib/tauri')
    vi.mocked(pickLocalDirectory).mockResolvedValueOnce('/Users/test/myproject')

    render(<HomeTaskComposerCard />)
    await waitFor(() => screen.getByText('测试默认项目'))
    fireEvent.click(screen.getByText('测试默认项目'))

    await waitFor(() => {
      expect(setSelectedWorkspace).toHaveBeenCalledWith({
        id: 'myproject',
        rootPath: '/Users/test/myproject',
        displayName: 'myproject',
      })
    })
  })
})
