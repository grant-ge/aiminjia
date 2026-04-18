import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ask } from '@tauri-apps/plugin-dialog'

import { Button } from '@/components/common/Button'
import {
  addMcpServer,
  connectMcpServer,
  disconnectMcpServer,
  listMcpServers,
  removeMcpServer,
  type McpServerConfig,
  type McpServerStatus,
} from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import { McpServerForm } from './McpServerForm'
import { McpServerList } from './McpServerList'

export function McpTab() {
  const { t } = useTranslation()
  const pushNotification = useNotificationStore((state) => state.push)

  const [servers, setServers] = useState<McpServerStatus[]>([])
  const [loading, setLoading] = useState(true)
  const [showForm, setShowForm] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [actionLoading, setActionLoading] = useState<Record<string, boolean>>({})

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const next = await listMcpServers()
      setServers(next)
    } catch (error) {
      setServers([])
      pushNotification({
        level: 'error',
        title: t('settings.mcp.loadFailed'),
        message: String(error),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    } finally {
      setLoading(false)
    }
  }, [pushNotification, t])

  useEffect(() => {
    void reload()
  }, [reload])

  const setRowLoading = (name: string, next: boolean) => {
    setActionLoading((state) => ({ ...state, [name]: next }))
  }

  const handleAdd = async (config: McpServerConfig) => {
    setSubmitting(true)
    try {
      await addMcpServer(config)
      setShowForm(false)
      await reload()
      pushNotification({
        level: 'success',
        title: t('settings.mcp.addSuccess'),
        message: config.name,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (error) {
      pushNotification({
        level: 'error',
        title: t('settings.mcp.addFailed'),
        message: String(error),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    } finally {
      setSubmitting(false)
    }
  }

  const handleConnect = async (name: string) => {
    setRowLoading(name, true)
    try {
      const ids = await connectMcpServer(name)
      await reload()
      pushNotification({
        level: 'success',
        title: t('settings.mcp.connectSuccess'),
        message: t('settings.mcp.list.tools', { count: ids.length }),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (error) {
      pushNotification({
        level: 'error',
        title: t('settings.mcp.connectFailed'),
        message: String(error),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    } finally {
      setRowLoading(name, false)
    }
  }

  const handleDisconnect = async (name: string) => {
    setRowLoading(name, true)
    try {
      await disconnectMcpServer(name)
      await reload()
      pushNotification({
        level: 'success',
        title: t('settings.mcp.disconnectSuccess'),
        message: name,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (error) {
      pushNotification({
        level: 'error',
        title: t('settings.mcp.disconnectFailed'),
        message: String(error),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    } finally {
      setRowLoading(name, false)
    }
  }

  const handleDelete = async (name: string) => {
    const confirmed = await ask(t('settings.mcp.confirmDelete', { name }), {
      title: 'AI小家',
      kind: 'warning',
    })
    if (!confirmed) return

    setRowLoading(name, true)
    try {
      await removeMcpServer(name)
      await reload()
      pushNotification({
        level: 'success',
        title: t('settings.mcp.deleteSuccess'),
        message: name,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (error) {
      pushNotification({
        level: 'error',
        title: t('settings.mcp.deleteFailed'),
        message: String(error),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    } finally {
      setRowLoading(name, false)
    }
  }

  return (
    <div>
      <div className="mb-4 flex items-start justify-between gap-4">
        <div>
          <div
            className="text-sm font-semibold"
            style={{ color: 'var(--color-text-primary)' }}
          >
            {t('settings.mcp.title')}
          </div>
          <p className="mt-1 text-sm" style={{ color: 'var(--color-text-secondary)' }}>
            {t('settings.mcp.description')}
          </p>
        </div>
        <Button variant="primary" size="sm" onClick={() => setShowForm((value) => !value)}>
          {t('settings.mcp.addServer')}
        </Button>
      </div>

      <McpServerForm
        visible={showForm}
        onSubmit={handleAdd}
        onCancel={() => setShowForm(false)}
        submitting={submitting}
      />

      <McpServerList
        servers={servers}
        loading={loading}
        onConnect={handleConnect}
        onDisconnect={handleDisconnect}
        onDelete={handleDelete}
        actionLoading={actionLoading}
      />
    </div>
  )
}
