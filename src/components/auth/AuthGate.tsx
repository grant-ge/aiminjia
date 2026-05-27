import { type PropsWithChildren, useEffect, useRef } from 'react'

import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useChat } from '@/hooks/useChat'
import { useUiStore } from '@/stores/uiStore'
import { useSkillStore } from '@/stores/skillStore'
import i18n from '@/i18n'
import { syncBuiltinSkills, syncDesktopResources } from '@/lib/tauri'

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
    // Apply cached brand first so the login page shows the previous tenant's
    // logo / colors / product name even when auth restore turns up empty
    // (logged out). authStore.restoreFromStorage will override with fresh
    // tenant info if a session is still valid.
    void useBrandingStore.getState().restoreFromDisk()
    void restoreFromStorage()
  }, [restoreFromStorage])

  // Load conversation history once the user is authenticated
  useEffect(() => {
    if (isLoggedIn) {
      void loadConversations()

      syncBuiltinSkills(i18n.language)
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

      syncDesktopResources(i18n.language)
        .catch((err) => {
          console.warn('[desktop-resources] sync failed:', err)
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
