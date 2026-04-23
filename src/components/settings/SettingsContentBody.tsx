/**
 * @designSource design.pen#7wrps/fRV7f/0M01f
 * @sizing padding [24,32] gap 24
 */
import type { PropsWithChildren } from 'react'

export function SettingsContentBody({ children }: PropsWithChildren) {
  return (
    <div className="flex flex-1 flex-col gap-6 overflow-auto px-8 py-6">
      {children}
    </div>
  )
}
