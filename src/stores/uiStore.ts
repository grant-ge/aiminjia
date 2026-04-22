import { create } from 'zustand'

export type Route =
  | { kind: 'home' }
  | { kind: 'skill-center' }
  | { kind: 'skill-detail'; skillId: string }
  | { kind: 'schedules' }
  | { kind: 'chat'; conversationId: string }

export type SettingsModalState = null | 'account' | 'general' | 'about' | 'usage'

interface UiState {
  route: Route
  settingsModal: SettingsModalState
  setRoute: (route: Route) => void
  openSettings: (settingsModal: Exclude<SettingsModalState, null>) => void
  closeSettings: () => void
}

export const useUiStore = create<UiState>((set) => ({
  route: { kind: 'home' },
  settingsModal: null,
  setRoute: (route) => set({ route }),
  openSettings: (settingsModal) => set({ settingsModal }),
  closeSettings: () => set({ settingsModal: null }),
}))
