import { Paperclip, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { PendingItem } from '@/types/pending'
import { Button } from '@/components/ui/button'

const PREVIEW_MAX = 30

interface Props {
  item: PendingItem
  onRemove: () => void
}

/**
 * Strip markdown attachment tokens like `![filename](file:///path)` or
 * `[文件: foo.xlsx](<file:///...>)` from the preview — these are payload
 * artefacts of how the composer serializes attachments inline. The chip
 * shows them via the Paperclip icon instead.
 */
function stripAttachmentTokens(text: string): string {
  return text
    .replace(/!?\[[^\]]*\]\(<?[^)>]+>?\)/g, '')
    .replace(/\s+/g, ' ')
    .trim()
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text
  return text.slice(0, max) + '…'
}

export function PendingChip({ item, onRemove }: Props) {
  const { t } = useTranslation()
  const cleanedText = stripAttachmentTokens(item.text)
  const previewText = cleanedText.length > 0
    ? truncate(cleanedText, PREVIEW_MAX)
    : t('chat.pending.attachmentOnly', { defaultValue: '附件' })

  return (
    <div
      className="
        inline-flex items-center gap-1.5 max-w-xs
        px-2 py-1 rounded-md
        bg-muted text-muted-foreground text-xs
        border border-border
      "
    >
      {item.senderNick && (
        <span className="font-medium text-foreground shrink-0">
          {item.senderNick}:
        </span>
      )}
      <span className="truncate">{previewText}</span>
      {item.attachments.length > 0 && (
        <Paperclip
          className="w-3 h-3 shrink-0"
          data-testid="pending-chip-attachment-icon"
        />
      )}
      <Button unstyled
        type="button"
        onClick={onRemove}
        aria-label={t('chat.pending.removeAria')}
        className="
          ml-0.5 shrink-0
          hover:bg-destructive/10 hover:text-destructive
          rounded-md p-0.5
          transition-colors
        "
      >
        <X className="w-3 h-3" />
      </Button>
    </div>
  )
}
