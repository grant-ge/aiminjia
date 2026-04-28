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

export const useUiStore = create<UiState>((set, get) => ({
  route: { kind: 'home' },
  settingsModal: null,
  prefillText: null,
  setRoute: (route) => set({ route }),
  openSettings: (key) => {
    const normalized: SettingsModalKey =
      (key as string) === 'general' ? 'permissions' : (key as SettingsModalKey)
    set({ settingsModal: normalized })
  },
  closeSettings: () => set({ settingsModal: null }),
  setPrefillText: (text) => set({ prefillText: text }),
  consumePrefillText: () => {
    const text = get().prefillText
    if (text !== null) set({ prefillText: null })
    return text
  },
}))
