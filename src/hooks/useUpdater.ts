import { useEffect } from 'react'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { useNotificationStore } from '@/stores/notificationStore'

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

        // Show downloading toast
        useNotificationStore.getState().push({
          level: 'info',
          title: '正在下载更新',
          message: `正在下载 v${update.version}，请稍候...`,
          actions: [],
          dismissible: false,
          persistent: true,
          context: 'toast',
        })

        let downloaded = 0
        let total = 0
        await update.downloadAndInstall((event) => {
          if (event.event === 'Started' && event.data.contentLength) {
            total = event.data.contentLength
          } else if (event.event === 'Progress') {
            downloaded += event.data.chunkLength
            if (total > 0) {
              const pct = Math.round((downloaded / total) * 100)
              console.log(`Update download: ${pct}%`)
            }
          } else if (event.event === 'Finished') {
            useNotificationStore.getState().push({
              level: 'success',
              title: '更新下载完成',
              message: '即将重启应用...',
              actions: [],
              dismissible: false,
              autoHide: 3,
              context: 'toast',
            })
          }
        })

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
