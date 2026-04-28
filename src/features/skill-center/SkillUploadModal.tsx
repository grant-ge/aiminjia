import { useState } from 'react'
import { open, ask } from '@tauri-apps/plugin-dialog'

import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { useNotificationStore } from '@/stores/notificationStore'
import { useSkillStore, SkillAlreadyExistsError } from '@/stores/skillStore'

interface SkillUploadModalProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SkillUploadModal({ open: isOpen, onOpenChange }: SkillUploadModalProps) {
  const upload = useSkillStore((s) => s.upload)
  const pushNotification = useNotificationStore((s) => s.push)
  const [isUploading, setIsUploading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handlePickDirectory() {
    setError(null)
    const selected = await open({ directory: true, multiple: false, title: '选择技能目录' })
    if (!selected || Array.isArray(selected)) return

    setIsUploading(true)
    try {
      await upload(selected)
      pushNotification({
        level: 'success',
        title: '技能上传成功',
        message: '技能已安装并刷新到技能中心。',
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
      onOpenChange(false)
    } catch (err) {
      if (err instanceof SkillAlreadyExistsError) {
        setIsUploading(false)
        const confirmed = await ask(`技能 "${err.skillId}" 已存在，是否覆盖？`, { title: 'AI小家', kind: 'warning' })
        if (!confirmed) return
        setIsUploading(true)
        try {
          await upload(selected, true)
          pushNotification({
            level: 'success',
            title: '技能上传成功',
            message: '技能已安装并刷新到技能中心。',
            actions: [],
            dismissible: true,
            autoHide: 4,
            context: 'toast',
          })
          onOpenChange(false)
        } catch (overwriteErr) {
          const message = overwriteErr instanceof Error ? overwriteErr.message : String(overwriteErr)
          setError(message)
          pushNotification({
            level: 'error',
            title: '技能上传失败',
            message,
            actions: [],
            dismissible: true,
            autoHide: 6,
            context: 'toast',
          })
        }
        return
      }
      const message = err instanceof Error ? err.message : String(err)
      setError(message)
      pushNotification({
        level: 'error',
        title: '技能上传失败',
        message,
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setIsUploading(false)
    }
  }

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>上传技能</DialogTitle>
          <DialogDescription>
            选择一个包含 <code>SKILL.md</code> 的本地技能目录，安装后会自动刷新技能中心。
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          {error ? <p className="text-sm text-destructive">{error}</p> : null}
          <Button onClick={() => void handlePickDirectory()} disabled={isUploading}>
            {isUploading ? '正在上传...' : '选择技能目录'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
