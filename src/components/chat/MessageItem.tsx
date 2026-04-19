/**
 * MessageItem — dispatches to UserBubble or AiBubble based on role.
 */
import type { Message } from '@/types/message'
import { formatRelativeTime } from '@/lib/format'
import { UserBubble } from './UserBubble'
import { AiBubble } from './AiBubble'

interface MessageItemProps {
  message: Message
  isStreaming?: boolean
}

export function MessageItem({ message, isStreaming }: MessageItemProps) {
  const relativeTime = formatRelativeTime(message.createdAt)

  if (message.role === 'user') {
    return (
      <div>
        <UserBubble message={message} />
        <div className="-mt-5 mb-5 pr-9 text-right text-xs" style={{ color: 'var(--color-text-muted)' }}>
          {relativeTime}
        </div>
      </div>
    )
  }

  if (message.role === 'assistant') {
    return (
      <div>
        <AiBubble message={message} isStreaming={isStreaming} />
        <div className="-mt-5 mb-5 pl-9 text-left text-xs" style={{ color: 'var(--color-text-muted)' }}>
          {relativeTime}
        </div>
      </div>
    )
  }

  // System messages are not rendered
  return null
}
