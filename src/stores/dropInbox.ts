import { create } from 'zustand'

import type { PendingAttachment } from '@/hooks/useChatAttachments'

/**
 * Single-shot drop-attachments inbox. The native drag-drop listener
 * (`useDragDropListener`) pushes resolved `PendingAttachment[]` here, then
 * whichever composer is mounted (HomeTaskComposerCard or ChatBottomArea)
 * drains it via `consume()` and appends into its own pendingFiles state.
 *
 * Why a store instead of a callback prop: there's no single React owner of
 * "the active composer" — Home and Chat are alternate routes. A pull model
 * lets each composer subscribe independently without prop drilling.
 */
interface DropInboxState {
  pending: PendingAttachment[]
  push(attachments: PendingAttachment[]): void
  consume(): PendingAttachment[]
}

export const useDropInbox = create<DropInboxState>((set, get) => ({
  pending: [],
  push(attachments) {
    if (attachments.length === 0) return
    set((s) => ({ pending: [...s.pending, ...attachments] }))
  },
  consume() {
    const taken = get().pending
    if (taken.length === 0) return []
    set({ pending: [] })
    return taken
  },
}))
