/**
 * @designSource design.pen#PqcAk / canvas* family
 *
 * 把"max-w + padding"分离：max-w 固定 1032，padding/gap 由 padding/gap props 传入。
 * 这样 home / skills / schedules 各页可保留自己的稿子 padding 节奏，
 * 而页面层不需要写颜色/边框。
 */
import type { PropsWithChildren, ReactNode } from 'react'

interface PageSectionShellProps extends PropsWithChildren {
  topBar?: ReactNode
  /** @deprecated alias of topBar; will be removed in plan-B */
  header?: ReactNode
  /** Tailwind padding classes; default "px-8 pt-6 pb-8" — keep pages consistent unless there's a real design reason to override */
  padding?: string
  /** Tailwind gap class; default "gap-5" */
  gap?: string
  /** override max width if needed (default 1032) */
  maxWidthClass?: string
  /** @deprecated alias of (padding + gap); will be removed in plan-B */
  className?: string
}

export function PageSectionShell({
  topBar,
  header,
  padding = 'px-8 pt-6 pb-8',
  gap = 'gap-5',
  maxWidthClass = 'max-w-[1032px]',
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
