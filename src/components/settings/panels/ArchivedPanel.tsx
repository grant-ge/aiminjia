import { useEffect, useState } from 'react'
import {
  deleteConversation,
  getArchivedConversations,
  getConversations,
  restoreConversation,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import type { Conversation } from '@/types/message'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'

interface ArchivedConversation {
  id: string
  title: string
  updatedAt: string
  isArchived: boolean
}

type PendingAction = { kind: 'restore' | 'delete'; item: ArchivedConversation } | null

function toConversation(raw: Record<string, unknown>): Conversation {
  return {
    id: (raw.id as string) ?? '',
    title: (raw.title as string) ?? '新对话',
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
    useChatStore.getState().setConversations(raw.map(toConversation))
  }

  const handleRestore = async (item: ArchivedConversation) => {
    setOperatingId(item.id)
    try {
      await restoreConversation(item.id)
      await Promise.all([load(), reloadConversations()])
      pushNotification({
        level: 'success',
        title: '恢复成功',
        message: `「${item.title}」已恢复到聊天列表。`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (error) {
      pushNotification({
        level: 'error',
        title: '恢复失败',
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
        title: '已彻底删除',
        message: `「${item.title}」及其消息和文件已删除。`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (error) {
      pushNotification({
        level: 'error',
        title: '删除失败',
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
    return <div className="text-sm text-muted-foreground p-4">加载中...</div>
  }

  if (items.length === 0) {
    return <div className="text-sm text-muted-foreground p-4">暂无归档记录</div>
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
              恢复
            </Button>
            <Button
              size="sm"
              variant="destructive"
              disabled={operatingId === item.id}
              onClick={() => setPendingAction({ kind: 'delete', item })}
            >
              彻底删除
            </Button>
          </div>
        </div>
      ))}
      <AlertDialog open={!!pendingAction} onOpenChange={(open) => !open && setPendingAction(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {pendingAction?.kind === 'restore' ? '恢复此聊天？' : '彻底删除此聊天？'}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {pendingAction?.kind === 'restore'
                ? '恢复后聊天会重新出现在左侧聊天列表中。'
                : '此操作会永久删除聊天、消息和关联文件，无法撤销。'}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              className={pendingAction?.kind === 'delete' ? 'bg-destructive text-destructive-foreground hover:brightness-110' : undefined}
              onClick={confirmPendingAction}
            >
              {pendingAction?.kind === 'restore' ? '确认恢复' : '确认删除'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
