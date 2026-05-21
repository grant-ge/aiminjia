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
import { useNotificationStore } from '@/stores/notificationStore'
import { useUiStore, type Route } from '@/stores/uiStore'

/**
 * Map a backend error string into a Chinese, user-facing message.
 * Falls back to the raw error when no specific pattern matches.
 *
 * Background: post-login model fetch can return 402 "Budget exceeded"
 * (admin used up the org's monthly cap), 403 (suspended account), 5xx, etc.
 * Showing the raw English JSON in a toast scares users; the login itself
 * should still succeed so the user can see their own settings page.
 */
function translatePostLoginError(raw: string): { title: string; message: string } {
  const lower = raw.toLowerCase()
  if (lower.includes('budget exceeded') || lower.includes('402')) {
    return {
      title: '本月额度已用完',
      message: '当前组织本月可用额度已耗尽,无法获取模型列表。请联系管理员充值或调整预算后重新登录。',
    }
  }
  if (lower.includes('payment required')) {
    return {
      title: '账户余额不足',
      message: '当前组织余额不足,无法获取模型列表。请联系管理员充值后重新登录。',
    }
  }
  if (lower.includes('suspended') || lower.includes('403')) {
    return {
      title: '账户被冻结',
      message: '当前账户或组织已被管理员冻结。请联系管理员处理后再登录。',
    }
  }
  if (lower.includes('timeout') || lower.includes('timed out') || lower.includes('network')) {
    return {
      title: '网络异常',
      message: '获取模型列表超时,稍后重试或检查网络连接。当前可正常使用其它功能。',
    }
  }
  return {
    title: '获取模型列表失败',
    message: '登录已成功,但获取可用模型列表失败,部分功能可能受限。可在设置中重试。',
  }
}

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
      // Restoring from disk must not log the user out just because the
      // model-list endpoint is currently 402/5xx. Surface a toast and
      // keep the session.
      let models: CloudModel[] = []
      let restoreError: string | null = null
      try {
        models = await getCloudModels()
      } catch (err) {
        restoreError = err instanceof Error ? err.message : String(err)
      }
      set({ ...mapAuthState(info, models), isAuthPending: false })
      if (restoreError) {
        const { title, message } = translatePostLoginError(restoreError)
        useNotificationStore.getState().push({
          level: 'warning',
          title,
          message,
          actions: [],
          dismissible: true,
          context: 'toast',
          autoHide: 10,
        })
      }
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
      // Post-login model fetch is non-fatal — a 402 / 403 / network blip
      // shouldn't block the user from logging in and seeing settings /
      // billing. Translate the error to Chinese and surface as a toast
      // after the auth state is committed.
      let models = info.models
      let postLoginError: string | null = null
      if (models.length === 0) {
        try {
          models = await getCloudModels()
        } catch (err) {
          postLoginError = err instanceof Error ? err.message : String(err)
          models = []
        }
      }
      set({ ...mapAuthState(info, models), isAuthPending: false })
      if (!useAuthStore.getState().redirectFrom) {
        useUiStore.getState().setRoute({ kind: 'home' })
      }
      if (postLoginError) {
        const { title, message } = translatePostLoginError(postLoginError)
        useNotificationStore.getState().push({
          level: 'warning',
          title,
          message,
          actions: [],
          dismissible: true,
          context: 'toast',
          autoHide: 10,
        })
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
      // Re-apply the persisted brand so the login page stays on the user's
      // custom tenant skin instead of flashing back to defaults after logout.
      // No-op when no cache exists.
      void useBrandingStore.getState().restoreFromDisk()
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
