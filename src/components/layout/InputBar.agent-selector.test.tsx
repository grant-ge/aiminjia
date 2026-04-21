import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const sendUserMessageMock = vi.hoisted(() => vi.fn(async () => undefined))
const listAgentsMock = vi.hoisted(
  () =>
    vi.fn<
      () => Promise<Array<{ name: string; description: string; source: 'builtin' | 'user' }>>
    >(async () => []),
)

vi.mock('@/lib/tauri', () => ({
  listAgents: listAgentsMock,
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: sendUserMessageMock,
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

describe('InputBar agent selector', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    listAgentsMock.mockResolvedValue([
      {
        name: 'browse_data_agent',
        description: 'Browse Data',
        source: 'builtin',
      },
      {
        name: 'daily_assistant_agent',
        description: 'Daily Assistant',
        source: 'builtin',
      },
    ])

    useChatStore.setState({
      activeConversationId: 'conv-agent',
    })
  })

  it('shows registered agents and sends the selected agent name', async () => {
    render(<InputBar />)

    const selector = await screen.findByRole('combobox')
    expect(screen.getByRole('option', { name: '自动（默认）' })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'Browse Data' })).toBeInTheDocument()

    fireEvent.change(selector, { target: { value: 'browse_data_agent' } })

    const textarea = screen.getByPlaceholderText('inputBar.placeholder')
    fireEvent.change(textarea, { target: { value: '帮我查一下本周报表' } })
    fireEvent.keyDown(textarea, { key: 'Enter', code: 'Enter', charCode: 13 })

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith(
        '帮我查一下本周报表',
        undefined,
        'browse_data_agent',
      )
    })
  })
})
