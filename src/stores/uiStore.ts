import { create } from 'zustand'

export type Route =
  | { kind: 'home' }
  | { kind: 'employees' }
  | { kind: 'skill-center' }
  | { kind: 'skill-detail'; skillId: string }
  | { kind: 'schedules' }
  | { kind: 'inbox' }
  | { kind: 'chat'; conversationId: string }
  | { kind: 'channel'; sessionId?: string }

export type SettingsModalKey =
  | 'account'
  | 'usage'
  | 'permissions'
  | 'mcp'
  | 'sso'
  | 'shortcuts'
  | 'archived'
  | 'runtime'
  | 'about'

export type SettingsModalState = null | SettingsModalKey

const DISABLED_SETTINGS_KEYS = new Set<SettingsModalKey>([
  'usage',
  'permissions',
  'mcp',
  'sso',
  'shortcuts',
])

interface UiState {
  route: Route
  settingsModal: SettingsModalState
  prefillText: string | null
  teamDrawerOpen: boolean
  teamDrawerExpanded: boolean
  setRoute: (route: Route) => void
  openSettings: (settingsModal: SettingsModalKey) => void
  closeSettings: () => void
  setPrefillText: (text: string) => void
  consumePrefillText: () => string | null
  openTeamDrawer: () => void
  closeTeamDrawer: () => void
  toggleTeamDrawer: () => void
  setTeamDrawerExpanded: (expanded: boolean) => void
}

const ROUTE_STORAGE_KEY = 'aijia-ui-route'
const DEFAULT_ROUTE: Route = { kind: 'home' }

function isRoute(value: unknown): value is Route {
  if (!value || typeof value !== 'object') return false
  const route = value as Partial<Route>
  switch (route.kind) {
    case 'home':
    case 'employees':
    case 'skill-center':
    case 'schedules':
    case 'inbox':
      return true
    case 'skill-detail':
      return typeof route.skillId === 'string' && route.skillId.length > 0
    case 'chat':
      return typeof route.conversationId === 'string' && route.conversationId.length > 0
    case 'channel':
      return true
    default:
      return false
  }
}

function loadPersistedRoute(): Route {
  if (typeof localStorage === 'undefined') return DEFAULT_ROUTE
  try {
    const raw = localStorage.getItem(ROUTE_STORAGE_KEY)
    if (!raw) return DEFAULT_ROUTE
    const parsed = JSON.parse(raw)
    return isRoute(parsed) ? parsed : DEFAULT_ROUTE
  } catch {
    return DEFAULT_ROUTE
  }
}

function persistRoute(route: Route) {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(ROUTE_STORAGE_KEY, JSON.stringify(route))
  } catch {
    // Ignore storage failures; routing should keep working in memory.
  }
}

export const useUiStore = create<UiState>((set, get) => ({
  route: loadPersistedRoute(),
  settingsModal: null,
  prefillText: null,
  teamDrawerOpen: false,
  teamDrawerExpanded: false,
  setRoute: (route) => {
    // 切对话时关掉群聊抽屉——保持每个 conversation 的视图独立。
    const prevRoute = get().route
    const switchingConversation =
      route.kind === 'chat' &&
      prevRoute.kind === 'chat' &&
      prevRoute.conversationId !== route.conversationId
    persistRoute(route)
    set({
      route,
      ...(switchingConversation || route.kind !== 'chat'
        ? { teamDrawerOpen: false, teamDrawerExpanded: false }
        : {}),
    })
  },
  openSettings: (key) => {
    const normalized: SettingsModalKey =
      (key as string) === 'general' ? 'permissions' : (key as SettingsModalKey)
    set({ settingsModal: DISABLED_SETTINGS_KEYS.has(normalized) ? 'account' : normalized })
  },
  closeSettings: () => set({ settingsModal: null }),
  setPrefillText: (text) => set({ prefillText: text }),
  consumePrefillText: () => {
    const text = get().prefillText
    if (text !== null) set({ prefillText: null })
    return text
  },
  openTeamDrawer: () => set({ teamDrawerOpen: true }),
  closeTeamDrawer: () => set({ teamDrawerOpen: false, teamDrawerExpanded: false }),
  toggleTeamDrawer: () => {
    const open = !get().teamDrawerOpen
    set({ teamDrawerOpen: open, teamDrawerExpanded: open ? get().teamDrawerExpanded : false })
  },
  setTeamDrawerExpanded: (expanded) => set({ teamDrawerExpanded: expanded }),
}))

// ---------------------------------------------------------------------------
// Route-derived selectors
// getActive* — non-hook form, safe to call outside React (e.g. Rust IPC handlers,
//              utility functions, tests). Read directly from store state snapshot.
// useActive* — React hook form, subscribes to the slice so the component re-renders
//              when the relevant route field changes.
// ---------------------------------------------------------------------------

export const getActiveConversationId = (): string | null => {
  const r = useUiStore.getState().route
  return r.kind === 'chat' ? r.conversationId : null
}

export const getActiveChannelSessionId = (): string | null => {
  const r = useUiStore.getState().route
  return r.kind === 'channel' ? r.sessionId ?? null : null
}

export const useActiveConversationId = (): string | null =>
  useUiStore((s) => (s.route.kind === 'chat' ? s.route.conversationId : null))

export const useActiveChannelSessionId = (): string | null =>
  useUiStore((s) => (s.route.kind === 'channel' ? s.route.sessionId ?? null : null))
