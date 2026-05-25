import { create } from 'zustand'

import type { AppLanguage } from '@/i18n'
import i18n, { persistLanguage } from '@/i18n'
import {
  applyFontScale,
  loadPersistedFontScale,
  normalizeFontScale,
  persistFontScale,
} from '@/styles/fontScale'
import type { Settings, LlmProvider, FontScale, DataMaskingLevel, ChatWidthMode } from '@/types/settings'
import { DEFAULT_SETTINGS } from '@/types/settings'

interface SettingsState extends Settings {
  isLoaded: boolean
  configuredProviders: LlmProvider[]
  setSettings: (settings: Partial<Settings>) => void
  setPrimaryModel: (model: LlmProvider) => void
  setPrimaryApiKey: (key: string) => void
  setWorkspacePath: (path: string) => void
  setAutoModelRouting: (enabled: boolean) => void
  setCustomModelEndpoint: (endpoint: string) => void
  setCustomModelName: (name: string) => void
  setConfiguredProviders: (providers: LlmProvider[]) => void
  setAppLanguage: (language: AppLanguage) => void
  setFontScale: (scale: FontScale) => void
  setChatWidthMode: (mode: ChatWidthMode) => void
  setDataMaskingLevel: (level: DataMaskingLevel) => void
  markLoaded: () => void
}

export const useSettingsStore = create<SettingsState>((set) => ({
  ...DEFAULT_SETTINGS,
  fontScale: loadPersistedFontScale(),
  isLoaded: false,
  configuredProviders: [],

  setSettings: (settings) => {
    if (settings.fontScale) {
      const fontScale = normalizeFontScale(settings.fontScale)
      persistFontScale(fontScale)
      applyFontScale(fontScale)
    }
    set(settings)
  },
  setPrimaryModel: (primaryModel) => set({ primaryModel }),
  setPrimaryApiKey: (primaryApiKey) => set({ primaryApiKey }),
  setWorkspacePath: (workspacePath) => set({ workspacePath }),
  setAutoModelRouting: (autoModelRouting) => set({ autoModelRouting }),
  setCustomModelEndpoint: (customModelEndpoint) => set({ customModelEndpoint }),
  setCustomModelName: (customModelName) => set({ customModelName }),
  setConfiguredProviders: (configuredProviders) => set({ configuredProviders }),
  setAppLanguage: (appLanguage) => {
    i18n.changeLanguage(appLanguage)
    persistLanguage(appLanguage)
    set({ appLanguage })
  },
  setFontScale: (fontScale) => {
    const normalized = normalizeFontScale(fontScale)
    persistFontScale(normalized)
    applyFontScale(normalized)
    set({ fontScale: normalized })
  },
  setChatWidthMode: (chatWidthMode) => set({ chatWidthMode }),
  setDataMaskingLevel: (dataMaskingLevel) => set({ dataMaskingLevel }),
  markLoaded: () => set({ isLoaded: true }),
}))
