/**
 * @designSource design.pen#PqcAk / canvas* family
 *
 * Page shell owns the common desktop workbench rhythm: 1280px content width,
 * 32px horizontal padding, and stable section gaps.
 */
import type { PropsWithChildren, ReactNode } from 'react'

interface PageSectionShellProps extends PropsWithChildren {
  topBar?: ReactNode
  /** @deprecated alias of topBar; will be removed in plan-B */
  header?: ReactNode
  /** Tailwind padding classes; default keeps pages aligned to the desktop workbench grid. */
  padding?: string
  /** Tailwind gap class; default "gap-6" */
  gap?: string
  /** override max width if needed (default 1280) */
  maxWidthClass?: string
  /** @deprecated alias of (padding + gap); will be removed in plan-B */
  className?: string
}

export function PageSectionShell({
  topBar,
  header,
  padding = 'px-8 pt-7 pb-10',
  gap = 'gap-6',
  maxWidthClass = 'max-w-[1280px]',
  className = '',
  children,
}: PageSectionShellProps) {
  const top = topBar ?? header
  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      {top}
      <div className="min-h-0 flex-1 overflow-auto">
        <div
          className={`mx-auto flex w-full ${maxWidthClass} flex-col ${gap} ${padding} ${className}`.trim()}
        >
          {children}
        </div>
      </div>
    </div>
  )
}
