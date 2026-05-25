import { create } from 'zustand'

import type { AuthorizedWorkspaceRef } from '@/lib/tauri'
import { updateSettings, getSettings } from '@/lib/tauri'
import type { Settings } from '@/types/settings'

const MAX_RECENT_WORKSPACES = 10

interface HomeState {
  selectedWorkspace: AuthorizedWorkspaceRef | null
  recentWorkspaces: AuthorizedWorkspaceRef[]
  setSelectedWorkspace: (ws: AuthorizedWorkspaceRef | null) => void
  removeRecentWorkspace: (rootPath: string) => void
}

function tryParse<T>(raw: string | undefined, fallback: T): T {
  if (!raw) return fallback
  try {
    return JSON.parse(raw) as T
  } catch {
    return fallback
  }
}

/**
 * Persist current home-page UI workspace state into AppSettings.
 *
 * Fire-and-forget: failures are logged but do not interrupt the user. On next
 * write attempt we'll try again.
 */
async function persist(
  selectedWorkspace: AuthorizedWorkspaceRef | null,
  recent: AuthorizedWorkspaceRef[],
) {
  try {
    const current = await getSettings()
    await updateSettings({
      ...current,
      uiHomeSelectedWorkspace: selectedWorkspace ? JSON.stringify(selectedWorkspace) : '',
      uiHomeRecentWorkspaces: recent.length ? JSON.stringify(recent) : '',
    })
  } catch (err) {
    console.warn('[homeStore] persist failed:', err)
  }
}

function withWorkspaceFirst(
  recent: AuthorizedWorkspaceRef[],
  workspace: AuthorizedWorkspaceRef,
): AuthorizedWorkspaceRef[] {
  const next = [
    workspace,
    ...recent.filter((item) => item.rootPath !== workspace.rootPath),
  ]
  return next.slice(0, MAX_RECENT_WORKSPACES)
}

export const useHomeStore = create<HomeState>((set, get) => ({
  selectedWorkspace: null,
  recentWorkspaces: [],
  setSelectedWorkspace: (ws) => {
    if (!ws) {
      void persist(null, get().recentWorkspaces)
      set({ selectedWorkspace: null })
      return
    }
    const recentWorkspaces = withWorkspaceFirst(get().recentWorkspaces, ws)
    void persist(ws, recentWorkspaces)
    set({ selectedWorkspace: ws, recentWorkspaces })
  },
  removeRecentWorkspace: (rootPath) => {
    const recentWorkspaces = get().recentWorkspaces.filter((ws) => ws.rootPath !== rootPath)
    void persist(get().selectedWorkspace, recentWorkspaces)
    set({ recentWorkspaces })
  },
}))

/**
 * Hydrate the store from AppSettings on app startup. Called from App.tsx.
 *
 * Parses the two JSON-stringified fields and applies a 10-item LRU cap to recent.
 */
export function hydrateHomeStore(settings: Settings) {
  const selected = tryParse<AuthorizedWorkspaceRef | null>(
    settings.uiHomeSelectedWorkspace,
    null,
  )
  const recent = tryParse<AuthorizedWorkspaceRef[]>(
    settings.uiHomeRecentWorkspaces,
    [],
  )
  useHomeStore.setState({
    selectedWorkspace: selected,
    recentWorkspaces: Array.isArray(recent)
      ? recent.slice(0, MAX_RECENT_WORKSPACES)
      : [],
  })
}
