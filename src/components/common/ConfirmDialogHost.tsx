import { useTranslation } from 'react-i18next'
import { create } from 'zustand'

import { ConfirmDialog, type ConfirmDialogOptions } from './ConfirmDialog'

interface ConfirmDialogRequest extends ConfirmDialogOptions {
  resolve: (confirmed: boolean) => void
}

interface ConfirmDialogState {
  request: ConfirmDialogRequest | null
  ask: (options: ConfirmDialogOptions) => Promise<boolean>
  resolve: (confirmed: boolean) => void
}

export const useConfirmDialogStore = create<ConfirmDialogState>((set, get) => ({
  request: null,
  ask: (options) =>
    new Promise<boolean>((resolve) => {
      set({ request: { ...options, resolve } })
    }),
  resolve: (confirmed) => {
    const current = get().request
    if (!current) return
    set({ request: null })
    current.resolve(confirmed)
  },
}))

export function requestConfirm(options: ConfirmDialogOptions): Promise<boolean> {
  return useConfirmDialogStore.getState().ask(options)
}

export function ConfirmDialogHost() {
  const { t } = useTranslation()
  const request = useConfirmDialogStore((state) => state.request)
  const resolve = useConfirmDialogStore((state) => state.resolve)

  return (
    <ConfirmDialog
      open={!!request}
      title={request?.title ?? ''}
      description={request?.description ?? ''}
      confirmLabel={request?.confirmLabel ?? t('common.confirm')}
      cancelLabel={request?.cancelLabel}
      variant={request?.variant}
      onOpenChange={(open) => {
        if (!open) resolve(false)
      }}
      onConfirm={() => resolve(true)}
    />
  )
}
