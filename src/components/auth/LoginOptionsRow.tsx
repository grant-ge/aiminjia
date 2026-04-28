/**
 * @designSource design.pen#hfGT2
 */
import type { ReactNode } from 'react'

interface LoginOptionsRowProps {
  rememberSlot: ReactNode
  onForget: () => void
}

export function LoginOptionsRow({ rememberSlot, onForget }: LoginOptionsRowProps) {
  return (
    <div className="flex w-full items-center justify-between">
      {rememberSlot}
      <button
        type="button"
        onClick={onForget}
        className="text-[0.8125rem] font-medium text-brand-secondary transition-colors hover:opacity-80"
      >
        忘记密码？
      </button>
    </div>
  )
}
