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

  if (!servers || servers.length === 0) {
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
        const isReady = server.state === 'ready'
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
                <StatusBadge state={server.state} />
              </div>
              <div className="mt-1 break-all text-xs" style={{ color: 'var(--color-text-secondary)' }}>
                {server.endpoint}
              </div>
              <div className="mt-1 text-xs" style={{ color: 'var(--color-text-muted)' }}>
                {t('settings.mcp.list.tools', { count: server.registeredToolIds.length })}
              </div>
              {server.lastError ? (
                <div className="mt-1 text-xs" style={{ color: 'var(--color-semantic-red, #ef4444)' }}>
                  {server.lastError}
                </div>
              ) : null}
            </div>

            <div className="flex flex-wrap items-center gap-2">
              {isReady ? (
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

function StatusBadge({ state }: { state: McpServerStatus['state'] }) {
  const { t } = useTranslation()

  const palette = {
    ready: {
      bg: 'rgba(52, 199, 89, 0.12)',
      fg: 'var(--color-semantic-green, #34C759)',
      text: t('settings.mcp.list.statusReady'),
    },
    connecting: {
      bg: 'rgba(59, 130, 246, 0.12)',
      fg: 'var(--color-semantic-blue, #3b82f6)',
      text: t('settings.mcp.list.statusConnecting'),
    },
    failed: {
      bg: 'rgba(239, 68, 68, 0.12)',
      fg: 'var(--color-semantic-red, #ef4444)',
      text: t('settings.mcp.list.statusFailed'),
    },
    disconnected: {
      bg: 'rgba(148, 163, 184, 0.12)',
      fg: 'var(--color-text-muted)',
      text: t('settings.mcp.list.statusDisconnected'),
    },
    configured: {
      bg: 'rgba(148, 163, 184, 0.12)',
      fg: 'var(--color-text-muted)',
      text: t('settings.mcp.list.statusConfigured'),
    },
  } as const

  const status = palette[state]

  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs"
      style={{
        background: status.bg,
        color: status.fg,
      }}
    >
      <span
        className="inline-block h-1.5 w-1.5 rounded-full"
        style={{ background: status.fg }}
      />
      {status.text}
    </span>
  )
}
