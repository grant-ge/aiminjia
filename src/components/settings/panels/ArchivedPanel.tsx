import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check } from 'lucide-react'
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
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'

interface ArchivedConversation {
  id: string
  title: string
  updatedAt: string
  isArchived: boolean
}

type PendingAction =
  | { kind: 'restore'; item: ArchivedConversation }
  | { kind: 'delete'; item: ArchivedConversation }
  | { kind: 'batchDelete'; ids: string[] }
  | null

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
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [batchDeleting, setBatchDeleting] = useState(false)
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

  // Drop selections that no longer exist (e.g. after delete) so the batch bar
  // count never lies.
  useEffect(() => {
    setSelectedIds((prev) => {
      if (prev.size === 0) return prev
      const valid = new Set(items.map((i) => i.id))
      let changed = false
      const next = new Set<string>()
      for (const id of prev) {
        if (valid.has(id)) next.add(id)
        else changed = true
      }
      return changed ? next : prev
    })
  }, [items])

  const toggleSelected = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const allSelected = useMemo(
    () => items.length > 0 && selectedIds.size === items.length,
    [items.length, selectedIds.size],
  )

  const toggleSelectAll = () => {
    if (allSelected) setSelectedIds(new Set())
    else setSelectedIds(new Set(items.map((i) => i.id)))
  }

  const clearSelection = () => setSelectedIds(new Set())

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

  const handleBatchDelete = async (ids: string[]) => {
    setBatchDeleting(true)
    try {
      // Sequential rather than Promise.all: each delete cancels active agents
      // and removes files; running them in parallel can race on shared FS
      // resources and produce confusing partial-failure logs.
      let successCount = 0
      const failures: string[] = []
      for (const id of ids) {
        try {
          await deleteConversation(id)
          successCount += 1
        } catch (error) {
          failures.push(getErrorMessage(error))
        }
      }
      await load()
      if (failures.length === 0) {
        pushNotification({
          level: 'success',
          title: t('settings.archived.permanentlyDeleted'),
          message: t('settings.archived.batchDeleteSucceeded', { count: successCount }),
          actions: [],
          dismissible: true,
          autoHide: 4,
          context: 'toast',
        })
      } else {
        pushNotification({
          level: 'warning',
          title: t('settings.archived.deleteFailed'),
          message: t('settings.archived.batchDeletePartial', {
            success: successCount,
            failure: failures.length,
          }),
          actions: [],
          dismissible: true,
          autoHide: 6,
          context: 'toast',
        })
      }
    } finally {
      setBatchDeleting(false)
      setPendingAction(null)
      setSelectedIds(new Set())
    }
  }

  const confirmPendingAction = () => {
    if (!pendingAction) return
    if (pendingAction.kind === 'restore') {
      void handleRestore(pendingAction.item)
    } else if (pendingAction.kind === 'delete') {
      void handleDelete(pendingAction.item)
    } else if (pendingAction.kind === 'batchDelete') {
      void handleBatchDelete(pendingAction.ids)
    }
  }

  if (loading) {
    return <div className="text-sm text-muted-foreground p-4">{t('common.loading')}</div>
  }

  if (items.length === 0) {
    return <div className="text-sm text-muted-foreground p-4">{t('settings.archived.noRecords')}</div>
  }

  const selectedCount = selectedIds.size

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between rounded-md border border-border bg-card px-4 py-2">
        <div className="flex items-center gap-3">
          <CheckSquare
            checked={allSelected}
            indeterminate={!allSelected && selectedCount > 0}
            onClick={toggleSelectAll}
          />
          <span className="text-xs text-muted-foreground">
            {selectedCount > 0
              ? t('settings.archived.selectedCount', { count: selectedCount })
              : t('settings.archived.selectAll')}
          </span>
        </div>
        {selectedCount > 0 ? (
          <div className="flex items-center gap-2">
            <Button size="sm" variant="ghost" onClick={clearSelection} disabled={batchDeleting}>
              {t('settings.archived.clearSelection')}
            </Button>
            <Button
              size="sm"
              variant="destructive"
              disabled={batchDeleting}
              onClick={() =>
                setPendingAction({ kind: 'batchDelete', ids: Array.from(selectedIds) })
              }
            >
              {t('settings.archived.deleteSelected')}
            </Button>
          </div>
        ) : null}
      </div>
      {items.map((item) => {
        const checked = selectedIds.has(item.id)
        return (
          <div
            key={item.id}
            className="flex items-center justify-between rounded-md border border-border bg-card px-4 py-3"
          >
            <div className="flex items-center gap-3">
              <CheckSquare checked={checked} onClick={() => toggleSelected(item.id)} />
              <div className="flex flex-col gap-0.5">
                <span className="text-sm font-medium text-foreground">{item.title}</span>
                <span className="text-xs text-muted-foreground">
                  {new Date(item.updatedAt).toLocaleDateString('zh-CN')}
                </span>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={operatingId === item.id || batchDeleting}
                onClick={() => setPendingAction({ kind: 'restore', item })}
              >
                {t('settings.archived.restore')}
              </Button>
              <Button
                size="sm"
                variant="destructive"
                disabled={operatingId === item.id || batchDeleting}
                onClick={() => setPendingAction({ kind: 'delete', item })}
              >
                {t('settings.archived.permanentDelete')}
              </Button>
            </div>
          </div>
        )
      })}
      <ConfirmDialog
        open={!!pendingAction}
        title={
          pendingAction?.kind === 'restore'
            ? t('settings.archived.restoreThisChat')
            : pendingAction?.kind === 'batchDelete'
              ? t('settings.archived.deleteSelectedTitle', { count: pendingAction.ids.length })
              : t('settings.archived.deleteThisChat')
        }
        description={
          pendingAction?.kind === 'restore'
            ? t('settings.archived.restoreDescription')
            : pendingAction?.kind === 'batchDelete'
              ? t('settings.archived.deleteSelectedDescription')
              : t('settings.archived.deleteDescription')
        }
        confirmLabel={
          pendingAction?.kind === 'restore'
            ? t('settings.archived.confirmRestore')
            : t('common.confirm')
        }
        variant={
          pendingAction?.kind === 'delete' || pendingAction?.kind === 'batchDelete'
            ? 'destructive'
            : 'default'
        }
        onOpenChange={(open) => !open && setPendingAction(null)}
        onConfirm={confirmPendingAction}
      />
    </div>
  )
}

interface CheckSquareProps {
  checked: boolean
  indeterminate?: boolean
  onClick: () => void
}

function CheckSquare({ checked, indeterminate, onClick }: CheckSquareProps) {
  const active = checked || indeterminate
  return (
    <Button unstyled
      type="button"
      role="checkbox"
      aria-checked={indeterminate ? 'mixed' : checked}
      onClick={onClick}
      className={cn(
        'flex h-4 w-4 shrink-0 items-center justify-center rounded-md border transition-colors',
        active
          ? 'border-primary bg-primary text-primary-foreground'
          : 'border-input bg-background hover:border-primary',
      )}
    >
      {active ? <Check className="h-3 w-3" strokeWidth={3} /> : null}
    </Button>
  )
}
