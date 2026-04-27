import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const workspaceState = vi.hoisted(() => ({
  current: null as null | { id: string; rootPath: string; displayName: string },
}))

const tauriMock = vi.hoisted(() => ({
  createConversation: vi.fn(async () => 'conv-new'),
  listAgents: vi.fn(async () => []),
  pickLocalDirectory: vi.fn<(options?: { defaultPath?: string; title?: string }) => Promise<string | null>>(async () => '/tmp/reports'),
  authorizeLocalDirectory: vi.fn(async (path: string) => {
    workspaceState.current = {
      id: 'aw-1',
      rootPath: path,
      displayName: path.split('/').filter(Boolean).pop() ?? path,
    }
    return workspaceState.current
  }),
  getAuthorizedWorkspace: vi.fn(async () => workspaceState.current),
  revokeAuthorizedWorkspace: vi.fn(async () => {
    workspaceState.current = null
  }),
}))

vi.mock('@/lib/tauri', () => tauriMock)
vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: vi.fn(),
    isStreaming: false,
    stopCurrentStream: vi.fn(),
  }),
}))
vi.mock('@/hooks/useFileUpload', () => ({
  useFileUpload: () => ({
    isUploading: false,
    selectAndUploadFiles: vi.fn(async () => []),
  }),
}))
vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: () => '#0f766e',
}))
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}))

import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { WorkspaceAuthPanel } from './WorkspaceAuthPanel'
import { useChatStore } from '@/stores/chatStore'

describe('Workspace-First frontend integration', () => {
  beforeEach(() => {
    workspaceState.current = null
    tauriMock.createConversation.mockClear()
    tauriMock.pickLocalDirectory.mockClear()
    tauriMock.authorizeLocalDirectory.mockClear()
    tauriMock.getAuthorizedWorkspace.mockClear()
    tauriMock.revokeAuthorizedWorkspace.mockClear()
    useChatStore.setState({
      activeConversationId: 'conv-workspace',
      conversations: [],
      messages: [],
    })
  })

  it('keeps settings authorization flow working without requiring composer status copy', async () => {
    render(
        <>
          <WorkspaceAuthPanel sessionId="conv-workspace" />
          <ChatBottomArea />
        </>,
    )

    fireEvent.click(await screen.findByRole('button', { name: '选择工作目录' }))

    await waitFor(() => {
      expect(tauriMock.authorizeLocalDirectory).toHaveBeenCalledWith(
        '/tmp/reports',
        'conv-workspace',
      )
    })

    expect(await screen.findByText('reports')).toBeInTheDocument()
    expect(screen.queryByText(/已连接本地目录：/)).not.toBeInTheDocument()
    expect(screen.queryByText('AI 当前可直接读取该目录，无需先上传文件')).not.toBeInTheDocument()
    expect(screen.queryByText(/workspace on/i)).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '撤销授权' }))

    await waitFor(() => {
      expect(tauriMock.revokeAuthorizedWorkspace).toHaveBeenCalledWith(
        'conv-workspace',
      )
    })

    await waitFor(() => {
      expect(screen.queryByText('reports')).not.toBeInTheDocument()
    })
  })

  it('does not expose a composer connect-directory action anymore', async () => {
    useChatStore.setState({
      activeConversationId: null,
      conversations: [],
      messages: [],
    })

    render(<ChatBottomArea />)

    expect(screen.queryByText('连接本地目录（不复制）')).not.toBeInTheDocument()
    expect(tauriMock.pickLocalDirectory).not.toHaveBeenCalled()
    expect(tauriMock.authorizeLocalDirectory).not.toHaveBeenCalled()
  })
})
