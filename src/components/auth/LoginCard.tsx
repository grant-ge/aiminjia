/**
 * @designSource design.pen#PFEwh
 * @sizing w 460 r-18 padding [40,40,32,40] gap 20 border 1 bg card
 */
import type { PropsWithChildren } from 'react'

export function LoginCard({ children }: PropsWithChildren) {
  return (
    <div
      data-testid="login-card"
      // spec §3.3 — page-level container rounded-md; §5 shadow-lg.
      // Glass-pane uses rgba(var(--card-rgb), 0.75); --card-rgb is kept in sync
      // with --card by deriveBackgroundPalette() in brandingStore.
      className="relative flex w-[460px] flex-col gap-5 rounded-md border border-[rgba(var(--border-rgb),0.60)] px-10 pb-8 pt-10 shadow-[var(--shadow-lg)]"
      style={{
        background: 'rgba(var(--card-rgb, 250, 250, 248), 0.75)',
        backdropFilter: 'blur(24px)',
        WebkitBackdropFilter: 'blur(24px)',
      }}
    >
      {children}
    </div>
  )
}
