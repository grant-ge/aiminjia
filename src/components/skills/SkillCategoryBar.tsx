/**
 * @designSource design.pen#ueSct catBar
 * @sizing row gap 8; chip padding [6,12] r-6
 */
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
                ? 'rounded-md bg-secondary px-3 py-1.5 text-[13px] font-semibold text-foreground shadow-sm'
                : 'rounded-md px-3 py-1.5 text-[13px] font-medium text-muted-foreground transition-colors hover:bg-muted'
            }
          >
            {it.key === activeKey ? it.label : it.label}
          </button>
        )
      })}
    </div>
  )
}
