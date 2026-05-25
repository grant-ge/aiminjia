/**
 * ChatArea — scrollable message container with auto-scroll.
 * Stick-to-bottom behaviour delegated to `use-stick-to-bottom` (MIT),
 * which handles streaming-time anchoring, escape detection, and
 * scroll-anchor-on-resize without the React-commit transients that
 * tripped our earlier hand-rolled ResizeObserver.
 */
import { ArrowDown } from 'lucide-react'
import { useCallback, useLayoutEffect, useRef } from 'react'
import { useStickToBottom } from 'use-stick-to-bottom'
import { useChatStore } from '@/stores/chatStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { MessageList } from '@/components/chat/MessageList'
import type { ExpertTeamId } from '@/features/expert-teams/teams'

interface ChatAreaProps {
  expertTeamId?: ExpertTeamId
}

export function ChatArea({ expertTeamId }: ChatAreaProps = {}) {
  const messages = useChatStore((s) => s.messages)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? 'full')

  const { scrollRef, contentRef, scrollToBottom, isNearBottom, escapedFromLock } =
    useStickToBottom({
      // 'instant' for both: streaming output should pin to bottom synchronously
      // every tick, like ChatGPT — spring physics here feels laggy when text
      // streams in fast.
      resize: 'instant',
      initial: 'instant',
    })

  // Conversation switching is two-phase: activeConversationId flips first,
  // then getMessages() resolves async and setMessages() lands frames later.
  // The library's ResizeObserver only fires AFTER the second phase, by which
  // time isAtBottom may be stale-false (inherited from the previous chat) —
  // that's the "flash" users see: new content briefly visible at the top
  // before being pinned. We force-pin every paint between the two phases.
  const lastConversationId = useRef<string | null>(null)
  const pendingHardJump = useRef(true)
  if (lastConversationId.current !== activeConversationId) {
    lastConversationId.current = activeConversationId
    pendingHardJump.current = true
  }
  useLayoutEffect(() => {
    if (!pendingHardJump.current) return
    void scrollToBottom({ animation: 'instant' })
    // Clear the flag once we've seen messages for this conversation. Until
    // then, every render re-pins to the bottom — covering the empty-then-
    // populated frame sequence above.
    if (messages.length > 0) pendingHardJump.current = false
  })

  // Show jump-to-bottom button only when the user has actively escaped the
  // sticky lock (i.e. scrolled up themselves). `isNearBottom` alone would
  // flicker during programmatic scrolls.
  const showJumpToBottom = escapedFromLock && !isNearBottom

  const jumpToBottom = useCallback(() => {
    void scrollToBottom({ animation: 'instant' })
  }, [scrollToBottom])

  return (
    <div className="relative flex min-h-0 flex-1">
      <div
        ref={scrollRef}
        data-testid="chat-scroll-region"
        // overscroll-contain: stop macOS WebKit rubber-band bounce at top/bottom.
        // Without this, scrolling past the edge translates the composited
        // contents while DOM hit-test stays in place — clicks and text
        // selection silently miss their targets.
        className="flex-1 overflow-y-auto overscroll-contain"
      >
        <div className="px-6 pt-6 pb-12">
          <div
            ref={contentRef}
            className={chatWidthMode === 'full' ? 'w-full' : 'mx-auto w-full max-w-[736px]'}
          >
            <MessageList expertTeamId={expertTeamId} />
          </div>
        </div>
      </div>
      {showJumpToBottom ? (
        <button
          type="button"
          aria-label="回到底部"
          onClick={jumpToBottom}
          className="absolute bottom-4 left-1/2 z-20 flex h-9 w-9 -translate-x-1/2 items-center justify-center rounded-full border border-border bg-card text-muted-foreground shadow-[var(--shadow-card)] transition-colors hover:bg-muted hover:text-foreground"
        >
          <ArrowDown className="h-4 w-4" />
        </button>
      ) : null}
    </div>
  )
}
