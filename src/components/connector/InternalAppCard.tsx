/**
 * InternalAppCard — Card for an internal system app with connect/disconnect via WebView login.
 */
import { useState } from 'react'
import type { InternalAppInfo } from '@/lib/tauri'
import { connectInternalApp, disconnectInternalApp } from '@/lib/tauri'

interface InternalAppCardProps {
  app: InternalAppInfo
  onStatusChange: () => void
}

export function InternalAppCard({ app, onStatusChange }: InternalAppCardProps) {
  const [loading, setLoading] = useState(false)

  const handleConnect = async () => {
    setLoading(true)
    try {
      await connectInternalApp(app.id)
      onStatusChange()
    } catch (e) {
      console.error('连接失败:', e)
    } finally {
      setLoading(false)
    }
  }

  const handleDisconnect = async () => {
    setLoading(true)
    try {
      await disconnectInternalApp(app.id)
      onStatusChange()
    } catch (e) {
      console.error('断开失败:', e)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div
      className="rounded-lg p-3 transition-colors"
      style={{
        border: '1px solid var(--color-border)',
        backgroundColor: 'var(--color-bg-card)',
      }}
    >
      <div className="flex items-center gap-3">
        {/* Icon */}
        {app.icon_url ? (
          <img src={app.icon_url} alt={app.name} className="h-10 w-10 rounded-lg object-cover" />
        ) : (
          <div
            className="flex h-10 w-10 items-center justify-center rounded-lg text-lg font-bold"
            style={{
              backgroundColor: 'var(--color-primary-subtle)',
              color: 'var(--color-primary)',
            }}
          >
            {app.name.charAt(0)}
          </div>
        )}

        {/* Info */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span
              className="text-sm font-medium truncate"
              style={{ color: 'var(--color-text-primary)' }}
            >
              {app.name}
            </span>
            <span
              className="inline-block h-2 w-2 rounded-full"
              style={{
                backgroundColor: app.connected
                  ? 'var(--color-semantic-green)'
                  : 'var(--color-text-disabled)',
              }}
            />
          </div>
          {app.description && (
            <p className="text-xs truncate" style={{ color: 'var(--color-text-secondary)' }}>
              {app.description}
            </p>
          )}
          {app.hints.length > 0 && (
            <p className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
              {app.hints.length} 个功能
            </p>
          )}
        </div>

        {/* Connect / Disconnect */}
        <div className="flex items-center gap-2">
          {app.connected ? (
            <button
              onClick={handleDisconnect}
              disabled={loading}
              className="rounded px-3 py-1 text-xs transition-colors disabled:opacity-50"
              style={{ color: 'var(--color-semantic-red)' }}
            >
              断开
            </button>
          ) : (
            <button
              onClick={handleConnect}
              disabled={loading}
              className="rounded px-3 py-1 text-xs transition-colors disabled:opacity-50"
              style={{
                backgroundColor: 'var(--color-primary)',
                color: 'var(--color-text-on-primary)',
              }}
            >
              {loading ? '连接中...' : '连接'}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
