import { create } from 'zustand'

export const DEV_SETTINGS_STORAGE_KEY = 'aijia-dev-settings'

interface DevSettingsSnapshot {
  showToolErrorIcon: boolean
}

interface DevSettingsState extends DevSettingsSnapshot {
  setShowToolErrorIcon: (show: boolean) => void
}

const DEFAULT_DEV_SETTINGS: DevSettingsSnapshot = {
  showToolErrorIcon: false,
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
    persistDevSettings({ showToolErrorIcon })
    set({ showToolErrorIcon })
  },
}))
