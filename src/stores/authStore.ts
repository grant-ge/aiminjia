import { create } from 'zustand'

import {
  cloudLogin,
  cloudLogout,
  getCloudAuth,
  getCloudModels,
  type CloudAuthInfo,
  type CloudModel,
} from '@/lib/tauri'
import { useBrandingStore } from '@/stores/brandingStore'
import { useChatStore } from '@/stores/chatStore'
import { useUiStore, type Route } from '@/stores/uiStore'

interface AuthState {
  isLoggedIn: boolean
  user: CloudAuthInfo['user']
  tenant: CloudAuthInfo['tenant']
  cloudModels: CloudModel[]
  selectedCloudModel: string | null
  redirectFrom: Route | null
  isAuthPending: boolean
  setAuth: (info: CloudAuthInfo) => void
  setCloudModels: (models: CloudModel[]) => void
  setSelectedCloudModel: (model: string | null) => void
  setRedirectFrom: (route: Route | null) => void
  resyncCloudModels: () => Promise<string | null>
  restoreFromStorage: () => Promise<void>
  login: (username: string, password: string) => Promise<void>
  logout: () => Promise<void>
  clearAndRedirect: (route?: Route) => void
  clearAuth: () => void
}

const EMPTY_AUTH_STATE = {
  isLoggedIn: false,
  user: null,
  tenant: null,
  cloudModels: [] as CloudModel[],
  selectedCloudModel: null,
}

function mapAuthState(info: CloudAuthInfo, models: CloudModel[]) {
  const selectedCloudModel = models[0]?.id ?? info.models[0]?.id ?? null
  return {
    isLoggedIn: info.loggedIn,
    user: info.user,
    tenant: info.tenant,
    cloudModels: models,
    selectedCloudModel,
  }
}

async function applyTenantBranding(info: CloudAuthInfo): Promise<void> {
  const tenant = info.tenant
  if (!tenant) return
  useBrandingStore.getState().applyBranding(tenant)
}

// Note: there used to be a `syncCloudModelSelection` here that wrote
// `models[0].id` back to settings.cloudModel. That was the source of the
// "user pinned to whichever model they happened to start with" bug —
// removed 2026-05 along with Step 2 (gateway decides routing). cloudModels
// are now informational only; selectedCloudModel state stays in memory for
// any UI angle still keyed on it but is never persisted.

export const useAuthStore = create<AuthState>((set) => ({
  ...EMPTY_AUTH_STATE,
  redirectFrom: null,
  isAuthPending: true,

  setAuth: (info) =>
    set({
      isLoggedIn: info.loggedIn,
      user: info.user,
      tenant: info.tenant,
      cloudModels: info.models,
      selectedCloudModel: info.models[0]?.id ?? null,
    }),

  setCloudModels: (models) =>
    set((state) => ({
      cloudModels: models,
      selectedCloudModel: models.find((model) => model.id === state.selectedCloudModel)?.id ?? models[0]?.id ?? null,
    })),

  setSelectedCloudModel: (selectedCloudModel) => set({ selectedCloudModel }),
  setRedirectFrom: (redirectFrom) => set({ redirectFrom }),

  async resyncCloudModels() {
    if (!useAuthStore.getState().isLoggedIn) return null
    const models = await getCloudModels()
    const selectedCloudModel = models[0]?.id ?? null
    set({ cloudModels: models, selectedCloudModel })
    return selectedCloudModel
  },

  async restoreFromStorage() {
    set({ isAuthPending: true })
    try {
      const info = await getCloudAuth()
      if (!info.loggedIn) {
        set({ ...EMPTY_AUTH_STATE, isAuthPending: false })
        return
      }

      await applyTenantBranding(info)
      const models = await getCloudModels()
      set({ ...mapAuthState(info, models), isAuthPending: false })
    } catch (error) {
      useBrandingStore.getState().reset()
      set({ ...EMPTY_AUTH_STATE, isAuthPending: false })
      throw error
    }
  },

  async login(username, password) {
    set({ isAuthPending: true })
    useChatStore.getState().resetAll()
    useChatStore.getState().resetStreaming()
    try {
      const info = await cloudLogin(username.trim(), password)
      await applyTenantBranding(info)
      const models = info.models.length > 0 ? info.models : await getCloudModels()
      set({ ...mapAuthState(info, models), isAuthPending: false })
      if (!useAuthStore.getState().redirectFrom) {
        useUiStore.getState().setRoute({ kind: 'home' })
      }
    } catch (error) {
      useBrandingStore.getState().reset()
      set({ isAuthPending: false })
      throw error
    }
  },

  async logout() {
    set({ isAuthPending: true })
    try {
      await cloudLogout()
      useChatStore.getState().resetAll()
      useChatStore.getState().resetStreaming()
      useBrandingStore.getState().reset()
      set({ ...EMPTY_AUTH_STATE, redirectFrom: null, isAuthPending: false })
    } catch (error) {
      set({ isAuthPending: false })
      throw error
    }
  },

  clearAndRedirect(route) {
    set({
      ...EMPTY_AUTH_STATE,
      redirectFrom: route ?? null,
      isAuthPending: false,
    })
  },

  clearAuth() {
    set({ ...EMPTY_AUTH_STATE, isAuthPending: false })
  },
}))
