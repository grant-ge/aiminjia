/**
 * DingtalkSection — DingTalk account connection and status display.
 * Displayed inside SettingsModal on the DingTalk tab.
 */
import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { open } from '@tauri-apps/plugin-shell'
import { listen } from '@tauri-apps/api/event'
import { Button } from '@/components/common/Button'
import { useNotificationStore } from '@/stores/notificationStore'
import {
  dingtalkLogin,
  dingtalkLogout,
  dingtalkStatus,
  dingtalkRefreshStatus,
  type DingtalkStatusInfo,
} from '@/lib/tauri'

export function DingtalkSection() {
  const { t } = useTranslation()
  const notifications = useNotificationStore()

  const [status, setStatus] = useState<DingtalkStatusInfo | null>(null)
  const [loading, setLoading] = useState(false)
  const [refreshing, setRefreshing] = useState(false)

  // Load status on mount + listen for auth URL event
  useEffect(() => {
    // Use refresh (network call) to get accurate status + user info
    dingtalkRefreshStatus().then(setStatus).catch(() => {
      // Fallback to cached status if network fails
      dingtalkStatus().then(setStatus).catch(() => {})
    })

    // Backend emits 'dingtalk:auth-url' with the OAuth URL to open
    const unlisten = listen<string>('dingtalk:auth-url', (event) => {
      if (event.payload) {
        open(event.payload).catch((e) => console.error('Failed to open auth URL:', e))
      }
    })
    return () => { unlisten.then((fn) => fn()) }
  }, [])

  const handleConnect = async () => {
    setLoading(true)
    try {
      const result = await dingtalkLogin()
      setStatus(result)
      if (result.connected) {
        notifications.push({ level: 'success', title: t('settings.dingtalk.connectSuccess'), message: '', actions: [], dismissible: true, autoHide: 3, context: 'toast' })
      }
    } catch (e) {
      notifications.push({ level: 'error', title: t('settings.dingtalk.connectFailed'), message: String(e), actions: [], dismissible: true, autoHide: 5, context: 'toast' })
    } finally {
      setLoading(false)
    }
  }

  const handleDisconnect = async () => {
    try {
      await dingtalkLogout()
      setStatus({ connected: false, userName: null, corpName: null })
      notifications.push({ level: 'success', title: t('settings.dingtalk.disconnectSuccess'), message: '', actions: [], dismissible: true, autoHide: 3, context: 'toast' })
    } catch (e) {
      notifications.push({ level: 'error', title: String(e), message: '', actions: [], dismissible: true, autoHide: 5, context: 'toast' })
    }
  }

  const handleRefresh = async () => {
    setRefreshing(true)
    try {
      const result = await dingtalkRefreshStatus()
      setStatus(result)
    } catch (e) {
      notifications.push({ level: 'error', title: String(e), message: '', actions: [], dismissible: true, autoHide: 5, context: 'toast' })
    } finally {
      setRefreshing(false)
    }
  }

  const isConnected = status?.connected === true

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h3
          className="text-base font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {t('settings.dingtalk.title')}
        </h3>
        <p
          className="mt-1 text-sm"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          {t('settings.dingtalk.description')}
        </p>
      </div>

      {/* Connection Status Card */}
      <div
        className="rounded-md border p-4 border-border"
        style={{
          borderColor: isConnected ? 'var(--color-accent)' : 'var(--color-border)',
          backgroundColor: 'var(--color-bg-secondary)',
        }}
      >
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            {/* Status indicator dot */}
            <div
              className="h-3 w-3 rounded-md"
              style={{
                backgroundColor: isConnected ? '#22c55e' : '#9ca3af',
              }}
            />
            <div>
              <span
                className="font-medium"
                style={{ color: 'var(--color-text-primary)' }}
              >
                {isConnected
                  ? t('settings.dingtalk.connected')
                  : t('settings.dingtalk.disconnected')}
              </span>
              {isConnected && status?.userName && (
                <p
                  className="mt-0.5 text-sm"
                  style={{ color: 'var(--color-text-secondary)' }}
                >
                  {t('settings.dingtalk.userName')}: {status.userName}
                  {status.corpName && (
                    <> &middot; {t('settings.dingtalk.corpName')}: {status.corpName}</>
                  )}
                </p>
              )}
            </div>
          </div>

          {/* Action buttons */}
          <div className="flex items-center gap-2">
            {isConnected ? (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleRefresh}
                  disabled={refreshing}
                >
                  {refreshing ? '...' : t('settings.dingtalk.refreshButton')}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleDisconnect}
                >
                  {t('settings.dingtalk.disconnectButton')}
                </Button>
              </>
            ) : (
              <Button
                variant="primary"
                size="sm"
                onClick={handleConnect}
                disabled={loading}
              >
                {loading
                  ? t('settings.dingtalk.connecting')
                  : t('settings.dingtalk.connectButton')}
              </Button>
            )}
          </div>
        </div>
      </div>

      {/* Capabilities list */}
      <div>
        <h4
          className="mb-2 text-sm font-medium"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          {t('settings.dingtalk.capabilities')}
        </h4>
        <ul className="space-y-1.5">
          {(['capAitable', 'capContact', 'capChat', 'capCalendar', 'capTodo'] as const).map(
            (key) => (
              <li
                key={key}
                className="flex items-start gap-2 text-sm"
                style={{ color: 'var(--color-text-secondary)' }}
              >
                <span style={{ color: isConnected ? '#22c55e' : '#9ca3af' }}>
                  {isConnected ? '✓' : '○'}
                </span>
                {t(`settings.dingtalk.${key}`)}
              </li>
            ),
          )}
        </ul>
      </div>
    </div>
  )
}
