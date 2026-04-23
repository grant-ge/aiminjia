/**
 * @designSource design.pen#ip8MF popover
 * @sizing w 560 r-14 bg popover border 1 shadow lvl-2; head padding [12,16] bottom-border 1; row padding [10,16]
 */
import { X } from 'lucide-react'

export interface SkillPopoverItem {
  id: string
  title: string
  subtitle: string
  source: string
}

interface SkillPopoverPanelProps {
  items: SkillPopoverItem[]
  onPick: (id: string) => void
  onClose: () => void
}

export function SkillPopoverPanel({ items, onPick, onClose }: SkillPopoverPanelProps) {
  return (
    <div
      className="w-[560px] overflow-hidden rounded-[14px] border border-border bg-popover"
      style={{
        boxShadow: '0 2px 3.5px -1px rgba(0,0,0,0.06), 0 4px 5.25px -1px rgba(0,0,0,0.10)',
      }}
    >
      <header className="flex items-center justify-between border-b border-border px-4 py-3 text-[12px] font-semibold text-muted-foreground">
        <span>管理已安装的技能</span>
        <button
          type="button"
          aria-label="关闭"
          onClick={onClose}
          className="text-muted-foreground transition-colors hover:text-foreground"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </header>
      <ul
        data-testid="skill-popover-list"
        className="flex max-h-[320px] flex-col overflow-auto"
      >
        {items.map((it) => (
          <li key={it.id}>
            <button
              type="button"
              onClick={() => onPick(it.id)}
              className="flex w-full items-center justify-between gap-3 px-4 py-2.5 text-left transition-colors hover:bg-muted"
            >
              <div className="flex min-w-0 flex-col">
                <span className="truncate text-sm font-semibold text-foreground">{it.title}</span>
                <span className="truncate text-[12px] text-muted-foreground">{it.subtitle}</span>
              </div>
              <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-[12px] text-muted-foreground">
                {it.source}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
