import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: vi.fn(async () => undefined),
    isStreaming: false,
    stopCurrentStream: vi.fn(),
  }),
}))

vi.mock('@/hooks/useAuthorizedWorkspace', () => ({
  useAuthorizedWorkspace: () => ({
    workspace: null,
  }),
}))

vi.mock('@/hooks/useFileUpload', () => ({
  useFileUpload: () => ({
    isUploading: false,
    selectAndUploadFiles: vi.fn(async () => []),
  }),
}))

vi.mock('@/hooks/useWorkspaceAuthorization', () => ({
  useWorkspaceAuthorization: () => ({
    isAuthorizingDirectory: false,
    selectAndAuthorizeDirectory: vi.fn(async () => undefined),
  }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: () => '#0f766e',
}))

vi.mock('@/stores/skillStore', () => ({
  useSkillStore: (selector: (state: { skills: Array<{ id: string; displayName: string }> }) => unknown) =>
    selector({
      skills: [
        { id: 'writing-plans', displayName: '写计划' },
        { id: 'research-brief', displayName: '研究简报' },
      ],
    }),
}))

const setRouteMock = vi.fn()

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (selector: (state: { setRoute: typeof setRouteMock }) => unknown) =>
    selector({
      setRoute: setRouteMock,
    }),
}))

vi.mock('@/components/chat/SlashCommandPopover', () => ({
  SlashCommandPopover: () => null,
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

import { InputBar } from '@/components/layout/InputBar'
import { useChatStore } from '@/stores/chatStore'

describe('InputBar skill popover', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useChatStore.setState({
      activeConversationId: 'conv-agent',
    })
  })

  it('shows skill popover entry and routes to skill detail', async () => {
    render(<InputBar />)

    fireEvent.click(screen.getByRole('button', { name: '技能' }))
    fireEvent.click(await screen.findByRole('button', { name: '写计划' }))

    expect(setRouteMock).toHaveBeenCalledWith({ kind: 'skill-detail', skillId: 'writing-plans' })
  })
})
