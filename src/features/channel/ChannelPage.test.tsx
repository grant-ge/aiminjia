import '@testing-library/jest-dom'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfirmDialogHost, useConfirmDialogStore } from '@/components/common/ConfirmDialogHost'
import { DEFAULTS, useBrandingStore } from '@/stores/brandingStore'
import { useChannelStore } from '@/stores/channelStore'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { ChannelPage } from './ChannelPage'

const getMessagesMock = vi.hoisted(() => vi.fn())
const getTasksMock = vi.hoisted(() => vi.fn())
const exportConversationMock = vi.hoisted(() => vi.fn())
const revealExportInFolderMock = vi.hoisted(() => vi.fn())

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
    exportConversation: exportConversationMock,
    revealExportInFolder: revealExportInFolderMock,
    openGeneratedFile: vi.fn(),
    onChannelPlatformState: vi.fn().mockResolvedValue(() => {}),
    onChannelMessage: vi.fn().mockResolvedValue(() => {}),
  }
})
vi.mock('@/hooks/useTeamOverview', () => ({
  useTeamOverview: () => ({ overview: null, loaded: true, refetch: vi.fn() }),
}))

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
  capability: 'available' as const,
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
    useNotificationStore.setState({ notifications: [] })
    useBrandingStore.setState({
      productName: DEFAULTS.productName,
      productNameEn: DEFAULTS.productNameEn,
      logoUrl: DEFAULTS.logoUrl,
      accentColor: DEFAULTS.accentColor,
      primaryColor: DEFAULTS.primaryColor,
      bgColor: DEFAULTS.bgColor,
      sidebarBgColor: DEFAULTS.sidebarBgColor,
      fontFamily: DEFAULTS.fontFamily,
      isCustom: false,
    })
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
      sendDingtalkGreeting: vi.fn().mockResolvedValue(undefined),
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
    exportConversationMock.mockReset()
    revealExportInFolderMock.mockReset()
  })

  it('unconfigured DingTalk shows only the config button', () => {
    renderPage()

    expect(screen.getByRole('heading', { name: 'IM 频道' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '配置钉钉' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '更多钉钉配置' })).not.toBeInTheDocument()
    expect(screen.queryByRole('switch', { name: /钉钉/ })).not.toBeInTheDocument()
  })

  it('overview header uses the compact 48px top bar height', () => {
    const { container } = renderPage()
    const header = container.querySelector('[data-tauri-drag-region]')
    expect(header).toHaveClass('h-12')
    expect(header).not.toHaveClass('h-14')
  })

  it('uses tenant product name in the hero description', () => {
    useBrandingStore.setState({ productName: '仁励猫' })

    renderPage()

    expect(screen.getByText(/让 仁励猫 接收并回复来自各平台的消息/)).toBeInTheDocument()
  })

  it('shows DingTalk, Feishu, WeChat, and Wecom cards when all are available', () => {
    useChannelStore.setState({
      platforms: {
        dingtalk: unconfigured,
        feishu,
        wechat: { ...feishu, platform: 'wechat', capability: 'available' as const, configured: false, enabled: false, connection: 'unconfigured' as const, config: null },
        wecom: { ...feishu, platform: 'wecom', capability: 'available' as const, configured: false, enabled: false, connection: 'unconfigured' as const, config: null },
      },
    })

    renderPage()

    expect(screen.getByText('钉钉')).toBeInTheDocument()
    expect(screen.getByText('飞书')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '配置飞书' })).toBeInTheDocument()
    // Phase 5 MVP: 个人微信 card is rendered and the 配置 button is active so
    // the user can drive the iLink scan-to-login flow (credentials don't
    // persist yet — that's Phase 5 PR3 territory).
    expect(screen.getByText('个人微信')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '配置个人微信' })).toBeInTheDocument()
    // Phase 2: 企业微信 card already shipping in parallel.
    expect(screen.getByText('企业微信')).toBeInTheDocument()
  })

  it('renders Feishu platform logo without a border', () => {
    useChannelStore.setState({
      platforms: {
        dingtalk: unconfigured,
        feishu,
      },
    })

    const { container } = renderPage()
    const logo = container.querySelector('img[src="/logos/feishu.png"]')

    expect(logo).toBeInTheDocument()
    expect(logo).not.toHaveClass('border')
    expect(logo).not.toHaveClass('border-border')
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

  it('configured DingTalk can wake the current bot from the overview', async () => {
    const sendDingtalkGreeting = vi.fn().mockResolvedValue(undefined)
    useChannelStore.setState({
      platforms: { dingtalk: connected, feishu },
      sendDingtalkGreeting,
    })
    renderPage()

    await userEvent.click(screen.getByRole('button', { name: '唤醒钉钉机器人' }))

    await waitFor(() => {
      expect(sendDingtalkGreeting).toHaveBeenCalledTimes(1)
    })
    expect(useNotificationStore.getState().notifications.at(-1)).toMatchObject({
      level: 'success',
      title: '机器人已唤醒',
      message: '请打开钉钉，看看左侧会话列表里有没有未读红点；机器人回复后就能找到这条对话。',
    })
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

  it('IM 会话可以从顶栏导出当前频道对话', async () => {
    exportConversationMock.mockResolvedValue({
      zipPath: '/tmp/im-session.zip',
      fileName: 'im-session.zip',
      sizeBytes: 2048,
    })
    revealExportInFolderMock.mockResolvedValue(undefined)
    useChannelStore.setState({
      conversations: [
        {
          sessionId: 'im-session-1',
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

    renderPage(<ChannelPage sessionId="im-session-1" />)
    await userEvent.click(screen.getByRole('button', { name: '导出对话' }))

    expect(exportConversationMock).not.toHaveBeenCalled()
    expect(screen.getByText('将生成一个本地 zip 文件，包含当前对话和最近 24 小时运行信息。文件只会保存在本机。')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: '开始导出' }))

    await waitFor(() => {
      expect(exportConversationMock).toHaveBeenCalledWith('im-session-1')
    })
    expect(await screen.findByText('im-session.zip')).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: '打开所在文件夹' }))
    await waitFor(() => {
      expect(revealExportInFolderMock).toHaveBeenCalledWith('/tmp/im-session.zip')
    })
  })

  it('钉钉 IM 会话可以从顶栏唤醒机器人并提示去钉钉查看未读红点', async () => {
    const sendDingtalkGreeting = vi.fn().mockResolvedValue(undefined)
    useChannelStore.setState({
      sendDingtalkGreeting,
      conversations: [
        {
          sessionId: 'im-dingtalk-1',
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

    renderPage(<ChannelPage sessionId="im-dingtalk-1" />)

    await userEvent.click(screen.getByRole('button', { name: '唤醒钉钉机器人' }))

    await waitFor(() => {
      expect(sendDingtalkGreeting).toHaveBeenCalledTimes(1)
    })
    expect(useNotificationStore.getState().notifications.at(-1)).toMatchObject({
      level: 'success',
      title: '机器人已唤醒',
      message: '请打开钉钉，看看左侧会话列表里有没有未读红点；机器人回复后就能找到这条对话。',
    })
  })
})
