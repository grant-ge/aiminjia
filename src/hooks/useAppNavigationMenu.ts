import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'

import { TAURI_EVENTS } from '@/lib/tauri'
import { useUiStore } from '@/stores/uiStore'

export function useAppNavigationMenu() {
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
