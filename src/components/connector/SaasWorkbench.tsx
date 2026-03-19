/**
 * SaasWorkbench — Main panel for managing SaaS app connections.
 * Shows a list of available SaaS apps with their connection status.
 */
import { useEffect, useState, useCallback } from 'react'
import type { SaasAppInfo } from '@/lib/tauri'
import { getSaasApps, syncSaasConfig } from '@/lib/tauri'
import { AppCard } from './AppCard'

export function SaasWorkbench() {
  const [apps, setApps] = useState<SaasAppInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [syncing, setSyncing] = useState(false)
  const [error, setError] = useState('')

  const loadApps = useCallback(async () => {
    try {
      const result = await getSaasApps()
      setApps(result)
      setError('')
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadApps()
  }, [loadApps])

  const handleSync = async () => {
    setSyncing(true)
    setError('')
    try {
      await syncSaasConfig()
      await loadApps()
    } catch (e) {
      setError(String(e))
    } finally {
      setSyncing(false)
    }
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium" style={{ color: 'var(--color-text-primary)' }}>
          Connected Systems
        </h3>
        <button
          onClick={handleSync}
          disabled={syncing}
          className="rounded px-3 py-1 text-xs transition-colors disabled:opacity-50"
          style={{
            color: 'var(--color-primary)',
            backgroundColor: syncing ? 'var(--color-primary-subtle)' : 'transparent',
          }}
          onMouseEnter={(e) => { if (!syncing) e.currentTarget.style.backgroundColor = 'var(--color-primary-subtle)' }}
          onMouseLeave={(e) => { if (!syncing) e.currentTarget.style.backgroundColor = 'transparent' }}
        >
          {syncing ? 'Syncing...' : 'Sync'}
        </button>
      </div>

      {error && (
        <p className="text-xs" style={{ color: 'var(--color-semantic-red)' }}>{error}</p>
      )}

      {loading ? (
        <p className="text-xs" style={{ color: 'var(--color-text-muted)' }}>Loading...</p>
      ) : apps.length === 0 ? (
        <div
          className="rounded-lg border border-dashed p-6 text-center"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <p className="text-sm" style={{ color: 'var(--color-text-secondary)' }}>
            No SaaS apps configured
          </p>
          <p className="mt-1 text-xs" style={{ color: 'var(--color-text-muted)' }}>
            Click Sync to fetch available apps from your organization.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {apps.map((app) => (
            <AppCard key={app.id} app={app} onStatusChange={loadApps} />
          ))}
        </div>
      )}
    </div>
  )
}
