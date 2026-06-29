import { type PropsWithChildren, useEffect, useRef, useState } from 'react'

import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useChat } from '@/hooks/useChat'
import { useUiStore, type Route } from '@/stores/uiStore'
import { useSkillStore } from '@/stores/skillStore'
import { onSkillEnablementChanged, onSkillRegistryRefreshed, syncBuiltinSkills, workplaceDirectoryCatalog } from '@/lib/tauri'
import i18n from '@/i18n'

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
  const [isRestoringAuth, setIsRestoringAuth] = useState(true)

  useEffect(() => {
    if (hasRestored.current) {
      return
    }
    hasRestored.current = true
    const devRoute = getDevForcedRoute()
    if (devRoute) {
      useAuthStore.getState().setAuth({
        loggedIn: true,
        user: { id: 0, name: 'Dev User', username: 'dev' },
        tenant: {
          id: 0,
          name: 'AIjia Dev',
          balance: '0',
          productName: 'AI小家',
        },
        models: [],
      })
      setRoute(devRoute)
      queueMicrotask(() => setIsRestoringAuth(false))
      return
    }
    // Apply cached brand first so the login page shows the previous tenant's
    // logo / colors / product name even when auth restore turns up empty
    // (logged out). authStore.restoreFromStorage will override with fresh
    // tenant info if a session is still valid.
    void useBrandingStore.getState().restoreFromDisk()
    void restoreFromStorage()
      .catch((err) => {
        console.warn('[auth] restore from storage failed:', err)
      })
      .finally(() => {
        setIsRestoringAuth(false)
      })
  }, [restoreFromStorage, setRoute])

  // Load conversation history once the user is authenticated
  useEffect(() => {
    if (isLoggedIn) {
      void loadConversations()

      syncBuiltinSkills()
        .then((result) => {
          if (result.installed.length > 0) {
            console.info('[builtin-skills] installed:', result.installed)
          }
          if (result.skipped.length > 0) {
            console.info('[builtin-skills] skipped:', result.skipped)
          }
        })
        .catch((err) => {
          console.warn('[builtin-skills] sync failed:', err)
        })
        .finally(() => {
          void useSkillStore.getState().reload().catch((err) => {
            console.warn('[builtin-skills] skillStore reload failed:', err)
          })
        })

      workplaceDirectoryCatalog(i18n.language)
        .then((directory) => {
          console.info('[workplace-directory] synced:', {
            categories: directory.categories.length,
            items: directory.items.length,
          })
        })
        .catch((err) => {
          console.warn('[workplace-directory] sync failed:', err)
        })
    }
  }, [isLoggedIn, loadConversations])

  useEffect(() => {
    if (isLoggedIn && redirectFrom) {
      setRoute(redirectFrom)
      useAuthStore.getState().setRedirectFrom(null)
    }
  }, [isLoggedIn, redirectFrom, setRoute])

  // 监听后端 skill registry / enabled 状态广播，自动刷新 skillStore。
  // 触发源包括：install_custom_skill / import_skill_package / RefreshSkills RuntimeTool /
  // load_skill miss-retry / refresh_skill_registry_cmd —— 所有路径共用一个事件，
  // enablement 事件则用于关闭/开启技能后同步 picker / slash / 模型 skill catalog。
  useEffect(() => {
    let unlistenAll: Array<() => void> = []
    let cancelled = false
    const reloadSkills = (source: string) => {
      void useSkillStore.getState().reload().catch((err) => {
        console.warn(`[${source}] skillStore reload failed:`, err)
      })
    }
    void (async () => {
      try {
        const handles = await Promise.all([
          onSkillRegistryRefreshed(() => reloadSkills('skill-registry-refreshed')),
          onSkillEnablementChanged(() => reloadSkills('skill-enablement-changed')),
        ])
        if (cancelled) handles.forEach((handle) => handle())
        else unlistenAll = handles
      } catch (err) {
        console.warn('[skill-events] listen failed:', err)
      }
    })()
    return () => {
      cancelled = true
      unlistenAll.forEach((handle) => handle())
    }
  }, [])

  if (isRestoringAuth && isAuthPending) {
    return <FullscreenLoader />
  }

  if (!isLoggedIn) {
    return <LoginPage />
  }

  return <>{children}</>
}

function getDevForcedRoute(): Route | null {
  if (!import.meta.env.DEV || typeof window === 'undefined') return null
  const route = new URLSearchParams(window.location.search).get('aijiaDevRoute')
  switch (route) {
    case 'schedules':
      return { kind: 'schedules' }
    case 'home':
      return { kind: 'home' }
    case 'employees':
      return { kind: 'employees' }
    case 'skill-center':
      return { kind: 'skill-center' }
    case 'expert-teams':
      return { kind: 'expert-teams' }
    case 'channel':
      return { kind: 'channel' }
    default:
      return null
  }
}
