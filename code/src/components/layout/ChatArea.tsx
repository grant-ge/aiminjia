/**
 * ChatArea — scrollable message container with auto-scroll.
 * Based on visual-prototype-zh.html chat-area section.
 */
import { useCallback, useEffect, useRef } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { MessageList } from '@/components/chat/MessageList'
import { WelcomeScreen } from '@/components/chat/WelcomeScreen'

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

  // Copy-to-clipboard event delegation for markdown code blocks.
  // Inline onclick is blocked by Tauri CSP, so we use data-copy-code
  // attributes with base64-encoded content.
  useEffect(() => {
    const container = scrollContainerRef.current
    if (!container) return

    const handleClick = (e: MouseEvent) => {
      const target = (e.target as HTMLElement).closest('[data-copy-code]') as HTMLElement | null
      if (!target) return
      const encoded = target.getAttribute('data-copy-code')
      if (!encoded) return
      try {
        const code = atob(encoded)
        navigator.clipboard.writeText(code).then(() => {
          const prev = target.textContent
          target.textContent = '已复制'
          setTimeout(() => { target.textContent = prev }, 2000)
        }).catch(() => {
          const prev = target.textContent
          target.textContent = '复制失败'
          setTimeout(() => { target.textContent = prev }, 2000)
        })
      } catch {
        // ignore decode errors
      }
    }

    container.addEventListener('click', handleClick)
    return () => container.removeEventListener('click', handleClick)
  }, [])

  return (
    <div
      ref={scrollContainerRef}
      className="flex-1 overflow-y-auto"
      style={{ background: 'var(--color-bg-main)' }}
      onScroll={handleScroll}
    >
      <div className="mx-auto max-w-[860px] px-6 pt-6 pb-40">
        {messages.length === 0 ? <WelcomeScreen /> : <MessageList />}
      </div>
    </div>
  )
}
