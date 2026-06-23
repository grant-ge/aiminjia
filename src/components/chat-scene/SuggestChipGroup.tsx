/**
 * @designSource design.pen#kFmPc
 * @sizing caption 12 muted; chip r-999 border 1 bg background padding [6,12] gap 8
 */
import type { ReactNode } from 'react'
import { Button } from '@/components/ui/button'

export interface SuggestChip {
  label: string
  icon?: ReactNode
  onClick: () => void
}

interface SuggestChipGroupProps {
  caption?: string
  items: SuggestChip[]
}

export function SuggestChipGroup({ caption = '建议回复', items }: SuggestChipGroupProps) {
  return (
    <div className="flex flex-col gap-2">
      <div className="text-xs text-muted-foreground">{caption}</div>
      <div className="flex flex-wrap gap-2">
        {items.map((it, i) => (
          <Button unstyled
            key={i}
            type="button"
            onClick={it.onClick}
            className="flex items-center gap-2 rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground transition-colors hover:bg-muted"
          >
            {it.icon}
            <span>{it.label}</span>
          </Button>
        ))}
      </div>
    </div>
  )
}
