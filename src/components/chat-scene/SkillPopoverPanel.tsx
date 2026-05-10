/**
 * @designSource design.pen#ip8MF popover
 * @sizing w 560 r-14 bg popover border 1; head padding [12,16] bottom-border 1; row padding [10,16]
 */
import { Blocks, X } from 'lucide-react'

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
      className="w-[560px] overflow-hidden rounded-lg border border-border bg-popover"
      style={{ boxShadow: '0 4px 12px -4px rgba(0,0,0,0.08)' }}
    >
      <header className="flex items-center justify-between border-b border-border px-4 py-3 text-xs font-semibold text-muted-foreground">
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
      {items.length === 0 ? (
        <div
          data-testid="skill-popover-empty"
          className="flex flex-col items-center justify-center gap-2 px-4 py-10 text-center"
        >
          <div className="flex h-10 w-10 items-center justify-center rounded-full bg-muted text-muted-foreground">
            <Blocks className="h-5 w-5" />
          </div>
          <span className="text-sm font-medium text-foreground">还没有可用的技能</span>
          <span className="text-xs text-muted-foreground">安装技能后会显示在这里</span>
        </div>
      ) : (
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
                  <span className="truncate text-xs text-muted-foreground">{it.subtitle}</span>
                </div>
                <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
                  {it.source}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
