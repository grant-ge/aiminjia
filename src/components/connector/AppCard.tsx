/**
 * AppCard — Individual SaaS app card with enable toggle, connection status, and credential setup.
 */
import { useState } from 'react'
import { open } from '@tauri-apps/plugin-shell'
import type { SaasAppInfo } from '@/lib/tauri'
import {
  startOauthConnect,
  checkOauthStatus,
  disconnectSaas,
  toggleSaasApp,
  setSaasCredential,
} from '@/lib/tauri'

interface AppCardProps {
  app: SaasAppInfo
  onStatusChange: () => void
}

export function AppCard({ app, onStatusChange }: AppCardProps) {
  const [loading, setLoading] = useState(false)
  const [polling, setPolling] = useState(false)
  const [showCredInput, setShowCredInput] = useState(false)
  const [credential, setCredential] = useState('')

  const enabled = app.enabled ?? false
  const authRequired = app.authRequired ?? true
  const hasCredential = app.hasCredential ?? false
  const needsCredential = enabled && authRequired && !hasCredential && !app.connected

  const handleToggle = async () => {
    setLoading(true)
    try {
      await toggleSaasApp(app.id, !enabled)
      onStatusChange()
    } catch (e) {
      console.error('Toggle failed:', e)
    } finally {
      setLoading(false)
    }
  }

  const handleConnect = async () => {
    setLoading(true)
    try {
      if (app.connectMode === 'oauth') {
        const url = await startOauthConnect(app.id)
        await open(url)
        setPolling(true)
        pollStatus()
      }
    } catch (e) {
      console.error('Connect failed:', e)
    } finally {
      setLoading(false)
    }
  }

  const pollStatus = async () => {
    for (let i = 0; i < 30; i++) {
      await new Promise((r) => setTimeout(r, 2000))
      try {
        const status = await checkOauthStatus(app.id)
        if (status === 'active') {
          setPolling(false)
          onStatusChange()
          return
        }
      } catch {
        break
      }
    }
    setPolling(false)
  }

  const handleDisconnect = async () => {
    setLoading(true)
    try {
      await disconnectSaas(app.id)
      onStatusChange()
    } catch (e) {
      console.error('Disconnect failed:', e)
    } finally {
      setLoading(false)
    }
  }

  const handleSaveCredential = async () => {
    if (!credential.trim()) return
    setLoading(true)
    try {
      await setSaasCredential(app.name, credential.trim())
      setCredential('')
      setShowCredInput(false)
      onStatusChange()
    } catch (e) {
      console.error('Set credential failed:', e)
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
        opacity: enabled ? 1 : 0.6,
      }}
    >
      <div className="flex items-center gap-3">
        {/* Icon */}
        {app.iconUrl ? (
          <img src={app.iconUrl} alt={app.name} className="h-10 w-10 rounded-lg object-cover" />
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
            {enabled && (
              <span
                className="inline-block h-2 w-2 rounded-full"
                style={{
                  backgroundColor: app.connected
                    ? 'var(--color-semantic-green)'
                    : needsCredential
                      ? 'var(--color-semantic-orange)'
                      : 'var(--color-text-disabled)',
                }}
              />
            )}
          </div>
          {app.summary && (
            <p className="text-xs truncate" style={{ color: 'var(--color-text-secondary)' }}>
              {app.summary}
            </p>
          )}
          {app.capabilities.length > 0 && (
            <p className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
              {app.capabilities.length} capabilities
            </p>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2">
          {/* Enable/Disable toggle */}
          <button
            onClick={handleToggle}
            disabled={loading}
            className="relative inline-flex h-5 w-9 items-center rounded-full transition-colors disabled:opacity-50"
            style={{
              backgroundColor: enabled ? 'var(--color-primary)' : 'var(--color-border-light)',
            }}
            title={enabled ? 'Disable' : 'Enable'}
          >
            <span
              className="inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform"
              style={{
                transform: enabled ? 'translateX(18px)' : 'translateX(3px)',
              }}
            />
          </button>

          {/* Connect / Disconnect / Set Credential */}
          {enabled && (
            <>
              {app.connected ? (
                <button
                  onClick={handleDisconnect}
                  disabled={loading}
                  className="rounded px-3 py-1 text-xs transition-colors disabled:opacity-50"
                  style={{ color: 'var(--color-semantic-red)' }}
                >
                  Disconnect
                </button>
              ) : needsCredential ? (
                <button
                  onClick={() => setShowCredInput(!showCredInput)}
                  disabled={loading}
                  className="rounded px-3 py-1 text-xs transition-colors disabled:opacity-50"
                  style={{
                    color: 'var(--color-semantic-orange)',
                    backgroundColor: 'rgba(245,166,35,0.08)',
                  }}
                >
                  Set Credential
                </button>
              ) : app.connectMode === 'oauth' && !app.connected ? (
                <button
                  onClick={handleConnect}
                  disabled={loading || polling}
                  className="rounded px-3 py-1 text-xs transition-colors disabled:opacity-50"
                  style={{
                    backgroundColor: 'var(--color-primary)',
                    color: 'var(--color-text-on-primary)',
                  }}
                >
                  {polling ? 'Waiting...' : loading ? 'Connecting...' : 'Connect'}
                </button>
              ) : null}
            </>
          )}
        </div>
      </div>

      {/* Credential input (expandable) */}
      {showCredInput && (
        <div className="mt-3 flex gap-2" style={{ borderTop: '1px solid var(--color-border-subtle)', paddingTop: '12px' }}>
          <input
            type="password"
            value={credential}
            onChange={(e) => setCredential(e.target.value)}
            placeholder="Paste API Key or Token..."
            className="flex-1 rounded px-2 py-1 text-xs outline-none"
            style={{
              border: '1px solid var(--color-border)',
              backgroundColor: 'var(--color-bg-input)',
              color: 'var(--color-text-primary)',
            }}
            onKeyDown={(e) => { if (e.key === 'Enter') handleSaveCredential() }}
          />
          <button
            onClick={handleSaveCredential}
            disabled={loading || !credential.trim()}
            className="rounded px-3 py-1 text-xs transition-colors disabled:opacity-50"
            style={{
              backgroundColor: 'var(--color-primary)',
              color: 'var(--color-text-on-primary)',
            }}
          >
            Save
          </button>
        </div>
      )}

      {/* Warning banner for enabled but missing credential */}
      {needsCredential && !showCredInput && (
        <p className="mt-2 text-xs" style={{ color: 'var(--color-semantic-orange)' }}>
          This app requires credentials to work. Click "Set Credential" or provide it in the chat.
        </p>
      )}
    </div>
  )
}
