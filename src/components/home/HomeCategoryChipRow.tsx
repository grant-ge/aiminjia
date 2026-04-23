/**
 * @designSource design.pen#Mk2H9 catRow
 * @sizing wrapper padding [8,12] r-14 border 1, chip padding [8,12] r-10
 */
import { Sparkles } from 'lucide-react'

export interface HomeChipItem {
  key: string
  label: string
}

interface HomeCategoryChipRowProps {
  items: HomeChipItem[]
  activeKey: string
  onSelect: (key: string) => void
}

export function HomeCategoryChipRow({
  items,
  activeKey,
  onSelect,
}: HomeCategoryChipRowProps) {
  return (
    <div className="flex w-full items-center gap-2 rounded-[14px] border border-border bg-card px-3 py-2">
      {items.map((it) => {
        const active = it.key === activeKey
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'flex items-center gap-1.5 rounded-[10px] bg-brand-primary-subtle px-3 py-2 text-[13px] font-semibold text-primary'
                : 'flex items-center gap-1.5 rounded-[10px] px-3 py-2 text-[13px] font-medium text-muted-foreground transition-colors hover:bg-muted'
            }
          >
            {active ? <Sparkles className="h-3.5 w-3.5" /> : null}
            <span>{it.label}</span>
          </button>
        )
      })}
    </div>
  )
}
