import { create } from 'zustand'

export type Route =
  | { kind: 'home' }
  | { kind: 'employees' }
  | { kind: 'skill-center' }
  | { kind: 'skill-detail'; skillId: string }
  | { kind: 'schedules' }
  | { kind: 'inbox' }
  | { kind: 'expert-teams' }
  | { kind: 'chat'; conversationId: string }
  | { kind: 'channel'; sessionId?: string }

export type SettingsModalKey =
  | 'account'
  | 'account-billing'
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

export type SidebarBodyTab = 'project' | 'employee' | 'expert-team' | 'channel'

export interface PendingSkillSelection {
  id: string
  label: string
  trigger: string
}

interface UiState {
  route: Route
  settingsModal: SettingsModalState
  sidebarTab: SidebarBodyTab
  sidebarHidden: boolean
  prefillText: string | null
  pendingSkill: PendingSkillSelection | null
  setRoute: (route: Route) => void
  openSettings: (settingsModal: SettingsModalKey) => void
  closeSettings: () => void
  setSidebarTab: (tab: SidebarBodyTab) => void
  setSidebarHidden: (hidden: boolean) => void
  toggleSidebarHidden: () => void
  setPrefillText: (text: string) => void
  consumePrefillText: () => string | null
  setPendingSkill: (skill: PendingSkillSelection) => void
  consumePendingSkill: () => PendingSkillSelection | null
}

const ROUTE_STORAGE_KEY = 'aijia-ui-route'
const SIDEBAR_TAB_STORAGE_KEY = 'aijia-sidebar-tab'
const SIDEBAR_HIDDEN_STORAGE_KEY = 'aijia-sidebar-hidden'
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
    case 'expert-teams':
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

function loadPersistedSidebarTab(): SidebarBodyTab {
  if (typeof localStorage === 'undefined') return 'project'
  try {
    const raw = localStorage.getItem(SIDEBAR_TAB_STORAGE_KEY)
    return raw === 'channel' || raw === 'expert-team' || raw === 'employee' ? raw : 'project'
  } catch {
    return 'project'
  }
}

function persistSidebarTab(tab: SidebarBodyTab) {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(SIDEBAR_TAB_STORAGE_KEY, tab)
  } catch {
    // Ignore storage failures; tab still works in memory.
  }
}

function loadPersistedSidebarHidden(): boolean {
  if (typeof localStorage === 'undefined') return false
  try {
    return localStorage.getItem(SIDEBAR_HIDDEN_STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

function persistSidebarHidden(hidden: boolean) {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(SIDEBAR_HIDDEN_STORAGE_KEY, hidden ? 'true' : 'false')
  } catch {
    // Ignore storage failures; visibility still works in memory.
  }
}

export const useUiStore = create<UiState>((set, get) => ({
  route: loadPersistedRoute(),
  settingsModal: null,
  sidebarTab: loadPersistedSidebarTab(),
  sidebarHidden: loadPersistedSidebarHidden(),
  prefillText: null,
  pendingSkill: null,
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
  setSidebarTab: (tab) => {
    persistSidebarTab(tab)
    set({ sidebarTab: tab })
  },
  setSidebarHidden: (hidden) => {
    persistSidebarHidden(hidden)
    set({ sidebarHidden: hidden })
  },
  toggleSidebarHidden: () => {
    const hidden = !get().sidebarHidden
    persistSidebarHidden(hidden)
    set({ sidebarHidden: hidden })
  },
  setPrefillText: (text) => set({ prefillText: text }),
  consumePrefillText: () => {
    const text = get().prefillText
    if (text !== null) set({ prefillText: null })
    return text
  },
  setPendingSkill: (skill) => set({ pendingSkill: skill }),
  consumePendingSkill: () => {
    const skill = get().pendingSkill
    if (skill !== null) set({ pendingSkill: null })
    return skill
  },
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
