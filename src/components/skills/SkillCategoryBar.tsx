import { Button } from '@/components/ui/button'
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
    <div className="flex w-full min-w-0 items-center gap-2 overflow-x-auto overflow-y-hidden px-1 pb-1">
      {items.map((it) => {
        const active = it.key === activeKey
        return (
          <Button unstyled
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'h-8 max-w-[220px] shrink-0 truncate rounded-md bg-[rgba(var(--primary-rgb),0.10)] px-3 text-sm font-semibold text-primary shadow-[inset_0_0_0_1px_rgba(var(--primary-rgb),0.12)]'
                : 'h-8 max-w-[220px] shrink-0 truncate rounded-md px-3 text-sm font-semibold text-muted-foreground/80 transition-colors hover:bg-muted/40 hover:text-foreground'
            }
            title={it.label}
          >
            {it.label}
          </Button>
        )
      })}
    </div>
  )
}
