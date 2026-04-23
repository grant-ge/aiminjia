/**
 * @designSource design.pen#ORsy4 statusList
 * @sizing wrapper r-14 border 1 padding 8 gap 8; iconBox 34×34 r-10
 */
import type { ReactNode } from 'react'

export type HomeStatusVariant = 'empty' | 'loading' | 'success'

export interface HomeStatusItem {
  key: string
  variant: HomeStatusVariant
  icon: ReactNode
  title: string
  desc: string
}

interface HomeStatusListProps {
  items: HomeStatusItem[]
}

const VARIANT_BG: Record<HomeStatusVariant, string | undefined> = {
  empty: 'bg-brand-primary-subtle',
  loading: 'bg-brand-secondary-subtle',
  success: undefined, // applied via inline style for #DCFCE7
}

export function HomeStatusList({ items }: HomeStatusListProps) {
  return (
    <div className="flex w-full flex-col gap-2 rounded-[14px] border border-border bg-card p-2">
      {items.map((it) => {
        const bgClass = VARIANT_BG[it.variant]
        const successStyle =
          it.variant === 'success' ? { backgroundColor: '#DCFCE7' } : undefined
        return (
          <div key={it.key} className="flex items-center gap-3.5 rounded-[10px] px-4 py-3.5">
            <div
              data-testid={`status-iconbox-${it.key}`}
              style={successStyle}
              className={
                bgClass
                  ? `flex h-[34px] w-[34px] shrink-0 items-center justify-center rounded-[10px] ${bgClass}`
                  : 'flex h-[34px] w-[34px] shrink-0 items-center justify-center rounded-[10px]'
              }
            >
              {it.icon}
            </div>
            <div className="flex min-w-0 flex-col gap-1">
              <div className="text-sm font-semibold text-foreground">{it.title}</div>
              <div className="text-[13px] text-muted-foreground">{it.desc}</div>
            </div>
          </div>
        )
      })}
    </div>
  )
}
