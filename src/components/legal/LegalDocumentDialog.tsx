import { useTranslation } from 'react-i18next'

import type { LegalDocument } from './legalDocuments'

import { useProductName } from '@/hooks/useProductName'
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from '@/components/ui/dialog'

interface LegalDocumentDialogProps {
  document: LegalDocument | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function LegalDocumentDialog({ document, open, onOpenChange }: LegalDocumentDialogProps) {
  const { t } = useTranslation()
  const productName = useProductName()
  const title = document ? t(document.titleKey, { productName }) : t('legal.dialogTitleFallback')
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        aria-describedby={undefined}
        className="flex h-[82vh] max-h-[820px] w-[min(920px,calc(100vw-32px))] max-w-none grid-rows-[auto,minmax(0,1fr)] gap-4 overflow-hidden p-0"
      >
        <DialogTitle className="sr-only">{title}</DialogTitle>
        {document ? (
          <iframe
            title={title}
            sandbox=""
            srcDoc={document.html}
            className="h-full min-h-0 w-full border-0 bg-background"
          />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
