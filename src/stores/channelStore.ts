import { create } from 'zustand'
import {
  type ChannelConversation,
  type ChannelPlatform,
  type ChannelPlatformState,
  channelBeginRegistration,
  channelGetConversations,
  channelGetPlatform,
  channelGetPlatforms,
  channelPollRegistration,
  channelRemovePlatform,
  channelRevealSecret,
  channelSendDingtalkGreeting,
  channelSetEnabled,
  onChannelMessage,
  onChannelPlatformState,
} from '@/lib/tauri'
import { useUiStore } from './uiStore'

type PlatformMap = Partial<Record<ChannelPlatform, ChannelPlatformState>>

interface ChannelState {
  platforms: PlatformMap
  conversations: ChannelConversation[]

  setPlatformState: (state: ChannelPlatformState) => void
  setConversations: (convs: ChannelConversation[]) => void
  incrementUnread: (sessionId: string) => void
  clearUnread: (sessionId: string) => void
  loadPlatforms: () => Promise<void>
  loadPlatform: (platform: ChannelPlatform) => Promise<ChannelPlatformState | null>
  beginRegistration: (platform: ChannelPlatform) => ReturnType<typeof channelBeginRegistration>
  pollRegistration: (
    platform: ChannelPlatform,
    deviceCode: string,
  ) => ReturnType<typeof channelPollRegistration>
  setEnabled: (platform: ChannelPlatform, enabled: boolean) => Promise<ChannelPlatformState>
  removePlatform: (platform: ChannelPlatform) => Promise<ChannelPlatformState>
  revealSecret: (platform: ChannelPlatform) => Promise<string>
  sendDingtalkGreeting: () => Promise<void>
  loadConversations: (platform?: ChannelPlatform) => Promise<void>
  reset: () => void
}

export const useChannelStore = create<ChannelState>((set, get) => ({
  platforms: {},
  conversations: [],

  reset: () => set({ platforms: {}, conversations: [] }),

  setPlatformState: (state) => {
    set((s) => ({
      platforms: { ...s.platforms, [state.platform]: state },
    }))
  },

  setConversations: (convs) => set({ conversations: convs }),

  incrementUnread: (sessionId) =>
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.sessionId === sessionId ? { ...c, unreadCount: c.unreadCount + 1 } : c,
      ),
    })),

  clearUnread: (sessionId) =>
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.sessionId === sessionId ? { ...c, unreadCount: 0 } : c,
      ),
    })),

  loadPlatforms: async () => {
    try {
      const platforms = await channelGetPlatforms()
      const platformMap = Object.fromEntries(
        platforms.map((platformState) => [platformState.platform, platformState]),
      ) as PlatformMap
      set({
        platforms: platformMap,
      })
    } catch (e) {
      console.error('[channelStore] loadPlatforms failed', e)
    }
  },

  loadPlatform: async (platform) => {
    try {
      const platformState = await channelGetPlatform(platform)
      get().setPlatformState(platformState)
      return platformState
    } catch (e) {
      console.error('[channelStore] loadPlatform failed', e)
      return null
    }
  },

  beginRegistration: (platform) => channelBeginRegistration(platform),

  pollRegistration: async (platform, deviceCode) => {
    const result = await channelPollRegistration(platform, deviceCode)
    if (result.platformState) {
      get().setPlatformState(result.platformState)
    }
    return result
  },

  setEnabled: async (platform, enabled) => {
    const platformState = await channelSetEnabled(platform, enabled)
    get().setPlatformState(platformState)
    return platformState
  },

  removePlatform: async (platform) => {
    const platformState = await channelRemovePlatform(platform)
    get().setPlatformState(platformState)

    const route = useUiStore.getState().route
    const activeId = route.kind === 'channel' ? route.sessionId ?? null : null
    const willRemoveActive = activeId
      ? get().conversations.some((c) => c.platform === platform && c.sessionId === activeId)
      : false

    set((s) => ({
      conversations: s.conversations.filter((c) => c.platform !== platform),
    }))

    if (willRemoveActive) {
      useUiStore.getState().setRoute({ kind: 'channel' })
    }
    return platformState
  },

  revealSecret: (platform) => channelRevealSecret(platform),

  sendDingtalkGreeting: () => channelSendDingtalkGreeting(),

  loadConversations: async (platform) => {
    try {
      const convs = await channelGetConversations(platform)
      set({ conversations: convs })
    } catch (e) {
      console.error('[channelStore] loadConversations failed', e)
    }
  },
}))

let listenersInitialized = false

/** App 启动时调用一次，订阅后端事件并拉取初始状态 */
export async function initChannelListeners() {
  if (listenersInitialized) {
    await useChannelStore.getState().loadPlatforms()
    await useChannelStore.getState().loadConversations()
    return
  }
  listenersInitialized = true

  await onChannelPlatformState(({ state }) => {
    useChannelStore.getState().setPlatformState(state)
    // refresh_active_robot_flags 改了 is_active_robot 但没单独的 conversations 事件，
    // 所以这里要主动拉一次新快照（remove / reconnect / 切换机器人都走这条）。
    void useChannelStore.getState().loadConversations()
  })

  // 注册 listener 之后再 loadPlatforms — 否则如果 app 启动期间后端已经
  // emit 过 Connected（钉钉/飞书 ws handshake 比前端 mount 还快），那条
  // 事件会丢，UI 永远停在 Connecting。先注册再 load 保证不漏。
  await useChannelStore.getState().loadPlatforms()
  await useChannelStore.getState().loadConversations()
  await onChannelMessage(({ sessionId }) => {
    const { conversations } = useChannelStore.getState()
    const isKnownSession = conversations.some((c) => c.sessionId === sessionId)
    if (!isKnownSession) {
      // 新对话首次出现 (后端 stream worker 刚 push 进内存)，拉一次完整快照
      void useChannelStore.getState().loadConversations()
      return
    }
    const route = useUiStore.getState().route
    const activeId = route.kind === 'channel' ? route.sessionId ?? null : null
    if (sessionId !== activeId) {
      useChannelStore.getState().incrementUnread(sessionId)
    }
  })
}
