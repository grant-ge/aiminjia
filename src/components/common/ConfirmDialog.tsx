import { useTranslation } from 'react-i18next'

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { cn } from '@/lib/utils'

export type ConfirmDialogVariant = 'default' | 'destructive'

export interface ConfirmDialogOptions {
  title: string
  description: string
  confirmLabel: string
  cancelLabel?: string
  variant?: ConfirmDialogVariant
}

interface ConfirmDialogProps extends ConfirmDialogOptions {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  cancelLabel,
  variant = 'default',
  onOpenChange,
  onConfirm,
}: ConfirmDialogProps) {
  const { t } = useTranslation()
  const resolvedCancelLabel = cancelLabel ?? t('common.cancel')
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent data-aijia-dialog="confirm">
        <AlertDialogHeader>
          <AlertDialogTitle data-aijia-dialog-title>{title}</AlertDialogTitle>
          <AlertDialogDescription data-aijia-dialog-description>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel
            className="border-input"
            data-aijia-dialog-action="cancel"
          >
            {resolvedCancelLabel}
          </AlertDialogCancel>
          <AlertDialogAction
            className={cn(
              variant === 'destructive' &&
                'bg-destructive text-destructive-foreground hover:brightness-110 active:brightness-95',
            )}
            onClick={onConfirm}
            data-aijia-dialog-action="confirm"
          >
            {confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
