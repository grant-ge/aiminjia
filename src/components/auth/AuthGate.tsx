import { type PropsWithChildren, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useChat } from '@/hooks/useChat'
import { useUiStore } from '@/stores/uiStore'
import { useSkillStore } from '@/stores/skillStore'
import { syncBuiltinSkills, TAURI_EVENTS, workplaceDirectoryCatalog } from '@/lib/tauri'
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

  // 监听后端 refresh_skill_registry 广播，自动刷新 skillStore。
  // 触发源包括：install_custom_skill / import_skill_package / RefreshSkills RuntimeTool /
  // load_skill miss-retry / refresh_skill_registry_cmd —— 所有路径共用一个事件，
  // 保证 SkillPopover picker / 技能中心 / 派活 banner 等任何依赖 skillStore 的位置
  // 在 AI 装完技能后立即看到新技能，无需重启应用或重开对话。
  useEffect(() => {
    let unlisten: (() => void) | null = null
    let cancelled = false
    void (async () => {
      try {
        const handle = await listen(TAURI_EVENTS.SKILL_REGISTRY_REFRESHED, () => {
          void useSkillStore.getState().reload().catch((err) => {
            console.warn('[skill-registry-refreshed] skillStore reload failed:', err)
          })
        })
        if (cancelled) handle()
        else unlisten = handle
      } catch (err) {
        console.warn('[skill-registry-refreshed] listen failed:', err)
      }
    })()
    return () => {
      cancelled = true
      if (unlisten) unlisten()
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
