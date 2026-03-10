import { useEffect } from 'react'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'

export function useUpdater() {
  useEffect(() => {
    let cancelled = false

    async function checkForUpdate() {
      try {
        const update = await check()
        if (cancelled || !update) return

        const yes = window.confirm(
          `发现新版本 v${update.version}，是否立即更新？\n\n${update.body ?? ''}`
        )
        if (!yes) return

        await update.downloadAndInstall()
        await relaunch()
      } catch (e) {
        console.warn('Update check failed:', e)
      }
    }

    // Delay 3s after launch to not block initial render
    const timer = setTimeout(checkForUpdate, 3000)

    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [])
}
