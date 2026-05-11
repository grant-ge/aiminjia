import { useEffect } from 'react'
import type { RefObject } from 'react'
import { useDropInbox } from '@/stores/dropInbox'
import { pendingAttachmentsToTokens } from './pendingAttachmentToToken'
import type { RichComposerHandle } from './RichComposer'

/**
 * Drains the global drop-inbox into the composer pointed to by `composerRef`.
 *
 * Trade-off: if the ref's `.current` is null when `pending` arrives (e.g. the
 * composer hasn't finished mounting yet), this effect early-returns and leaves
 * `pending` untouched in the store. The next store change (or rerender that
 * includes pending items) will retry. In practice the native drag-drop listener
 * resolves paths asynchronously, so the composer is always mounted by the time
 * `pending` arrives — this is just defensive.
 *
 * Multiple consumers: if two RichComposer instances mount simultaneously,
 * whichever effect fires first wins (consume() is self-clearing). The Home /
 * Chat pages route-mutex this, so it doesn't happen in practice.
 */
export function useComposerDropInbox(
  composerRef: RefObject<RichComposerHandle | null>,
): void {
  const pending = useDropInbox((s) => s.pending)
  const consume = useDropInbox((s) => s.consume)

  useEffect(() => {
    if (pending.length === 0) return
    const handle = composerRef.current
    if (!handle) return
    const taken = consume()
    if (taken.length === 0) return
    handle.insertAttachmentTokens(pendingAttachmentsToTokens(taken))
  }, [pending, consume, composerRef])
}
