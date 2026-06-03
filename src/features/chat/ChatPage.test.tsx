import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { useChatStore } from '@/stores/chatStore'
import { setExpertTeam, clearExpertTeam } from '@/features/expert-teams/expertTeamRegistry'
import { ChatPage } from './ChatPage'

const switchConversationMock = vi.hoisted(() => vi.fn())
const tauriMocks = vi.hoisted(() => ({
  exportConversation: vi.fn(),
  revealExportInFolder: vi.fn(),
  getConversationSource: vi.fn(),
  openGeneratedFile: vi.fn(),
  clearConversationSource: vi.fn(),
  setConversationExpertTeam: vi.fn(),
  getTeamOverview: vi.fn(),
  onMessageUpdated: vi.fn(),
  onToolCompleted: vi.fn(),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ switchConversation: switchConversationMock }),
}))

vi.mock('@/hooks/useTeamOverview', () => ({
  useTeamOverview: () => ({ overview: null, loaded: true, refetch: vi.fn() }),
}))

vi.mock('@/lib/tauri', () => ({
  exportConversation: tauriMocks.exportConversation,
  revealExportInFolder: tauriMocks.revealExportInFolder,
  getConversationSource: tauriMocks.getConversationSource,
  openGeneratedFile: tauriMocks.openGeneratedFile,
  clearConversationSource: tauriMocks.clearConversationSource,
  setConversationExpertTeam: tauriMocks.setConversationExpertTeam,
  getTeamOverview: tauriMocks.getTeamOverview,
  onMessageUpdated: tauriMocks.onMessageUpdated,
  onToolCompleted: tauriMocks.onToolCompleted,
}))

vi.mock('@/components/shell/ChatTopBar', () => ({
  ChatTopBar: ({
    title,
    sourceLabel,
    onShare,
    shareLabel,
  }: {
    title: string
    sourceLabel?: string
    onShare?: () => void
    shareLabel?: string
  }) => (
    <header data-testid="chat-header">
      {title}
      {sourceLabel ? <span data-testid="chat-source-label">{sourceLabel}</span> : null}
      {onShare ? <button onClick={onShare}>{shareLabel ?? '分享'}</button> : null}
    </header>
  ),
}))

vi.mock('@/components/layout/ChatArea', () => ({
  ChatArea: () => <main data-testid="chat-content" />,
}))

vi.mock('@/components/chat-scene/ChatBottomArea', () => ({
  ChatBottomArea: () => <footer data-testid="chat-footer-input" />,
}))

vi.mock('@/components/chat/RightPanel', () => ({
  RightPanel: () => <div data-testid="right-panel" />,
}))

vi.mock('@/components/team/TeamChatDrawer', () => ({
  TeamChatDrawer: () => <aside data-testid="team-chat-drawer" />,
}))

describe('ChatPage layout', () => {
  beforeEach(async () => {
    switchConversationMock.mockClear()
    tauriMocks.exportConversation.mockReset()
    tauriMocks.revealExportInFolder.mockReset()
    tauriMocks.getConversationSource.mockReset()
    tauriMocks.openGeneratedFile.mockReset()
    tauriMocks.clearConversationSource.mockReset()
    tauriMocks.setConversationExpertTeam.mockReset()
    tauriMocks.getTeamOverview.mockReset()
    tauriMocks.onMessageUpdated.mockReset()
    tauriMocks.onToolCompleted.mockReset()
    tauriMocks.clearConversationSource.mockResolvedValue(undefined)
    tauriMocks.setConversationExpertTeam.mockResolvedValue(undefined)
    tauriMocks.getTeamOverview.mockResolvedValue(null)
    tauriMocks.onMessageUpdated.mockResolvedValue(() => undefined)
    tauriMocks.onToolCompleted.mockResolvedValue(() => undefined)
    await i18n.changeLanguage('zh-CN')
    await clearExpertTeam('conv-layout')
    await clearExpertTeam('conv-team')
    await clearExpertTeam('conv-retro')
    useChatStore.setState({ activeConversationId: null, conversations: [], messages: [] })
  })


  it('loads messages on reload when route conversation is already active but message cache is empty', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-reload',
      conversations: [{ id: 'conv-reload', title: '刷新恢复', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-reload" />)

    await waitFor(() => {
      expect(switchConversationMock).toHaveBeenCalledWith('conv-reload')
    })
  })



  it('does not render a redundant expert team banner above the chat content', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-team',
      conversations: [{ id: 'conv-team', title: '专家团会话', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [{ id: 'm1', conversationId: 'conv-team', role: 'assistant', content: { text: '已有消息' }, createdAt: '' }],
    })
    await setExpertTeam('conv-team', 'marketing')

    render(<ChatPage conversationId="conv-team" />)

    expect(screen.queryByLabelText('关闭专家团')).not.toBeInTheDocument()
  })

  it('uses the localized expert team name for the conversation source chip', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-retro',
      conversations: [{
        id: 'conv-retro',
        title: '专家团会话',
        createdAt: '',
        updatedAt: '',
        isArchived: false,
        kind: 'expertTeam',
        sourceLabel: 'Retrospective Diagnosis Team',
      }],
      messages: [{ id: 'm1', conversationId: 'conv-retro', role: 'assistant', content: { text: '已有消息' }, createdAt: '' }],
    })
    await setExpertTeam('conv-retro', 'retrospective', 'Retrospective Diagnosis Team')

    render(<ChatPage conversationId="conv-retro" />)

    expect(screen.getByTestId('chat-source-label')).toHaveTextContent('复盘归因团')
    expect(screen.queryByText('Retrospective Diagnosis Team')).not.toBeInTheDocument()
  })

  it('composes the chat column as header, content, and footer using flex layout', () => {
    useChatStore.setState({
      activeConversationId: 'conv-layout',
      conversations: [{ id: 'conv-layout', title: '布局测试', createdAt: '', updatedAt: '', isArchived: false }],
    })

    render(<ChatPage conversationId="conv-layout" />)

    const column = screen.getByTestId('chat-layout-column')
    expect(column).toHaveClass('flex')
    expect(column).toHaveClass('flex-col')
    expect(column).toHaveClass('overflow-hidden')

    expect(screen.getByTestId('chat-header')).toBeInTheDocument()
    expect(screen.getByTestId('chat-content')).toBeInTheDocument()
    expect(screen.getByTestId('chat-footer-input')).toBeInTheDocument()
    expect(screen.queryByTestId('right-panel')).not.toBeInTheDocument()
  })

  it('exports the current conversation and can reveal the zip in folder', async () => {
    tauriMocks.exportConversation.mockResolvedValue({
      zipPath: '/tmp/aijia-export.zip',
      fileName: 'aijia-export.zip',
      sizeBytes: 2048,
    })
    tauriMocks.revealExportInFolder.mockResolvedValue(undefined)
    useChatStore.setState({
      activeConversationId: 'conv-export',
      conversations: [{ id: 'conv-export', title: '导出测试', createdAt: '', updatedAt: '', isArchived: false }],
      messages: [],
    })

    render(<ChatPage conversationId="conv-export" />)
    fireEvent.click(screen.getByRole('button', { name: '导出对话' }))

    await waitFor(() => {
      expect(tauriMocks.exportConversation).toHaveBeenCalledWith('conv-export')
    })
    expect(await screen.findByText('aijia-export.zip')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '打开所在文件夹' }))
    await waitFor(() => {
      expect(tauriMocks.revealExportInFolder).toHaveBeenCalledWith('/tmp/aijia-export.zip')
    })
  })

  it('drops stale export results after switching conversations', async () => {
    let resolveFirstExport!: (result: { zipPath: string; fileName: string; sizeBytes: number }) => void
    const firstExport = new Promise<{ zipPath: string; fileName: string; sizeBytes: number }>((resolve) => {
      resolveFirstExport = resolve
    })
    tauriMocks.exportConversation
      .mockReturnValueOnce(firstExport)
      .mockResolvedValueOnce({
        zipPath: '/tmp/conv-b.zip',
        fileName: 'conv-b.zip',
        sizeBytes: 4096,
      })
    useChatStore.setState({
      activeConversationId: 'conv-a',
      conversations: [
        { id: 'conv-a', title: '会话 A', createdAt: '', updatedAt: '', isArchived: false },
        { id: 'conv-b', title: '会话 B', createdAt: '', updatedAt: '', isArchived: false },
      ],
      messages: [],
    })

    const { rerender } = render(<ChatPage conversationId="conv-a" />)
    fireEvent.click(screen.getByRole('button', { name: '导出对话' }))
    await waitFor(() => {
      expect(tauriMocks.exportConversation).toHaveBeenCalledWith('conv-a')
    })

    await act(async () => {
      useChatStore.setState({ activeConversationId: 'conv-b' })
      rerender(<ChatPage conversationId="conv-b" />)
    })

    await act(async () => {
      resolveFirstExport({
        zipPath: '/tmp/conv-a.zip',
        fileName: 'conv-a.zip',
        sizeBytes: 2048,
      })
      await firstExport
    })

    expect(screen.queryByText('conv-a.zip')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '导出对话' }))
    await waitFor(() => {
      expect(tauriMocks.exportConversation).toHaveBeenCalledWith('conv-b')
    })
    expect(await screen.findByText('conv-b.zip')).toBeInTheDocument()
  })
})
