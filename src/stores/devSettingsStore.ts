import { create } from 'zustand'

export const DEV_SETTINGS_STORAGE_KEY = 'aijia-dev-settings'

interface DevSettingsSnapshot {
  showToolErrorIcon: boolean
  showRawSkillContent: boolean
}

interface DevSettingsState extends DevSettingsSnapshot {
  setShowToolErrorIcon: (show: boolean) => void
  setShowRawSkillContent: (show: boolean) => void
}

const DEFAULT_DEV_SETTINGS: DevSettingsSnapshot = {
  showToolErrorIcon: false,
  showRawSkillContent: false,
}

function loadDevSettings(): DevSettingsSnapshot {
  if (typeof localStorage === 'undefined') return DEFAULT_DEV_SETTINGS

  try {
    const raw = localStorage.getItem(DEV_SETTINGS_STORAGE_KEY)
    if (!raw) return DEFAULT_DEV_SETTINGS
    const parsed = JSON.parse(raw) as Partial<DevSettingsSnapshot>
    return {
      showToolErrorIcon:
        typeof parsed.showToolErrorIcon === 'boolean'
          ? parsed.showToolErrorIcon
          : DEFAULT_DEV_SETTINGS.showToolErrorIcon,
      showRawSkillContent:
        typeof parsed.showRawSkillContent === 'boolean'
          ? parsed.showRawSkillContent
          : DEFAULT_DEV_SETTINGS.showRawSkillContent,
    }
  } catch {
    return DEFAULT_DEV_SETTINGS
  }
}

function persistDevSettings(snapshot: DevSettingsSnapshot) {
  if (typeof localStorage === 'undefined') return
  localStorage.setItem(DEV_SETTINGS_STORAGE_KEY, JSON.stringify(snapshot))
}

export const useDevSettingsStore = create<DevSettingsState>((set) => ({
  ...loadDevSettings(),
  setShowToolErrorIcon: (showToolErrorIcon) => {
    set((state) => {
      const next = { ...state, showToolErrorIcon }
      persistDevSettings({
        showToolErrorIcon: next.showToolErrorIcon,
        showRawSkillContent: next.showRawSkillContent,
      })
      return { showToolErrorIcon }
    })
  },
  setShowRawSkillContent: (showRawSkillContent) => {
    set((state) => {
      const next = { ...state, showRawSkillContent }
      persistDevSettings({
        showToolErrorIcon: next.showToolErrorIcon,
        showRawSkillContent: next.showRawSkillContent,
      })
      return { showRawSkillContent }
    })
  },
}))
