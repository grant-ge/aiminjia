/**
 * @designSource design.pen#5aczK/dQk75/YuBIQ
 * @sizing h 56, padding [0,28], bottom-border 1
 */
import { X } from 'lucide-react'
import { Button } from '@/components/ui/button'

interface SettingsContentTopProps {
  title: string
  onClose: () => void
}

export function SettingsContentTop({ title, onClose }: SettingsContentTopProps) {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-7">
      <div className="text-base font-bold text-foreground">{title}</div>
      <Button unstyled
        type="button"
        aria-label="关闭"
        onClick={onClose}
        className="text-muted-foreground transition-colors hover:text-foreground"
      >
        <X className="h-4 w-4" />
      </Button>
    </header>
  )
}
