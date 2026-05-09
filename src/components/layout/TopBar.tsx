/**
 * TopBar — title bar. Hidden when no active conversation.
 * Based on visual-prototype-zh.html top-bar section.
 */
import { useChatStore } from '@/stores/chatStore'

export function TopBar() {
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const conversations = useChatStore((s) => s.conversations)

  const activeConversation = conversations.find(
    (c) => c.id === activeConversationId,
  )

  // Hide TopBar when no active conversation (welcome screen)
  if (!activeConversation || !activeConversationId) return null

  const title = activeConversation.title

  return (
    <header
      className="flex h-11 shrink-0 items-center border-b px-6 border-border"
      style={{ borderColor: 'var(--color-border)' }}
    >
      <h2
        className="text-base font-semibold truncate"
        style={{ color: 'var(--color-text-primary)' }}
      >
        {title}
      </h2>
    </header>
  )
}
