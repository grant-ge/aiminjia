/**
 * @designSource design.pen#ip8MF popover
 * @sizing w 560 r-14 bg popover border 1; head padding [12,16] bottom-border 1; row padding [10,16]
 */
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Blocks, Search, X } from 'lucide-react'

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
  const { t } = useTranslation()
  const [query, setQuery] = useState('')

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return items
    // Rank: title prefix (0) > title contains (1) > subtitle (2) > source (3); drop non-matches.
    const ranked: Array<{ item: SkillPopoverItem; rank: number }> = []
    for (const it of items) {
      const title = it.title.toLowerCase()
      const sub = it.subtitle.toLowerCase()
      const src = it.source.toLowerCase()
      let rank = -1
      if (title.startsWith(q)) rank = 0
      else if (title.includes(q)) rank = 1
      else if (sub.includes(q)) rank = 2
      else if (src.includes(q)) rank = 3
      if (rank >= 0) ranked.push({ item: it, rank })
    }
    ranked.sort((a, b) => a.rank - b.rank)
    return ranked.map((r) => r.item)
  }, [items, query])

  return (
    <div
      className="w-[560px] overflow-hidden rounded-lg border border-border bg-popover"
      style={{ boxShadow: '0 4px 12px -4px rgba(0,0,0,0.08)' }}
    >
      <header className="flex items-center gap-2 border-b border-border px-3 py-2.5">
        <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
        <input
          type="text"
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t('skillPopover.searchPlaceholder')}
          className="flex-1 bg-transparent text-sm text-foreground placeholder:text-muted-foreground focus:outline-none"
          data-testid="skill-popover-search"
        />
        <button
          type="button"
          aria-label={t('common.close')}
          onClick={onClose}
          className="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
        >
          <X className="h-4 w-4" />
        </button>
      </header>
      <div className="h-[280px] overflow-hidden">
        {filtered.length === 0 ? (
          <div
            data-testid="skill-popover-empty"
            className="flex h-full flex-col items-center justify-center gap-2 px-4 text-center"
          >
            <div className="flex h-10 w-10 items-center justify-center rounded-full bg-muted text-muted-foreground">
              <Blocks className="h-5 w-5" />
            </div>
            <span className="text-sm font-medium text-foreground">
              {items.length === 0 ? t('skillPopover.emptyNoSkills') : t('skillPopover.emptyNoMatch')}
            </span>
            <span className="text-xs text-muted-foreground">
              {items.length === 0 ? t('skillPopover.emptyInstallHint') : t('skillPopover.emptyTryOther')}
            </span>
          </div>
        ) : (
          <ul
            data-testid="skill-popover-list"
            className="flex h-full flex-col overflow-auto"
          >
            {filtered.map((it) => (
              <li key={it.id}>
                <button
                  type="button"
                  onClick={() => onPick(it.id)}
                  className="flex w-full items-center justify-between gap-3 px-4 py-2.5 text-left transition-colors hover:bg-muted"
                >
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate text-sm font-medium text-foreground">{it.title}</span>
                    <span className="truncate text-xs text-muted-foreground">{it.subtitle}</span>
                  </div>
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {it.source}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}
