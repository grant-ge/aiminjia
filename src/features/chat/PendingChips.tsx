import { useTranslation } from 'react-i18next'

import { usePendingStore } from '@/stores/pendingStore'
import type { PendingItem } from '@/types/pending'

import { PendingChip } from './PendingChip'

interface Props {
  sessionId: string
}

const EMPTY: PendingItem[] = []

export function PendingChips({ sessionId }: Props) {
  const { t } = useTranslation()
  // Stable selector: return the actual slot (or the singleton EMPTY) so React
  // doesn't see a new `[]` each render (which would trigger an infinite loop
  // under useSyncExternalStore semantics).
  const items = usePendingStore((s) => s.bySession[sessionId] ?? EMPTY)
  const removeItem = usePendingStore((s) => s.removeItem)

  if (items.length === 0) return null

  const hint =
    items.length === 1
      ? t('chat.pending.singleHint')
      : t('chat.pending.batchHint', { count: items.length })

  return (
    <div className="absolute bottom-full left-0 right-0 mx-3 flex h-12 min-w-0 items-center gap-1.5 overflow-x-auto overflow-y-hidden rounded-t-xl border border-b-0 border-border bg-card px-3 [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      <span className="text-xs text-muted-foreground whitespace-nowrap shrink-0">{hint}</span>
      {items.map((item) => (
        <PendingChip
          key={item.id}
          item={item}
          onRemove={() => removeItem(sessionId, item.id)}
        />
      ))}
    </div>
  )
}
