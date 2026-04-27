import { create } from 'zustand'

import { cloudLogin, cloudLogout, getCloudAuth, getCloudModels, type CloudAuthInfo, type CloudModel } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import type { Route } from '@/stores/uiStore'

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

  async restoreFromStorage() {
    set({ isAuthPending: true })
    try {
      const info = await getCloudAuth()
      if (!info.loggedIn) {
        set({ ...EMPTY_AUTH_STATE, isAuthPending: false })
        return
      }

      const models = await getCloudModels()
      set({ ...mapAuthState(info, models), isAuthPending: false })
    } catch (error) {
      set({ ...EMPTY_AUTH_STATE, isAuthPending: false })
      throw error
    }
  },

  async login(username, password) {
    set({ isAuthPending: true })
    useChatStore.getState().resetAll()
    useChatStore.getState().resetStreaming()
    useChatStore.setState({ selectedSkillCommands: {} })
    try {
      const info = await cloudLogin(username.trim(), password)
      const models = info.models.length > 0 ? info.models : await getCloudModels()
      set({ ...mapAuthState(info, models), isAuthPending: false })
    } catch (error) {
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
      useChatStore.setState({ selectedSkillCommands: {} })
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
