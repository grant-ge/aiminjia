export interface SkillCategoryItem {
  key: string
  label: string
}

interface SkillCategoryBarProps {
  items: SkillCategoryItem[]
  activeKey: string
  onSelect: (key: string) => void
}

export function SkillCategoryBar({ items, activeKey, onSelect }: SkillCategoryBarProps) {
  return (
    <div className="flex w-full min-w-0 items-center gap-1 overflow-x-auto overflow-y-hidden rounded-md border border-border bg-card p-1 pb-2">
      {items.map((it) => {
        const active = it.key === activeKey
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'h-8 max-w-[220px] shrink-0 truncate rounded-[var(--radius)] bg-brand-primary-subtle px-3 text-sm font-semibold text-primary'
                : 'h-8 max-w-[220px] shrink-0 truncate rounded-[var(--radius)] px-3 text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground'
            }
            title={it.label}
          >
            {it.label}
          </button>
        )
      })}
    </div>
  )
}
