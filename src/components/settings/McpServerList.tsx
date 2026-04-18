import { useTranslation } from 'react-i18next'

import { Button } from '@/components/common/Button'
import type { McpServerStatus } from '@/lib/tauri'

interface McpServerListProps {
  servers: McpServerStatus[]
  loading: boolean
  onConnect: (name: string) => Promise<void>
  onDisconnect: (name: string) => Promise<void>
  onDelete: (name: string) => Promise<void>
  actionLoading: Record<string, boolean>
}

export function McpServerList({
  servers,
  loading,
  onConnect,
  onDisconnect,
  onDelete,
  actionLoading,
}: McpServerListProps) {
  const { t } = useTranslation()

  if (loading) {
    return (
      <div
        className="rounded-lg border p-4 text-sm"
        style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-secondary)' }}
      >
        {t('common.loading')}
      </div>
    )
  }

  if (servers.length === 0) {
    return (
      <div
        className="rounded-lg border p-6 text-center"
        style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-secondary)' }}
      >
        <p className="mb-1">{t('settings.mcp.list.empty')}</p>
        <p className="text-xs">{t('settings.mcp.list.emptyHint')}</p>
      </div>
    )
  }

  return (
    <div className="space-y-2">
      {servers.map((server) => {
        const rowBusy = !!actionLoading[server.name]
        return (
          <div
            key={server.name}
            className="flex flex-col gap-3 rounded-lg border px-4 py-3 md:flex-row md:items-center md:justify-between"
            style={{
              borderColor: 'var(--color-border)',
              background: 'var(--color-bg-main)',
            }}
          >
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium" style={{ color: 'var(--color-text-primary)' }}>
                  {server.name}
                </span>
                <span
                  className="rounded-full px-2 py-0.5 text-[11px] uppercase tracking-[0.08em]"
                  style={{
                    background: 'var(--color-bg-card)',
                    color: 'var(--color-text-muted)',
                    border: '1px solid var(--color-border)',
                  }}
                >
                  {server.transportType}
                </span>
                <StatusBadge connected={server.connected} />
              </div>
              <div className="mt-1 break-all text-xs" style={{ color: 'var(--color-text-secondary)' }}>
                {server.endpoint}
              </div>
              <div className="mt-1 text-xs" style={{ color: 'var(--color-text-muted)' }}>
                {t('settings.mcp.list.tools', { count: server.registeredToolIds.length })}
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              {server.connected ? (
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={rowBusy}
                  onClick={() => onDisconnect(server.name)}
                >
                  {rowBusy
                    ? t('settings.mcp.list.disconnecting')
                    : t('settings.mcp.list.disconnect')}
                </Button>
              ) : (
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={rowBusy}
                  onClick={() => onConnect(server.name)}
                >
                  {rowBusy
                    ? t('settings.mcp.list.connecting')
                    : t('settings.mcp.list.connect')}
                </Button>
              )}
              <Button
                variant="secondary"
                size="sm"
                disabled={rowBusy}
                onClick={() => onDelete(server.name)}
              >
                {t('settings.mcp.list.delete')}
              </Button>
            </div>
          </div>
        )
      })}
    </div>
  )
}

function StatusBadge({ connected }: { connected: boolean }) {
  const { t } = useTranslation()

  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs"
      style={{
        background: connected
          ? 'rgba(52, 199, 89, 0.12)'
          : 'rgba(148, 163, 184, 0.12)',
        color: connected ? 'var(--color-semantic-green, #34C759)' : 'var(--color-text-muted)',
      }}
    >
      <span
        className="inline-block h-1.5 w-1.5 rounded-full"
        style={{
          background: connected ? 'var(--color-semantic-green, #34C759)' : 'var(--color-text-muted)',
        }}
      />
      {connected ? t('settings.mcp.list.statusConnected') : t('settings.mcp.list.statusDisconnected')}
    </span>
  )
}
