import { useCallback, useEffect, useState } from 'react'
import {
  getAuthorizedWorkspace,
  type AuthorizedWorkspaceRef,
} from '@/lib/tauri'

const AUTHORIZED_WORKSPACE_CHANGED_EVENT = 'aijia:authorized-workspace-changed'

interface AuthorizedWorkspaceChangedDetail {
  sessionId: string
}

export function emitAuthorizedWorkspaceChanged(sessionId: string) {
  if (typeof window === 'undefined') return
  window.dispatchEvent(
    new CustomEvent<AuthorizedWorkspaceChangedDetail>(
      AUTHORIZED_WORKSPACE_CHANGED_EVENT,
      {
        detail: { sessionId },
      },
    ),
  )
}

export function useAuthorizedWorkspace(sessionId: string | null) {
  const [workspace, setWorkspace] = useState<AuthorizedWorkspaceRef | null>(null)
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    if (!sessionId) {
      setWorkspace(null)
      setLoading(false)
      return
    }

    setLoading(true)
    try {
      const current = await getAuthorizedWorkspace(sessionId)
      setWorkspace(current)
    } catch (error) {
      console.error('[useAuthorizedWorkspace] Failed to load authorized workspace:', error)
      setWorkspace(null)
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => {
    void refresh()
  }, [refresh])

  useEffect(() => {
    if (!sessionId || typeof window === 'undefined') return

    const handleChanged = (event: Event) => {
      const detail = (event as CustomEvent<AuthorizedWorkspaceChangedDetail>).detail
      if (detail?.sessionId === sessionId) {
        void refresh()
      }
    }

    window.addEventListener(AUTHORIZED_WORKSPACE_CHANGED_EVENT, handleChanged)
    return () => {
      window.removeEventListener(AUTHORIZED_WORKSPACE_CHANGED_EVENT, handleChanged)
    }
  }, [refresh, sessionId])

  return {
    workspace,
    loading,
    refresh,
  }
}
