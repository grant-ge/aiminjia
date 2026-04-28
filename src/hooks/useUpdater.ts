import { useEffect } from 'react'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { getVersion } from '@tauri-apps/api/app'
import { useNotificationStore } from '@/stores/notificationStore'
import i18n from '@/i18n'

const SNOOZED_VERSION_KEY = 'update-snoozed-version'

function getSnoozedVersion(): string | null {
  try { return localStorage.getItem(SNOOZED_VERSION_KEY) } catch { return null }
}

function setSnoozedVersion(version: string) {
  try { localStorage.setItem(SNOOZED_VERSION_KEY, version) } catch { /* ignore */ }
}

export function useUpdater() {
  useEffect(() => {
    let cancelled = false

    async function checkForUpdate() {
      try {
        const update = await check()
        if (cancelled || !update) return

        // Skip if remote version is same as (or older than) current
        const currentVersion = await getVersion()
        if (update.version === currentVersion) {
          console.info(`Update skipped: remote ${update.version} = current ${currentVersion}`)
          return
        }

        // User already snoozed this version (persisted across restarts)
        if (getSnoozedVersion() === update.version) return

        const yes = await requestConfirm({
          title: i18n.t('updater.newVersionFound', { version: update.version }),
          description: `${update.body ?? i18n.t('updater.updateAvailableDesc')}`,
          confirmLabel: i18n.t('updater.updateNow'),
          cancelLabel: i18n.t('updater.updateLater'),
        })
        if (!yes || cancelled) {
          setSnoozedVersion(update.version)
          console.info('User snoozed update to', update.version)
          return
        }

        // Show downloading toast with a unique id so we can dismiss it later
        const downloadToastId = `update-download-${Date.now()}`
        useNotificationStore.getState().push({
          id: downloadToastId,
          level: 'info',
          title: i18n.t('updater.downloading'),
          message: i18n.t('updater.downloadingDesc', { version: update.version }),
          actions: [],
          dismissible: false,
          persistent: true,
          context: 'toast',
        })

        let downloaded = 0
        let total = 0
        try {
          await update.downloadAndInstall((event) => {
            if (event.event === 'Started' && event.data.contentLength) {
              total = event.data.contentLength
            } else if (event.event === 'Progress') {
              downloaded += event.data.chunkLength
              if (total > 0) {
                const pct = Math.round((downloaded / total) * 100)
                useNotificationStore.getState().update(downloadToastId, {
                  message: i18n.t('updater.downloadingProgress', { version: update.version, pct }),
                })
              }
            } else if (event.event === 'Finished') {
              useNotificationStore.getState().dismiss(downloadToastId)
              useNotificationStore.getState().push({
                level: 'success',
                title: i18n.t('updater.downloadComplete'),
                message: i18n.t('updater.restarting'),
                actions: [],
                dismissible: false,
                autoHide: 3,
                context: 'toast',
              })
            }
          })
        } catch (downloadErr) {
          // Dismiss the persistent downloading toast
          useNotificationStore.getState().dismiss(downloadToastId)
          useNotificationStore.getState().push({
            level: 'error',
            title: i18n.t('updater.downloadFailed'),
            message: i18n.t('updater.downloadFailedDesc'),
            actions: [],
            dismissible: true,
            autoHide: 8,
            context: 'toast',
          })
          console.warn('Update download failed:', downloadErr)
          return
        }

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
