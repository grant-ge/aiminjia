import type { Conversation } from '@/types/message'

/**
 * Idempotently insert a placeholder conversation at the head of `conversations`.
 *
 * Called synchronously after `employeeTrigger` returns a backend-allocated
 * convId, so MessageList + ChatSidebar have a stable anchor before the
 * backend's async `conversation:created` event lands in App.tsx's reload
 * listener.
 *
 * Returns the new conversations array. If `convId` already exists, returns
 * the input array unchanged (referentially equal) so callers can skip the
 * Zustand write entirely.
 */
export function seedDispatchConversation(
  conversations: Conversation[],
  convId: string,
  employeeName: string,
  now: string = new Date().toISOString(),
): Conversation[] {
  if (conversations.find((c) => c.id === convId)) return conversations
  const placeholder: Conversation = {
    id: convId,
    title: `派活: ${employeeName}`,
    createdAt: now,
    updatedAt: now,
    isArchived: false,
  }
  return [placeholder, ...conversations]
}
