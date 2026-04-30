import { type PropsWithChildren, useEffect, useRef } from 'react'

import { useAuthStore } from '@/stores/authStore'
import { useChat } from '@/hooks/useChat'
import { useUiStore } from '@/stores/uiStore'
import { useSkillStore } from '@/stores/skillStore'
import { syncBuiltinSkills } from '@/lib/tauri'

import { FullscreenLoader } from './FullscreenLoader'
import { LoginPage } from './LoginPage'

export function AuthGate({ children }: PropsWithChildren) {
  const isLoggedIn = useAuthStore((state) => state.isLoggedIn)
  const isAuthPending = useAuthStore((state) => state.isAuthPending)
  const redirectFrom = useAuthStore((state) => state.redirectFrom)
  const restoreFromStorage = useAuthStore((state) => state.restoreFromStorage)
  const setRoute = useUiStore((state) => state.setRoute)
  const { loadConversations } = useChat()
  const hasRestored = useRef(false)

  useEffect(() => {
    if (hasRestored.current) {
      return
    }
    hasRestored.current = true
    void restoreFromStorage()
  }, [restoreFromStorage])

  // Load conversation history once the user is authenticated
  useEffect(() => {
    if (isLoggedIn) {
      void loadConversations()

      syncBuiltinSkills()
        .then((result) => {
          if (result.installed.length > 0) {
            console.info('[builtin-skills] installed:', result.installed)
            void useSkillStore.getState().reload()
          }
          if (result.skipped.length > 0) {
            console.info('[builtin-skills] skipped:', result.skipped)
          }
        })
        .catch((err) => {
          console.warn('[builtin-skills] sync failed:', err)
        })
    }
  }, [isLoggedIn, loadConversations])

  useEffect(() => {
    if (isLoggedIn && redirectFrom) {
      setRoute(redirectFrom)
      useAuthStore.getState().setRedirectFrom(null)
    }
  }, [isLoggedIn, redirectFrom, setRoute])

  if (isAuthPending) {
    return <FullscreenLoader />
  }

  if (!isLoggedIn) {
    return <LoginPage />
  }

  return <>{children}</>
}
