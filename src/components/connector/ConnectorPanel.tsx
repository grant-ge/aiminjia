/**
 * ConnectorPanel — Main panel for managing internal system app connections.
 */
import { useEffect, useState, useCallback } from 'react'
import type { InternalAppInfo } from '@/lib/tauri'
import { getInternalApps, syncInternalApps } from '@/lib/tauri'
import { InternalAppCard } from './InternalAppCard'

export function ConnectorPanel() {
  const [apps, setApps] = useState<InternalAppInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [syncing, setSyncing] = useState(false)
  const [error, setError] = useState('')

  const loadApps = useCallback(async () => {
    try {
      const result = await getInternalApps()
      setApps(result)
      setError('')
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { loadApps() }, [loadApps])

  const handleSync = async () => {
    setSyncing(true)
    setError('')
    try {
      await syncInternalApps()
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
        <div>
          <h3 className="text-sm font-medium" style={{ color: 'var(--color-text-primary)' }}>
            内部系统连接
          </h3>
          <p className="mt-0.5 text-xs" style={{ color: 'var(--color-text-muted)' }}>
            连接企业内部系统后，AI 可以自动查询数据
          </p>
        </div>
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
          {syncing ? '同步中...' : '同步'}
        </button>
      </div>

      {error && (
        <p className="text-xs" style={{ color: 'var(--color-semantic-red)' }}>{error}</p>
      )}

      {loading ? (
        <p className="text-xs" style={{ color: 'var(--color-text-muted)' }}>加载中...</p>
      ) : apps.length === 0 ? (
        <div
          className="rounded-lg border border-dashed p-6 text-center"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <p className="text-sm" style={{ color: 'var(--color-text-secondary)' }}>
            暂无已配置的内部系统
          </p>
          <p className="mt-1 text-xs" style={{ color: 'var(--color-text-muted)' }}>
            点击"同步"从组织获取可用的内部系统列表
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {apps.map((app) => (
            <InternalAppCard key={app.id} app={app} onStatusChange={loadApps} />
          ))}
        </div>
      )}
    </div>
  )
}
