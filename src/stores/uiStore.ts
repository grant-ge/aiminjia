import { create } from 'zustand'

export type Route =
  | { kind: 'home' }
  | { kind: 'skill-center' }
  | { kind: 'skill-detail'; skillId: string }
  | { kind: 'schedules' }
  | { kind: 'chat'; conversationId: string }

export type SettingsModalKey =
  | 'account'
  | 'usage'
  | 'permissions'
  | 'mcp'
  | 'sso'
  | 'shortcuts'
  | 'archived'
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
  setRoute: (route: Route) => void
  openSettings: (settingsModal: SettingsModalKey) => void
  closeSettings: () => void
  setPrefillText: (text: string) => void
  consumePrefillText: () => string | null
}

const ROUTE_STORAGE_KEY = 'aijia-ui-route'
const DEFAULT_ROUTE: Route = { kind: 'home' }

function isRoute(value: unknown): value is Route {
  if (!value || typeof value !== 'object') return false
  const route = value as Partial<Route>
  switch (route.kind) {
    case 'home':
    case 'skill-center':
    case 'schedules':
      return true
    case 'skill-detail':
      return typeof route.skillId === 'string' && route.skillId.length > 0
    case 'chat':
      return typeof route.conversationId === 'string' && route.conversationId.length > 0
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
  setRoute: (route) => {
    persistRoute(route)
    set({ route })
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
}))
