/**
 * @designSource design.pen#Mk2H9 catRow
 * @sizing wrapper padding [8,12] r-14 border 1, chip padding [8,12] r-10
 */
import type { ReactNode } from 'react'
import {
  BarChart3,
  Bot,
  FileText,
  PencilLine,
  Search,
  Sparkles,
} from 'lucide-react'

export interface HomeChipItem {
  key: string
  label: string
  icon?: 'sparkles' | 'pencil' | 'search' | 'file' | 'chart' | 'bot'
}

interface HomeCategoryChipRowProps {
  items: HomeChipItem[]
  activeKey: string
  onSelect: (key: string) => void
}

function renderIcon(icon?: HomeChipItem['icon']): ReactNode {
  switch (icon) {
    case 'pencil':
      return <PencilLine className="h-4.5 w-4.5" />
    case 'search':
      return <Search className="h-4.5 w-4.5" />
    case 'file':
      return <FileText className="h-4.5 w-4.5" />
    case 'chart':
      return <BarChart3 className="h-4.5 w-4.5" />
    case 'bot':
      return <Bot className="h-4.5 w-4.5" />
    case 'sparkles':
    default:
      return <Sparkles className="h-4.5 w-4.5" />
  }
}

export function HomeCategoryChipRow({
  items,
  activeKey,
  onSelect,
}: HomeCategoryChipRowProps) {
  return (
    <div className="flex w-full items-center justify-between gap-1.5 rounded-[26px] bg-card/95 p-2">
      {items.map((it) => {
        const active = it.key === activeKey
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'flex min-w-0 flex-1 items-center justify-center gap-1.5 rounded-[20px] px-3 py-3 text-[14px] font-semibold text-foreground'
                : 'flex min-w-0 flex-1 items-center justify-center gap-1.5 rounded-[20px] px-3 py-3 text-[14px] font-medium text-muted-foreground'
            }
          >
            <span className={active ? 'text-primary' : ''}>{renderIcon(it.icon)}</span>
            <span className={active ? 'text-primary' : ''}>{it.label}</span>
          </button>
        )
      })}
    </div>
  )
}
