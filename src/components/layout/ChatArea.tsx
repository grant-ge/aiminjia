/**
 * ChatArea — scrollable message container with auto-scroll.
 * Based on visual-prototype-zh.html chat-area section.
 */
import { useCallback, useEffect, useRef } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { MessageList } from '@/components/chat/MessageList'

/** Scroll a container to the very bottom using scrollTop (avoids scrollIntoView rendering issues). */
function scrollToBottom(el: HTMLElement | null, smooth = false) {
  if (!el) return
  if (smooth) {
    el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
  } else {
    el.scrollTop = el.scrollHeight
  }
}

export function ChatArea() {
  const messages = useChatStore((s) => s.messages)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const userScrolledUp = useRef(false)
  const lastConversationId = useRef<string | null>(null)
  // When switching conversations, we want an instant jump to bottom (not smooth).
  // The flag stays set until we observe messages for the new conversation, since
  // setActiveConversationId and setMessages can land in separate renders.
  const pendingHardJumpRef = useRef(true)

  /** Detect when the user scrolls up (away from bottom). */
  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current
    if (!el) return
    userScrolledUp.current = el.scrollHeight - el.scrollTop - el.clientHeight > 100
  }, [])

  useEffect(() => {
    if (lastConversationId.current !== activeConversationId) {
      lastConversationId.current = activeConversationId
      pendingHardJumpRef.current = true
      userScrolledUp.current = false
    }
    if (userScrolledUp.current) return
    const el = scrollContainerRef.current
    if (!el) return
    if (pendingHardJumpRef.current) {
      scrollToBottom(el)
      requestAnimationFrame(() => scrollToBottom(scrollContainerRef.current))
      if (messages.length > 0) pendingHardJumpRef.current = false
    } else {
      scrollToBottom(el, true)
    }
  }, [activeConversationId, messages.length])

  // During streaming, use a 300ms interval for smooth auto-scroll
  // instead of per-token scrollIntoView that causes rendering issues
  useEffect(() => {
    if (!isStreaming) return
    const timer = setInterval(() => {
      if (!userScrolledUp.current) {
        scrollToBottom(scrollContainerRef.current)
      }
    }, 300)
    return () => clearInterval(timer)
  }, [isStreaming])

  // When streaming ends, scroll to bottom once
  useEffect(() => {
    if (!isStreaming && !userScrolledUp.current) {
      scrollToBottom(scrollContainerRef.current, true)
    }
  }, [isStreaming])

  return (
    <div
      ref={scrollContainerRef}
      data-testid="chat-scroll-region"
      className="flex-1 overflow-y-auto [scrollbar-gutter:stable_both-edges]"
      onScroll={handleScroll}
    >
      <div className="px-6 pt-6 pb-8 [scrollbar-gutter:stable_both-edges]">
        <div className="mx-auto w-full max-w-[736px]">
          <MessageList />
        </div>
      </div>
    </div>
  )
}
