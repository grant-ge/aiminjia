import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  deleteConversation,
  getArchivedConversations,
  getConversations,
  restoreConversation,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import type { Conversation } from '@/types/message'
import { ConfirmDialog } from '@/components/common/ConfirmDialog'
import { Button } from '@/components/ui/button'

interface ArchivedConversation {
  id: string
  title: string
  updatedAt: string
  isArchived: boolean
}

type PendingAction = { kind: 'restore' | 'delete'; item: ArchivedConversation } | null

function toConversation(raw: Record<string, unknown>, newChatLabel: string): Conversation {
  return {
    id: (raw.id as string) ?? '',
    title: (raw.title as string) ?? newChatLabel,
    createdAt: (raw.createdAt as string) ?? new Date().toISOString(),
    updatedAt: (raw.updatedAt as string) ?? new Date().toISOString(),
    isArchived: (raw.isArchived as boolean) ?? false,
    workspaceName: (raw.workspaceName as string | undefined) ?? undefined,
  }
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function ArchivedPanel() {
  const { t } = useTranslation()
  const [items, setItems] = useState<ArchivedConversation[]>([])
  const [loading, setLoading] = useState(true)
  const [operatingId, setOperatingId] = useState<string | null>(null)
  const [pendingAction, setPendingAction] = useState<PendingAction>(null)
  const pushNotification = useNotificationStore((s) => s.push)

  const load = async () => {
    setLoading(true)
    try {
      const data = await getArchivedConversations()
      setItems(data)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { void load() }, [])

  const reloadConversations = async () => {
    const raw = await getConversations()
    useChatStore.getState().setConversations(raw.map((r) => toConversation(r, t('settings.archived.newChat'))))
  }

  const handleRestore = async (item: ArchivedConversation) => {
    setOperatingId(item.id)
    try {
      await restoreConversation(item.id)
      await Promise.all([load(), reloadConversations()])
      pushNotification({
        level: 'success',
        title: t('settings.archived.restoreSuccess'),
        message: t('settings.archived.restoredToList', { title: item.title }),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (error) {
      pushNotification({
        level: 'error',
        title: t('settings.archived.restoreFailed'),
        message: getErrorMessage(error),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setOperatingId(null)
      setPendingAction(null)
    }
  }

  const handleDelete = async (item: ArchivedConversation) => {
    setOperatingId(item.id)
    try {
      await deleteConversation(item.id)
      await load()
      pushNotification({
        level: 'success',
        title: t('settings.archived.permanentlyDeleted'),
        message: t('settings.archived.deletedWithFiles', { title: item.title }),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (error) {
      pushNotification({
        level: 'error',
        title: t('settings.archived.deleteFailed'),
        message: getErrorMessage(error),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setOperatingId(null)
      setPendingAction(null)
    }
  }

  const confirmPendingAction = () => {
    if (!pendingAction) return
    if (pendingAction.kind === 'restore') {
      void handleRestore(pendingAction.item)
    } else {
      void handleDelete(pendingAction.item)
    }
  }

  if (loading) {
    return <div className="text-sm text-muted-foreground p-4">{t('common.loading')}</div>
  }

  if (items.length === 0) {
    return <div className="text-sm text-muted-foreground p-4">{t('settings.archived.noRecords')}</div>
  }

  return (
    <div className="flex flex-col gap-2">
      {items.map((item) => (
        <div key={item.id} className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3">
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium text-foreground">{item.title}</span>
            <span className="text-xs text-muted-foreground">
              {new Date(item.updatedAt).toLocaleDateString('zh-CN')}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={operatingId === item.id}
              onClick={() => setPendingAction({ kind: 'restore', item })}
            >
              {t('settings.archived.restore')}
            </Button>
            <Button
              size="sm"
              variant="destructive"
              disabled={operatingId === item.id}
              onClick={() => setPendingAction({ kind: 'delete', item })}
            >
              {t('settings.archived.permanentDelete')}
            </Button>
          </div>
        </div>
      ))}
      <ConfirmDialog
        open={!!pendingAction}
        title={pendingAction?.kind === 'restore' ? t('settings.archived.restoreThisChat') : t('settings.archived.deleteThisChat')}
        description={
          pendingAction?.kind === 'restore'
            ? t('settings.archived.restoreDescription')
            : t('settings.archived.deleteDescription')
        }
        confirmLabel={pendingAction?.kind === 'restore' ? t('settings.archived.confirmRestore') : t('common.confirm')}
        variant={pendingAction?.kind === 'delete' ? 'destructive' : 'default'}
        onOpenChange={(open) => !open && setPendingAction(null)}
        onConfirm={confirmPendingAction}
      />
    </div>
  )
}
