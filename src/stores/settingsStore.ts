/**
 * Settings store — model config, workspace, and preferences.
 * Based on tech-architecture.md §3.2
 */
import { create } from 'zustand'
import type { Settings, LlmProvider } from '@/types/settings'
import { DEFAULT_SETTINGS } from '@/types/settings'
import type { AppLanguage } from '@/i18n'
import i18n, { persistLanguage } from '@/i18n'

interface SettingsState extends Settings {
  // Whether settings have been loaded from backend
  isLoaded: boolean

  // Providers that have a saved API key
  configuredProviders: LlmProvider[]

  // Actions
  setSettings: (settings: Partial<Settings>) => void
  setPrimaryModel: (model: LlmProvider) => void
  setPrimaryApiKey: (key: string) => void
  setWorkspacePath: (path: string) => void
  setAutoModelRouting: (enabled: boolean) => void
  setTavilyApiKey: (key: string) => void
  setBochaApiKey: (key: string) => void
  setCustomModelEndpoint: (endpoint: string) => void
  setCustomModelName: (name: string) => void
  setConfiguredProviders: (providers: LlmProvider[]) => void
  setAppLanguage: (language: AppLanguage) => void
  markLoaded: () => void
}

export const useSettingsStore = create<SettingsState>((set) => ({
  ...DEFAULT_SETTINGS,
  isLoaded: false,
  configuredProviders: [],

  setSettings: (settings) => set(settings),

  setPrimaryModel: (primaryModel) => set({ primaryModel }),

  setPrimaryApiKey: (primaryApiKey) => set({ primaryApiKey }),

  setWorkspacePath: (workspacePath) => set({ workspacePath }),

  setAutoModelRouting: (autoModelRouting) => set({ autoModelRouting }),

  setTavilyApiKey: (tavilyApiKey) => set({ tavilyApiKey }),

  setBochaApiKey: (bochaApiKey) => set({ bochaApiKey }),

  setCustomModelEndpoint: (customModelEndpoint) => set({ customModelEndpoint }),

  setCustomModelName: (customModelName) => set({ customModelName }),

  setConfiguredProviders: (configuredProviders) => set({ configuredProviders }),

  setAppLanguage: (appLanguage) => {
    i18n.changeLanguage(appLanguage)
    persistLanguage(appLanguage)
    set({ appLanguage })
  },

  markLoaded: () => set({ isLoaded: true }),
}))
