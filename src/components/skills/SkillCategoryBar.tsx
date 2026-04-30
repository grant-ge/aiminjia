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
    <div className="flex w-full flex-wrap items-center gap-2">
      {items.map((it) => {
        const active = it.key === activeKey
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'rounded-full bg-brand-primary-subtle px-3.5 py-2 text-[0.8125rem] font-semibold text-primary'
                : 'rounded-full px-3.5 py-2 text-[0.8125rem] font-medium text-muted-foreground transition-colors hover:bg-muted/60'
            }
          >
            {it.label}
          </button>
        )
      })}
    </div>
  )
}
