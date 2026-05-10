/**
 * @designSource design.pen#PFEwh
 * @sizing w 460 r-18 padding [40,40,32,40] gap 20 border 1 bg card
 */
import type { PropsWithChildren } from 'react'

export function LoginCard({ children }: PropsWithChildren) {
  return (
    <div
      data-testid="login-card"
      className="relative flex w-[460px] flex-col gap-5 rounded-xl border border-border/60 px-10 pb-8 pt-10"
      style={{
        background: 'rgba(255, 255, 255, 0.75)',
        backdropFilter: 'blur(24px)',
        WebkitBackdropFilter: 'blur(24px)',
        boxShadow: '0 8px 40px rgba(0,0,0,0.08), 0 1px 0 rgba(255,255,255,0.9) inset',
      }}
    >
      {children}
    </div>
  )
}
