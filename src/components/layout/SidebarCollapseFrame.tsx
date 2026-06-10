import type { ReactNode } from 'react'

interface SidebarCollapseFrameProps {
  hidden: boolean
  children: ReactNode
}

export function SidebarCollapseFrame({ hidden, children }: SidebarCollapseFrameProps) {
  const inertProps = hidden ? { inert: true } : {}

  return (
    <div
      data-aijia-sidebar-collapse-frame
      data-state={hidden ? 'collapsed' : 'expanded'}
      aria-hidden={hidden}
      className={`h-full shrink-0 overflow-hidden transition-[width] duration-200 ease-out motion-reduce:transition-none ${
        hidden ? 'w-0' : 'w-64'
      }`}
      {...inertProps}
    >
      {children}
    </div>
  )
}
