import { useEffect, useState } from 'react'

import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface ConversationRenameDialogProps {
  open: boolean
  initialTitle: string
  onOpenChange: (open: boolean) => void
  onConfirm: (title: string) => void | Promise<void>
}

export function ConversationRenameDialog({
  open,
  initialTitle,
  onOpenChange,
  onConfirm,
}: ConversationRenameDialogProps) {
  const [value, setValue] = useState(initialTitle)

  useEffect(() => {
    if (open) setValue(initialTitle)
  }, [initialTitle, open])

  const trimmed = value.trim()
  const handleConfirm = () => {
    if (!trimmed) return
    void onConfirm(trimmed)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[400px]">
        <DialogHeader>
          <DialogTitle>重命名聊天</DialogTitle>
        </DialogHeader>
        <Input
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') handleConfirm()
          }}
          autoFocus
        />
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={handleConfirm} disabled={!trimmed}>
            确认
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
