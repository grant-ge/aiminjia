import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'

interface SkillUploadModalProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SkillUploadModal({ open, onOpenChange }: SkillUploadModalProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>上传技能</DialogTitle>
          <DialogDescription>上传能力即将开放。</DialogDescription>
        </DialogHeader>
      </DialogContent>
    </Dialog>
  )
}
