import { create } from 'zustand'

import type { AuthorizedWorkspaceRef } from '@/lib/tauri'

const STORAGE_KEY = 'aijia-home-workspace'

interface HomeState {
  selectedWorkspace: AuthorizedWorkspaceRef | null
  setSelectedWorkspace: (ws: AuthorizedWorkspaceRef | null) => void
}

function loadFromStorage(): AuthorizedWorkspaceRef | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    return JSON.parse(raw) as AuthorizedWorkspaceRef
  } catch {
    return null
  }
}

export const useHomeStore = create<HomeState>()((set) => ({
  selectedWorkspace: loadFromStorage(),
  setSelectedWorkspace: (ws) => {
    try {
      if (ws) {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(ws))
      } else {
        localStorage.removeItem(STORAGE_KEY)
      }
    } catch {
      // ignore storage errors
    }
    set({ selectedWorkspace: ws })
  },
}))
