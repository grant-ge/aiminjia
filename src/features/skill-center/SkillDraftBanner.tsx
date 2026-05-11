import { useEffect, useState, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { useNotificationStore } from '@/stores/notificationStore'

interface DraftMetaInfo {
  draft_id: string
  conversation_id: string | null
  name: string
  description: string
  created_at: string
  last_modified_at: string
  installed_to: string | null
}

export function SkillDraftBanner() {
  const [drafts, setDrafts] = useState<DraftMetaInfo[]>([])
  const [loading, setLoading] = useState(false)
  const pushNotif = useNotificationStore((s) => s.push)

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const list = await invoke<DraftMetaInfo[]>('list_skill_drafts')
      setDrafts(list.filter((d) => !d.installed_to))
    } catch (err) {
      console.error('[SkillDraftBanner] list failed', err)
      setDrafts([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [reload])

  const handleDiscard = useCallback(async (draftId: string, name: string) => {
    const ok = await requestConfirm({
      title: '放弃草稿？',
      description: `将永久删除草稿 "${name}"，无法恢复。`,
      confirmLabel: '放弃',
      variant: 'destructive',
    })
    if (!ok) return
    try {
      await invoke('discard_skill_draft', { draftId })
      pushNotif({ context: 'toast', level: 'success', title: '', message: '草稿已放弃', actions: [], dismissible: true })
      void reload()
    } catch (err) {
      pushNotif({ context: 'toast', level: 'error', title: '', message: `删除失败：${err}`, actions: [], dismissible: true })
    }
  }, [pushNotif, reload])

  if (loading || drafts.length === 0) return null

  return (
    <div className="rounded-lg border border-primary/30 bg-accent-soft px-4 py-3">
      <div className="mb-2 text-xs font-semibold text-foreground">
        🛠️ 你有 {drafts.length} 个未完成的技能草稿（小程帮你创建中）
      </div>
      <ul className="space-y-1">
        {drafts.map((d) => (
          <li key={d.draft_id} className="flex items-center justify-between gap-3 text-xs">
            <div className="flex-1 min-w-0">
              <span className="font-medium text-foreground">{d.name || '(未命名)'}</span>
              {d.description ? (
                <span className="ml-2 text-muted-foreground truncate">{d.description}</span>
              ) : null}
            </div>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void handleDiscard(d.draft_id, d.name || '(未命名)')}
            >
              放弃
            </Button>
          </li>
        ))}
      </ul>
    </div>
  )
}
