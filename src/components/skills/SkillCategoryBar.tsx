import { Button } from '@/components/ui/button'
export interface SkillCategoryItem {
  key: string
  label: string
  count?: number
}

interface SkillCategoryBarProps {
  items: SkillCategoryItem[]
  activeKey: string
  onSelect: (key: string) => void
  itemDataAttribute?: string
}

export function SkillCategoryBar({ items, activeKey, onSelect, itemDataAttribute }: SkillCategoryBarProps) {
  return (
    <div className="flex w-full min-w-0 items-center gap-2 overflow-x-auto overflow-y-hidden px-1 pb-1">
      {items.map((it) => {
        const active = it.key === activeKey
        const dataAttrs = itemDataAttribute ? { [itemDataAttribute]: it.key } : {}
        return (
          <Button unstyled
            key={it.key}
            type="button"
            aria-label={it.label}
            {...dataAttrs}
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'inline-flex h-8 max-w-[220px] shrink-0 items-center truncate rounded-md bg-[rgba(var(--primary-rgb),0.10)] px-3 text-sm font-semibold text-primary shadow-[inset_0_0_0_1px_rgba(var(--primary-rgb),0.12)]'
                : 'inline-flex h-8 max-w-[220px] shrink-0 items-center truncate rounded-md px-3 text-sm font-semibold text-[rgba(var(--muted-foreground-rgb),0.80)] transition-colors hover:bg-[rgba(var(--muted-rgb),0.40)] hover:text-foreground'
            }
            title={it.label}
          >
            <span>{it.label}</span>
            {typeof it.count === 'number' ? (
              <span className="ml-1.5 rounded-[2px] bg-muted px-1.5 py-0 text-2xs leading-4 text-muted-foreground">
                {it.count}
              </span>
            ) : null}
          </Button>
        )
      })}
    </div>
  )
}
