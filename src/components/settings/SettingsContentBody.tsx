/**
 * @designSource design.pen#7wrps/fRV7f/0M01f
 * @sizing padding [24,32] gap 24
 */
import type { PropsWithChildren } from 'react'

export function SettingsContentBody({ children }: PropsWithChildren) {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto overflow-x-hidden px-8 pb-6 pt-10">
      {children}
    </div>
  )
}
