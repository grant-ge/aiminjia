import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  channelGetPlatforms: vi.fn(),
  channelGetPlatform: vi.fn(),
  channelGetConversations: vi.fn(),
  channelBeginRegistration: vi.fn(),
  channelPollRegistration: vi.fn(),
  channelSetEnabled: vi.fn(),
  channelRemovePlatform: vi.fn(),
  channelRevealSecret: vi.fn(),
  onChannelPlatformState: vi.fn().mockResolvedValue(() => {}),
  onChannelMessage: vi.fn().mockResolvedValue(() => {}),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import { useChannelStore } from './channelStore'
import { useUiStore } from './uiStore'
import type { ChannelPlatformState } from '@/lib/tauri'

function platformState(overrides: Partial<ChannelPlatformState> = {}): ChannelPlatformState {
  return {
    platform: 'dingtalk',
    capability: 'available',
    configured: true,
    enabled: true,
    connection: 'connected',
    config: {
      platform: 'dingtalk',
      appKey: 'app_key_1',
      appSecretMasked: 'sk_****_tail',
      robotCode: 'robot_1',
      robotCodeSource: 'registration',
      source: 'OPEN_CLAW',
      createdAt: '2026-05-07T00:00:00Z',
      updatedAt: '2026-05-07T00:00:00Z',
    },
    ...overrides,
  }
}

function resetStore() {
  useChannelStore.setState({
    platforms: {},
    conversations: [],
  })
}

describe('channelStore platform domain', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.resetModules()
    resetStore()
  })

  it('loadPlatforms converts platform array into a map keyed by platform', async () => {
    const dingtalk = platformState({ platform: 'dingtalk' })
    const feishu = platformState({
      platform: 'feishu',
      capability: 'comingSoon',
      configured: false,
      enabled: false,
      connection: 'unconfigured',
      config: null,
    })
    tauriMock.channelGetPlatforms.mockResolvedValue([dingtalk, feishu])

    await useChannelStore.getState().loadPlatforms()

    expect(useChannelStore.getState().platforms).toEqual({
      dingtalk,
      feishu,
    })
  })

  it('setEnabled updates only the returned platform state', async () => {
    const existingFeishu = platformState({
      platform: 'feishu',
      capability: 'comingSoon',
      configured: false,
      enabled: false,
      connection: 'unconfigured',
      config: null,
    })
    const disabledDingtalk = platformState({ enabled: false, connection: 'disconnected' })
    useChannelStore.setState({
      platforms: {
        dingtalk: platformState({ enabled: true, connection: 'connected' }),
        feishu: existingFeishu,
      },
    })
    tauriMock.channelSetEnabled.mockResolvedValue(disabledDingtalk)

    await useChannelStore.getState().setEnabled('dingtalk', false)

    expect(tauriMock.channelSetEnabled).toHaveBeenCalledWith('dingtalk', false)
    expect(useChannelStore.getState().platforms.dingtalk).toEqual(disabledDingtalk)
    expect(useChannelStore.getState().platforms.feishu).toBe(existingFeishu)
  })

  it('removePlatform clears runtime conversations and resets route when active session belongs to removed platform', async () => {
    const unconfigured = platformState({
      configured: false,
      enabled: false,
      connection: 'unconfigured',
      config: null,
    })
    useChannelStore.setState({
      platforms: { dingtalk: platformState() },
      conversations: [
        {
          sessionId: 'dingtalk_session_1',
          platform: 'dingtalk',
          conversationType: 'group',
          externalId: 'group_1',
          displayName: 'DingTalk Group',
          unreadCount: 3,
          robotCode: 'robot-test',
          isActiveRobot: true,
        },
      ],
    })
    useUiStore.setState({ route: { kind: 'channel', sessionId: 'dingtalk_session_1' } })
    tauriMock.channelRemovePlatform.mockResolvedValue(unconfigured)

    await useChannelStore.getState().removePlatform('dingtalk')

    expect(useChannelStore.getState().conversations).toEqual([])
    expect(useUiStore.getState().route).toEqual({ kind: 'channel' })
  })

  it('removePlatform writes the returned unconfigured platform state', async () => {
    const unconfigured = platformState({
      configured: false,
      enabled: false,
      connection: 'unconfigured',
      config: null,
    })
    useChannelStore.setState({ platforms: { dingtalk: platformState() } })
    tauriMock.channelRemovePlatform.mockResolvedValue(unconfigured)

    await useChannelStore.getState().removePlatform('dingtalk')

    expect(tauriMock.channelRemovePlatform).toHaveBeenCalledWith('dingtalk')
    expect(useChannelStore.getState().platforms.dingtalk).toEqual(unconfigured)
  })

  it('revealSecret returns plaintext without storing it in JSON state', async () => {
    const secret = 'full_app_secret_should_not_be_persisted'
    useChannelStore.setState({ platforms: { dingtalk: platformState() } })
    tauriMock.channelRevealSecret.mockResolvedValue(secret)

    const revealed = await useChannelStore.getState().revealSecret('dingtalk')

    expect(revealed).toBe(secret)
    expect(JSON.stringify(useChannelStore.getState())).not.toContain(secret)
  })

  it('initChannelListeners triggers loadConversations on startup', async () => {
    const { initChannelListeners } = await import('./channelStore')
    const channelGetConversationsMock = (
      await import('@/lib/tauri')
    ).channelGetConversations as ReturnType<typeof vi.fn>

    channelGetConversationsMock.mockResolvedValue([])
    await initChannelListeners()

    expect(channelGetConversationsMock).toHaveBeenCalled()
  })

  it('platform-state event triggers loadConversations refresh', async () => {
    const tauriMod = await import('@/lib/tauri')
    const onChannelPlatformStateMock = tauriMod.onChannelPlatformState as ReturnType<typeof vi.fn>
    const channelGetConversationsMock = tauriMod.channelGetConversations as ReturnType<typeof vi.fn>

    let capturedHandler: ((p: any) => void) | null = null
    onChannelPlatformStateMock.mockImplementation((handler: (p: any) => void) => {
      capturedHandler = handler
      return Promise.resolve(() => {})
    })
    channelGetConversationsMock.mockResolvedValue([])

    const { initChannelListeners } = await import('./channelStore')
    await initChannelListeners()

    channelGetConversationsMock.mockClear()
    ;(capturedHandler as ((payload: any) => void) | null)?.({
      state: {
        platform: 'dingtalk',
        capability: 'available',
        configured: true,
        enabled: true,
        connection: 'connected',
        config: null,
        lastConnectedAt: null,
        lastError: null,
      },
    })

    await new Promise((r) => setTimeout(r, 0))
    expect(channelGetConversationsMock).toHaveBeenCalled()
  })
})
