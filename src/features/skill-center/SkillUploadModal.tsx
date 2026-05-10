import { useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'

import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { useNotificationStore } from '@/stores/notificationStore'
import { useSkillStore } from '@/stores/skillStore'
import { uploadWithOverwriteConfirm } from './uploadWithOverwriteConfirm'

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
      const result = await uploadWithOverwriteConfirm((force) => upload(selected, force))
      if (result === 'installed') {
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
      }
      // 'cancelled' — silent, modal stays open
    } catch (err) {
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
          <DialogTitle>从目录导入技能</DialogTitle>
          <DialogDescription>
            开发者选项：选一个本地目录，目录内须有 <code>SKILL.md</code>。<br />
            一般用户请用 <strong>"导入 .aijia-skill"</strong> 选择同事发来的技能包文件。
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          {error ? <p className="text-sm text-destructive">{error}</p> : null}
          <div className="flex items-center justify-end gap-2">
            <Button size="sm" variant="outline" onClick={() => onOpenChange(false)} disabled={isUploading}>
              取消
            </Button>
            <Button size="sm" onClick={() => void handlePickDirectory()} disabled={isUploading}>
              {isUploading ? '正在导入...' : '选择目录'}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
