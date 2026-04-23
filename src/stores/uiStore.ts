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
  setRoute: (route: Route) => void
  openSettings: (settingsModal: SettingsModalKey) => void
  closeSettings: () => void
}

export const useUiStore = create<UiState>((set) => ({
  route: { kind: 'home' },
  settingsModal: null,
  setRoute: (route) => set({ route }),
  openSettings: (key) => {
    const normalized: SettingsModalKey =
      (key as string) === 'general' ? 'permissions' : (key as SettingsModalKey)
    set({ settingsModal: normalized })
  },
  closeSettings: () => set({ settingsModal: null }),
}))
