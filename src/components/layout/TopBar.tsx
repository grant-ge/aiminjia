/**
 * TopBar — title + export menu. Hidden when no active conversation.
 * Based on visual-prototype-zh.html top-bar section.
 */
import { useChatStore } from '@/stores/chatStore'
import { ExportMenu } from '@/components/rich-content/ExportMenu'

export function TopBar() {
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const conversations = useChatStore((s) => s.conversations)
  const streamStates = useChatStore((s) => s.streamStates)
  const isStreaming = activeConversationId ? (streamStates[activeConversationId]?.isStreaming ?? false) : false

  const activeConversation = conversations.find(
    (c) => c.id === activeConversationId,
  )

  // Hide TopBar when no active conversation (welcome screen)
  if (!activeConversation || !activeConversationId) return null

  const title = activeConversation.title

  return (
    <header
      className="flex h-11 shrink-0 items-center border-b px-6"
      style={{ borderColor: 'var(--color-border)' }}
    >
      <h2
        className="text-base font-semibold truncate"
        style={{ color: 'var(--color-text-primary)' }}
      >
        {title}
      </h2>

      <div className="ml-auto flex items-center gap-2">
        {!isStreaming && (
          <ExportMenu conversationId={activeConversationId} />
        )}
      </div>
    </header>
  )
}
