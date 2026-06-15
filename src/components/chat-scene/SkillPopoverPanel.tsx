/**
 * @designSource design.pen#ip8MF popover
 * @sizing w 420 r-12 bg popover border 1; head padding [10,12] bottom-border 1; row padding [8,12]
 *
 * Skill picker popover with keyboard navigation:
 * - ArrowUp/ArrowDown moves the highlight
 * - Enter selects the highlighted skill
 * - Escape closes
 * - The highlighted row shows a CornerDownLeft (↵) hint
 * - Footer "explore & manage skills" navigates to skill-center
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronRight, CornerDownLeft, Search, Settings2 } from 'lucide-react'

import { getSkillCategoryBg, getSkillIconComponent } from '@/components/skills/skillVisual'
import { useUiStore } from '@/stores/uiStore'

export interface SkillPopoverItem {
  id: string
  title: string
  subtitle: string
  /**
   * SKILL.md frontmatter `icon` slug (kebab-case lucide name). When missing,
   * falls back to a category-driven default (matches SkillCenter cards).
   */
  icon?: string | null
  /** Skill category slug (`hr` / `finance` / ...). Drives the icon tile color. */
  category?: string | null
  /** Slash command shown in the composer, e.g. `/toggle-restore-skill`. */
  command?: string | null
}

interface SkillPopoverPanelProps {
  items: SkillPopoverItem[]
  onPick: (id: string) => void
  onClose: () => void
}

export function SkillPopoverPanel({ items, onPick, onClose }: SkillPopoverPanelProps) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)
  const panelRef = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLUListElement>(null)
  const setRoute = useUiStore((s) => s.setRoute)

  useEffect(() => {
    const handlePointerDown = (event: PointerEvent) => {
      const panel = panelRef.current
      if (!panel || !(event.target instanceof Node)) return
      if (!panel.contains(event.target)) onClose()
    }

    document.addEventListener('pointerdown', handlePointerDown)
    return () => document.removeEventListener('pointerdown', handlePointerDown)
  }, [onClose])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return items
    // Rank: command/title prefix (0) > title contains (1) > subtitle/id (2); drop non-matches.
    const ranked: Array<{ item: SkillPopoverItem; rank: number }> = []
    for (const it of items) {
      const title = it.title.toLowerCase()
      const sub = it.subtitle.toLowerCase()
      const command = (it.command?.trim() || `/${it.id}`).toLowerCase()
      const bareCommand = command.startsWith('/') ? command.slice(1) : command
      const bareQuery = q.startsWith('/') ? q.slice(1) : q
      let rank = -1
      if (command.startsWith(q) || (bareQuery && bareCommand.startsWith(bareQuery)) || title.startsWith(q)) rank = 0
      else if (title.includes(q)) rank = 1
      else if (sub.includes(q) || it.id.toLowerCase().includes(bareQuery)) rank = 2
      if (rank >= 0) ranked.push({ item: it, rank })
    }
    ranked.sort((a, b) => a.rank - b.rank)
    return ranked.map((r) => r.item)
  }, [items, query])

  const safeActiveIndex = filtered.length === 0 ? 0 : Math.min(activeIndex, filtered.length - 1)

  // Keep highlighted row visible during keyboard navigation.
  useEffect(() => {
    const list = listRef.current
    if (!list) return
    const row = list.querySelector<HTMLElement>(`[data-skill-index="${safeActiveIndex}"]`)
    row?.scrollIntoView?.({ block: 'nearest' })
  }, [safeActiveIndex])

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      if (filtered.length === 0) return
      setActiveIndex((prev) => (prev + 1) % filtered.length)
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      if (filtered.length === 0) return
      setActiveIndex((prev) => (prev - 1 + filtered.length) % filtered.length)
    } else if (event.key === 'Enter') {
      event.preventDefault()
      const target = filtered[safeActiveIndex]
      if (target) onPick(target.id)
    } else if (event.key === 'Escape') {
      event.preventDefault()
      onClose()
    }
  }

  const handleOpenSkillCenter = () => {
    setRoute({ kind: 'skill-center' })
    onClose()
  }

  return (
    <div
      ref={panelRef}
      onKeyDown={handleKeyDown}
      className="w-[420px] overflow-hidden rounded-md border border-border bg-popover shadow-[var(--shadow-popover)]"
    >
      <header className="flex items-center gap-2 px-3 py-2.5">
        <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
        <input
          type="text"
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t('skillPopover.searchPlaceholder')}
          className="flex-1 bg-transparent text-sm text-foreground placeholder:text-muted-foreground focus:outline-none"
          data-aijia-skill-picker-search
          data-testid="skill-popover-search"
        />
      </header>
      <div className="max-h-[210px] overflow-y-auto">
        {filtered.length === 0 ? (
          <div
            data-testid="skill-popover-empty"
            className="flex h-[120px] flex-col items-center justify-center gap-1 px-4 text-center"
          >
            <span className="text-sm font-medium text-foreground">
              {items.length === 0 ? t('skillPopover.emptyNoSkills') : t('skillPopover.emptyNoMatch')}
            </span>
            <span className="text-xs text-muted-foreground">
              {items.length === 0 ? t('skillPopover.emptyInstallHint') : t('skillPopover.emptyTryOther')}
            </span>
          </div>
        ) : (
          <ul
            ref={listRef}
            data-testid="skill-popover-list"
            role="listbox"
            className="flex flex-col py-1"
          >
            {filtered.map((it, idx) => {
              const isActive = idx === safeActiveIndex
              const Icon = getSkillIconComponent(it.icon)
              const iconBg = getSkillCategoryBg(it.category)
              return (
                <li key={it.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={isActive}
                    data-skill-index={idx}
                    data-aijia-skill-picker-item="true"
                    data-aijia-skill-id={it.id}
                    data-aijia-skill-command={it.command?.trim() || `/${it.id}`}
                    data-active={isActive ? 'true' : undefined}
                    onMouseEnter={() => setActiveIndex(idx)}
                    onClick={() => onPick(it.id)}
                    className={
                      isActive
                        ? 'flex w-full items-center gap-3 px-3 py-2 text-left transition-colors bg-muted'
                        : 'flex w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-muted'
                    }
                  >
                    <span
                      className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-md ${iconBg}`}
                    >
                      <Icon className="h-3.5 w-3.5 text-primary-foreground" />
                    </span>
                    <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="truncate text-sm font-medium text-foreground">
                        {it.title}
                      </span>
                      {it.subtitle ? (
                        <span className="truncate text-xs text-muted-foreground">
                          {it.subtitle}
                        </span>
                      ) : null}
                    </span>
                    {isActive ? (
                      <span
                        aria-hidden
                        className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md border border-border text-muted-foreground"
                      >
                        <CornerDownLeft className="h-3 w-3" />
                      </span>
                    ) : null}
                  </button>
                </li>
              )
            })}
          </ul>
        )}
      </div>
      <button
        type="button"
        onClick={handleOpenSkillCenter}
        data-testid="skill-popover-explore"
        className="flex w-full items-center gap-3 border-t border-border px-3 py-2.5 text-left text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
      >
        <Settings2 className="h-4 w-4 shrink-0" />
        <span className="flex-1">{t('skillPopover.exploreAndManage')}</span>
        <ChevronRight className="h-4 w-4 shrink-0" />
      </button>
    </div>
  )
}
