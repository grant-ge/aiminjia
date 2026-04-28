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
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const userScrolledUp = useRef(false)

  /** Detect when the user scrolls up (away from bottom). */
  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current
    if (!el) return
    userScrolledUp.current = el.scrollHeight - el.scrollTop - el.clientHeight > 100
  }, [])

  // Scroll to bottom when new messages arrive
  useEffect(() => {
    if (!userScrolledUp.current) {
      scrollToBottom(scrollContainerRef.current, true)
    }
  }, [messages.length])

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
      style={{ background: 'var(--color-bg-main)' }}
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
