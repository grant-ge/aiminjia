import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'

interface SkillMarketModalProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SkillMarketModal({ open, onOpenChange }: SkillMarketModalProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>技能市场</DialogTitle>
          <DialogDescription>技能市场即将开放。</DialogDescription>
        </DialogHeader>
      </DialogContent>
    </Dialog>
  )
}
