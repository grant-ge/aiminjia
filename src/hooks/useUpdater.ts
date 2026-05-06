import { useEffect, useRef, useState, useCallback } from 'react'
import type { Update } from '@tauri-apps/plugin-updater'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
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

export interface UpdaterPrompt {
  version: string
  notes: string
}

/**
 * Auto-updater. On startup checks `update.json`; if a newer version is
 * available and the user hasn't snoozed this exact version, returns the
 * prompt so a parent component can render UpdaterDialog. Caller decides
 * timing: `acceptUpdate()` downloads + installs + relaunches; `dismissUpdate()`
 * remembers the snooze locally.
 */
export function useUpdater(): {
  prompt: UpdaterPrompt | null
  acceptUpdate: () => Promise<void>
  dismissUpdate: () => void
} {
  const [prompt, setPrompt] = useState<UpdaterPrompt | null>(null)
  const updateRef = useRef<Update | null>(null)
  const cancelledRef = useRef(false)

  useEffect(() => {
    cancelledRef.current = false

    async function checkForUpdate() {
      try {
        const update = await check()
        if (cancelledRef.current || !update) return

        const currentVersion = await getVersion()
        if (update.version === currentVersion) {
          console.info(`Update skipped: remote ${update.version} = current ${currentVersion}`)
          return
        }

        if (getSnoozedVersion() === update.version) return

        updateRef.current = update
        setPrompt({
          version: update.version,
          notes: update.body ?? '',
        })
      } catch (e) {
        console.warn('Update check failed:', e)
      }
    }

    const timer = setTimeout(checkForUpdate, 3000)
    return () => {
      cancelledRef.current = true
      clearTimeout(timer)
    }
  }, [])

  const dismissUpdate = useCallback(() => {
    if (updateRef.current) {
      setSnoozedVersion(updateRef.current.version)
      console.info('User snoozed update to', updateRef.current.version)
    }
    setPrompt(null)
    updateRef.current = null
  }, [])

  const acceptUpdate = useCallback(async () => {
    const update = updateRef.current
    if (!update) {
      setPrompt(null)
      return
    }
    setPrompt(null)

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
      updateRef.current = null
      return
    }

    await relaunch()
  }, [])

  return { prompt, acceptUpdate, dismissUpdate }
}
