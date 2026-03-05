/**
 * Auth store — cloud authentication state.
 */
import { create } from 'zustand'
import type { CloudAuthInfo, CloudModel } from '@/lib/tauri'

interface AuthState {
  isLoggedIn: boolean
  user: { id: number; name: string; username: string } | null
  tenant: { id: number; name: string; balance: string } | null
  cloudModels: CloudModel[]
  selectedCloudModel: string

  setAuth: (info: CloudAuthInfo) => void
  setCloudModels: (models: CloudModel[]) => void
  setSelectedCloudModel: (model: string) => void
  clearAuth: () => void
}

export const useAuthStore = create<AuthState>((set) => ({
  isLoggedIn: false,
  user: null,
  tenant: null,
  cloudModels: [],
  selectedCloudModel: '',

  setAuth: (info) =>
    set({
      isLoggedIn: info.loggedIn,
      user: info.user,
      tenant: info.tenant,
      cloudModels: info.models,
      selectedCloudModel: info.models.length > 0 ? info.models[0].id : '',
    }),

  setCloudModels: (models) => set({ cloudModels: models }),

  setSelectedCloudModel: (model) => set({ selectedCloudModel: model }),

  clearAuth: () =>
    set({
      isLoggedIn: false,
      user: null,
      tenant: null,
      cloudModels: [],
      selectedCloudModel: '',
    }),
}))
