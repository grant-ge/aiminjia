import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'

import i18n, { type AppLanguage } from '@/i18n'
import { setAppMenuLanguage, TAURI_EVENTS } from '@/lib/tauri'
import { useUiStore } from '@/stores/uiStore'

function normalizeMenuLanguage(language: string): AppLanguage {
  return language === 'en-US' ? 'en-US' : 'zh-CN'
}

export function useAppNavigationMenu() {
  useEffect(() => {
    const syncLanguage = (language: string) => {
      setAppMenuLanguage(normalizeMenuLanguage(language)).catch((err) => {
        console.warn('[app-navigation-menu] language sync failed:', err)
      })
    }

    syncLanguage(i18n.language)
    i18n.on('languageChanged', syncLanguage)

    return () => {
      i18n.off('languageChanged', syncLanguage)
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | null = null

    void (async () => {
      try {
        const handle = await listen<string>(TAURI_EVENTS.NAVIGATION_MENU_COMMAND, (event) => {
          const navigation = useUiStore.getState()
          if (event.payload === 'back') {
            navigation.goBack()
          } else if (event.payload === 'forward') {
            navigation.goForward()
          }
        })
        if (cancelled) {
          handle()
        } else {
          unlisten = handle
        }
      } catch (err) {
        console.warn('[app-navigation-menu] listen failed:', err)
      }
    })()

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
}
