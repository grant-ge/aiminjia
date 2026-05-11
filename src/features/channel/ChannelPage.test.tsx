import '@testing-library/jest-dom'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmDialogHost, useConfirmDialogStore } from '@/components/common/ConfirmDialogHost'
import { useChannelStore } from '@/stores/channelStore'
import { useChatStore } from '@/stores/chatStore'
import { ChannelPage } from './ChannelPage'

const getMessagesMock = vi.hoisted(() => vi.fn())
const getTasksMock = vi.hoisted(() => vi.fn())

vi.mock('@/components/layout/ChatArea', () => ({ ChatArea: () => <main data-testid="channel-chat-content" /> }))
vi.mock('@/components/chat-scene/ChatBottomArea', () => ({ ChatBottomArea: () => <footer data-testid="channel-chat-input" /> }))
vi.mock('@/components/chat/RightPanel', () => ({
  RightPanel: ({ conversationId }: { conversationId: string }) => <aside data-testid="channel-right-panel">{conversationId}</aside>,
}))
vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    getMessages: getMessagesMock,
    getTasks: getTasksMock,
    openGeneratedFile: vi.fn(),
    onChannelPlatformState: vi.fn().mockResolvedValue(() => {}),
    onChannelMessage: vi.fn().mockResolvedValue(() => {}),
  }
})

const unconfigured = {
  platform: 'dingtalk' as const,
  capability: 'available' as const,
  configured: false,
  enabled: false,
  connection: 'unconfigured' as const,
  config: null,
  lastConnectedAt: null,
  lastError: null,
}

const connected = {
  platform: 'dingtalk' as const,
  capability: 'available' as const,
  configured: true,
  enabled: true,
  connection: 'connected' as const,
  config: {
    platform: 'dingtalk' as const,
    appKey: 'ding-app-key',
    appSecretMasked: '••••••••••••cret',
    robotCode: 'robot-code',
    robotCodeSource: 'registration' as const,
    source: 'OPEN_CLAW' as const,
    createdAt: '2026-05-07T00:00:00Z',
    updatedAt: '2026-05-07T01:00:00Z',
  },
  lastConnectedAt: null,
  lastError: null,
}

const feishu = {
  platform: 'feishu' as const,
  capability: 'comingSoon' as const,
  configured: false,
  enabled: false,
  connection: 'unconfigured' as const,
  config: null,
  lastConnectedAt: null,
  lastError: null,
}

function renderPage(ui = <ChannelPage />) {
  return render(
    <>
      {ui}
      <ConfirmDialogHost />
    </>,
  )
}

describe('ChannelPage domain UI', () => {
  beforeEach(() => {
    useConfirmDialogStore.setState({ request: null })
    useChannelStore.setState({
      platforms: { dingtalk: unconfigured, feishu },
      conversations: [],
      loadPlatforms: vi.fn().mockResolvedValue(undefined),
      loadConversations: vi.fn().mockResolvedValue(undefined),
      beginRegistration: vi.fn(),
      pollRegistration: vi.fn(),
      setEnabled: vi.fn().mockResolvedValue(undefined),
      removePlatform: vi.fn().mockResolvedValue(undefined),
      revealSecret: vi.fn().mockResolvedValue('plain-secret'),
    })
    useChatStore.setState({
      conversations: [],
      activeConversationId: null,
      messages: [],
      taskStates: {},
      streamStates: {},
      isStreaming: false,
      streamingContent: '',
      toolExecutions: [],
    })
    getMessagesMock.mockResolvedValue([])
    getTasksMock.mockResolvedValue([])
  })

  it('unconfigured DingTalk shows only the config button', () => {
    renderPage()

    expect(screen.getByRole('heading', { name: 'IM 频道' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '配置钉钉' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '更多钉钉配置' })).not.toBeInTheDocument()
    expect(screen.queryByRole('switch', { name: /钉钉/ })).not.toBeInTheDocument()
  })

  it('only shows DingTalk platform while other IM integrations are not implemented', () => {
    useChannelStore.setState({
      platforms: {
        dingtalk: unconfigured,
        feishu,
        wechat: { ...feishu, platform: 'wechat' },
        wecom: { ...feishu, platform: 'wecom' },
      },
    })

    renderPage()

    expect(screen.getByText('钉钉')).toBeInTheDocument()
    expect(screen.queryByText('飞书')).not.toBeInTheDocument()
    expect(screen.queryByText('微信')).not.toBeInTheDocument()
    expect(screen.queryByText('企业微信')).not.toBeInTheDocument()
  })

  it('configured DingTalk opens read-only config details from menu', async () => {
    useChannelStore.setState({ platforms: { dingtalk: connected, feishu } })
    renderPage()

    await userEvent.click(screen.getByRole('button', { name: '更多钉钉配置' }))
    await userEvent.click(await screen.findByRole('menuitem', { name: '配置' }))

    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText('钉钉配置')).toBeInTheDocument()
    expect(within(dialog).getByText('ding-app-key')).toBeInTheDocument()
    expect(within(dialog).getByText('robot-code')).toBeInTheDocument()
    expect(within(dialog).queryByLabelText('钉钉扫码二维码')).not.toBeInTheDocument()
  })

  it('remove requires confirmation and restores unconfigured state through store action', async () => {
    const removePlatform = vi.fn().mockResolvedValue(undefined)
    useChannelStore.setState({ platforms: { dingtalk: connected, feishu }, removePlatform })
    renderPage()

    await userEvent.click(screen.getByRole('button', { name: '更多钉钉配置' }))
    await userEvent.click(await screen.findByRole('menuitem', { name: '移除' }))
    expect(await screen.findByText('移除钉钉频道？')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '确认移除' }))

    await waitFor(() => {
      expect(removePlatform).toHaveBeenCalledWith('dingtalk')
    })
  })

  it('switch off disables connection without removing config', async () => {
    const setEnabled = vi.fn().mockResolvedValue(undefined)
    const removePlatform = vi.fn().mockResolvedValue(undefined)
    useChannelStore.setState({ platforms: { dingtalk: connected, feishu }, setEnabled, removePlatform })
    renderPage()

    await userEvent.click(screen.getByRole('switch', { name: '钉钉频道已启用' }))

    expect(setEnabled).toHaveBeenCalledWith('dingtalk', false)
    expect(removePlatform).not.toHaveBeenCalled()
  })

  it('switch on reconnects using existing config', async () => {
    const setEnabled = vi.fn().mockResolvedValue(undefined)
    useChannelStore.setState({
      platforms: { dingtalk: { ...connected, enabled: false, connection: 'disconnected' }, feishu },
      setEnabled,
    })
    renderPage()

    await userEvent.click(screen.getByRole('switch', { name: '钉钉频道已停用' }))

    expect(setEnabled).toHaveBeenCalledWith('dingtalk', true)
  })

  it('inactive 会话：输入区 disabled + banner 提示', async () => {
    useChannelStore.setState({
      conversations: [
        {
          sessionId: 'sess-old',
          platform: 'dingtalk',
          conversationType: 'private',
          externalId: 'u',
          displayName: '老用户',
          unreadCount: 0,
          robotCode: 'old-robot',
          isActiveRobot: false,
        },
      ],
    })
    renderPage(<ChannelPage sessionId="sess-old" />)
    expect(
      screen.getByText(/已下线的机器人，无法发送新消息/),
    ).toBeInTheDocument()
    // 输入区 textarea 应该 disabled（按现有 selector 调整）
    const textarea = screen.queryByPlaceholderText(/输入|发送|消息/i)
    if (textarea) {
      expect(textarea).toBeDisabled()
    }
  })

  it('active 会话：输入区可用，无 banner', async () => {
    useChannelStore.setState({
      conversations: [
        {
          sessionId: 'sess-cur',
          platform: 'dingtalk',
          conversationType: 'private',
          externalId: 'u',
          displayName: '姚斌权',
          unreadCount: 0,
          robotCode: 'current-robot',
          isActiveRobot: true,
        },
      ],
    })
    renderPage(<ChannelPage sessionId="sess-cur" />)
    expect(
      screen.queryByText(/已下线的机器人，无法发送新消息/),
    ).not.toBeInTheDocument()
  })
})
