/**
 * @designSource design.pen#BixkY/aAO2u/tCYsE/WgoHO
 * @sizing height 56, padding [0,24], bottom border 1
 */
import type { ReactNode } from 'react'
import { ChevronRight } from 'lucide-react'

export type PageTopBarVariant = 'default' | 'title' | 'breadcrumb' | 'compact'

export interface BreadcrumbCrumb {
  label: string
  onClick?: () => void
  current?: boolean
}

interface PageTopBarProps {
  variant: PageTopBarVariant
  title?: ReactNode
  breadcrumbs?: BreadcrumbCrumb[]
  leading?: ReactNode
  trailing?: ReactNode
}

export function PageTopBar({
  variant,
  title,
  breadcrumbs,
  leading,
  trailing,
}: PageTopBarProps) {
  return (
    <header data-tauri-drag-region className="flex h-14 shrink-0 items-center justify-between border-b border-border bg-background px-8">
      {variant === 'compact' ? (
        <div className="flex min-w-0 items-center gap-3 text-sm font-semibold text-foreground">
          {leading}
          <span className="truncate">{title}</span>
        </div>
      ) : variant === 'title' ? (
        <div className="flex min-w-0 items-center gap-3">
          {leading}
          {typeof title === 'string' ? (
            <span className="truncate text-[15px] font-semibold leading-[22px] text-foreground">{title}</span>
          ) : (
            title
          )}
        </div>
      ) : variant === 'breadcrumb' ? (
        <div className="flex min-w-0 items-center gap-3">
          {leading}
          {breadcrumbs ? (
            <ol className="flex min-w-0 items-center gap-2 text-sm text-muted-foreground">
              {breadcrumbs.map((c, i) => (
                <li key={i} className="flex items-center gap-2">
                  {i > 0 ? <ChevronRight className="h-3.5 w-3.5" /> : null}
                  {c.onClick ? (
                    <button
                      type="button"
                      className={c.current ? 'text-foreground' : 'hover:text-foreground'}
                      onClick={c.onClick}
                    >
                      {c.label}
                    </button>
                  ) : (
                    <span className={c.current ? 'text-foreground' : ''}>{c.label}</span>
                  )}
                </li>
              ))}
            </ol>
          ) : null}
        </div>
      ) : (
        /* default variant: empty bar */
        <div className="flex min-w-0 items-center gap-3">
          {leading}
        </div>
      )}
      <div className="flex items-center gap-2">
        {trailing ? <div className="flex items-center gap-2">{trailing}</div> : null}
      </div>
    </header>
  )
}
