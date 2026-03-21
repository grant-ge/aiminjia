/**
 * Browser store — tracks the state of the embedded browser WebView.
 * Used by BrowserPanel to show URL, title, and loading status.
 */
import { create } from 'zustand'

export interface BrowserState {
  isOpen: boolean
  isLoading: boolean
  url: string
  title: string
  appId: number | null
}

interface BrowserStore extends BrowserState {
  setNavigating: (appId: number, url: string) => void
  setPageReady: (appId: number, url: string, title: string) => void
  setClosed: (appId: number) => void
  reset: () => void
}

const initialState: BrowserState = {
  isOpen: false,
  isLoading: false,
  url: '',
  title: '',
  appId: null,
}

export const useBrowserStore = create<BrowserStore>((set) => ({
  ...initialState,

  setNavigating: (appId, url) =>
    set({ isOpen: true, isLoading: true, url, title: '', appId }),

  setPageReady: (appId, url, title) =>
    set((state) => {
      if (state.appId !== null && state.appId !== appId) return state
      return { isOpen: true, isLoading: false, url, title, appId }
    }),

  setClosed: (appId) =>
    set((state) => {
      // Only close if this panel is tracking the same app (or no app)
      if (state.appId !== null && state.appId !== appId) return state
      // Don't reset if panel is already closed
      if (!state.isOpen) return state
      return initialState
    }),

  reset: () => set(initialState),
}))
