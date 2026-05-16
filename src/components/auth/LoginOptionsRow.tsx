/**
 * @designSource design.pen#hfGT2
 */
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

interface LoginOptionsRowProps {
  rememberSlot: ReactNode
  onForget: () => void
}

export function LoginOptionsRow({ rememberSlot, onForget }: LoginOptionsRowProps) {
  const { t } = useTranslation()
  return (
    <div className="flex w-full items-center justify-between">
      {rememberSlot}
      <button
        type="button"
        onClick={onForget}
        className="text-sm font-medium text-brand-secondary transition-colors hover:opacity-80"
      >
        {t('login.forgotPassword')}
      </button>
    </div>
  )
}
