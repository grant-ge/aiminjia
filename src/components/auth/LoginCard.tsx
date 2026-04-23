/**
 * @designSource design.pen#PFEwh
 * @sizing w 460 r-18 padding [40,40,32,40] gap 20 border 1 bg card
 */
import type { PropsWithChildren } from 'react'

export function LoginCard({ children }: PropsWithChildren) {
  return (
    <div
      data-testid="login-card"
      className="flex w-[460px] flex-col gap-5 rounded-[18px] border border-border bg-card px-10 pb-8 pt-10"
    >
      {children}
    </div>
  )
}
