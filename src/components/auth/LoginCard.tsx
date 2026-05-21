/**
 * @designSource design.pen#PFEwh
 * @sizing w 460 r-18 padding [40,40,32,40] gap 20 border 1 bg card
 */
import type { PropsWithChildren } from 'react'

export function LoginCard({ children }: PropsWithChildren) {
  return (
    <div
      data-testid="login-card"
      // spec §3.3 — page-level container rounded-xl; §5 shadow-lg.
      // Glass-pane uses color-mix from --card so it stays correct under
      // tenant theming / dark backgrounds instead of hardcoded white.
      className="relative flex w-[460px] flex-col gap-5 rounded-xl border border-border/60 px-10 pb-8 pt-10 shadow-[var(--shadow-lg)]"
      style={{
        background: 'color-mix(in srgb, var(--card) 75%, transparent)',
        backdropFilter: 'blur(24px)',
        WebkitBackdropFilter: 'blur(24px)',
      }}
    >
      {children}
    </div>
  )
}
