/**
 * ChatArea — scrollable message container with auto-scroll.
 * Based on visual-prototype-zh.html chat-area section.
 */
import { ArrowDown } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
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
  const contentRef = useRef<HTMLDivElement>(null)
  const userScrolledUp = useRef(false)
  const [showJumpToBottom, setShowJumpToBottom] = useState(false)
  const lastConversationId = useRef<string | null>(null)
  // When switching conversations, we want an instant jump to bottom (not smooth).
  // The flag stays set until we observe messages for the new conversation, since
  // setActiveConversationId and setMessages can land in separate renders.
  const pendingHardJumpRef = useRef(true)

  /** Detect when the user scrolls up (away from bottom). */
  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current
    if (!el) return
    const nextScrolledUp = el.scrollHeight - el.scrollTop - el.clientHeight > 100
    userScrolledUp.current = nextScrolledUp
    setShowJumpToBottom(nextScrolledUp)
  }, [])

  const jumpToBottom = useCallback(() => {
    userScrolledUp.current = false
    setShowJumpToBottom(false)
    scrollToBottom(scrollContainerRef.current, true)
  }, [])

  useEffect(() => {
    if (lastConversationId.current !== activeConversationId) {
      lastConversationId.current = activeConversationId
      pendingHardJumpRef.current = true
      userScrolledUp.current = false
      setShowJumpToBottom(false)
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

  useEffect(() => {
    const content = contentRef.current
    if (!content || typeof ResizeObserver === 'undefined') return

    const observer = new ResizeObserver(() => {
      if (!userScrolledUp.current) {
        scrollToBottom(scrollContainerRef.current)
      }
    })
    observer.observe(content)
    return () => observer.disconnect()
  }, [])

  return (
    <div className="relative flex min-h-0 flex-1">
      <div
        ref={scrollContainerRef}
        data-testid="chat-scroll-region"
        className="flex-1 overflow-y-auto"
        onScroll={handleScroll}
      >
        <div className="px-6 pt-6 pb-24">
          <div ref={contentRef} className="mx-auto w-full max-w-[736px]">
            <MessageList />
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
