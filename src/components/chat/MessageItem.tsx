/**
 * MessageItem — dispatches to UserBubble or AiBubble based on role.
 */
import type { Message } from '@/types/message'
import { UserBubble } from './UserBubble'
import { AiBubble } from './AiBubble'

interface MessageItemProps {
  message: Message
  isStreaming?: boolean
}

export function MessageItem({ message, isStreaming }: MessageItemProps) {
  if (message.role === 'user') {
    return <UserBubble message={message} />
  }

  if (message.role === 'assistant') {
    return <AiBubble message={message} isStreaming={isStreaming} />
  }

  // System messages are not rendered
  return null
}
