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
    <div className="flex w-full items-center gap-1 overflow-x-auto rounded-md border border-border bg-card p-1 [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      {items.map((it) => {
        const active = it.key === activeKey
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'h-8 shrink-0 rounded-[var(--radius)] bg-brand-primary-subtle px-3 text-sm font-semibold text-primary'
                : 'h-8 shrink-0 rounded-[var(--radius)] px-3 text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground'
            }
          >
            {it.label}
          </button>
        )
      })}
    </div>
  )
}
