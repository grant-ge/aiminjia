import { create } from 'zustand'

import type { AuthorizedWorkspaceRef } from '@/lib/tauri'

const STORAGE_KEY = 'aijia-home-workspace'
const RECENT_STORAGE_KEY = 'aijia-home-recent-workspaces'

interface HomeState {
  selectedWorkspace: AuthorizedWorkspaceRef | null
  recentWorkspaces: AuthorizedWorkspaceRef[]
  setSelectedWorkspace: (ws: AuthorizedWorkspaceRef | null) => void
  removeRecentWorkspace: (rootPath: string) => void
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return fallback
    return JSON.parse(raw) as T
  } catch {
    return fallback
  }
}

function writeJson<T>(key: string, value: T | null) {
  try {
    if (value == null) {
      localStorage.removeItem(key)
    } else {
      localStorage.setItem(key, JSON.stringify(value))
    }
  } catch {
    // ignore storage errors
  }
}

function loadFromStorage(): AuthorizedWorkspaceRef | null {
  return readJson<AuthorizedWorkspaceRef | null>(STORAGE_KEY, null)
}

function loadRecentFromStorage(): AuthorizedWorkspaceRef[] {
  const recent = readJson<AuthorizedWorkspaceRef[]>(RECENT_STORAGE_KEY, [])
  return Array.isArray(recent) ? recent : []
}

function withWorkspaceFirst(
  recent: AuthorizedWorkspaceRef[],
  workspace: AuthorizedWorkspaceRef,
): AuthorizedWorkspaceRef[] {
  return [
    workspace,
    ...recent.filter((item) => item.rootPath !== workspace.rootPath),
  ]
}

export const useHomeStore = create<HomeState>((set, get) => ({
  selectedWorkspace: loadFromStorage(),
  recentWorkspaces: loadRecentFromStorage(),
  setSelectedWorkspace: (ws) => {
    writeJson(STORAGE_KEY, ws)

    if (!ws) {
      set({ selectedWorkspace: null })
      return
    }

    const recentWorkspaces = withWorkspaceFirst(get().recentWorkspaces, ws)
    writeJson(RECENT_STORAGE_KEY, recentWorkspaces)
    set({ selectedWorkspace: ws, recentWorkspaces })
  },
  removeRecentWorkspace: (rootPath) => {
    const recentWorkspaces = get().recentWorkspaces.filter((ws) => ws.rootPath !== rootPath)
    writeJson(RECENT_STORAGE_KEY, recentWorkspaces)
    set({ recentWorkspaces })
  },
}))
