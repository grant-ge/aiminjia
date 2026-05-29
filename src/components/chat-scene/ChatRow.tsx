/**
 * @designSource manual — chat row layout that pairs an avatar+name with a
 *               bubble, giving the chat the "two-person conversation"
 *               look users asked for (2026-05-15).
 *
 * Layout:
 *   role='user'      → name+avatar on the right, bubble fills the rest
 *   role='assistant' → name+avatar on the left,  bubble fills the rest
 *
 * The bubble itself is unchanged (still rendered by `UserMessageBubble`
 * or `AiBubble`); this wrapper just gives it a header strip with sender
 * identity. Used by `MessageList`.
 *
 * The DispatchBanner (派活提示) and team progress / generated-file cards
 * are full-width centered system messages and should NOT be wrapped —
 * the caller decides whether to render <ChatRow> at all.
 */
import type { ReactNode } from 'react'
import { ChatAvatar } from './ChatAvatar'
import { formatChatTime, formatFullDateTime } from '@/lib/chatTime'

interface ChatRowProps {
  role: 'user' | 'assistant'
  name: string
  avatarUrl?: string | null
  /**
   * Fallback variant when `avatarUrl` is null. `'neutral'` paints the
   * gender-free brand-tinted silhouette (default for the current user),
   * `'initial'` paints the first character on a hashed palette color.
   */
  avatarVariant?: 'initial' | 'neutral'
  /** Background color seed for the avatar fallback. Defaults to `name`. */
  colorSeed?: string
  /**
   * ISO 时间。提供时在 name 旁渲染小号时间戳（hover 显示完整时间）。
   * 同一个 turn 内的连续消息只在第一条传入，避免视觉噪音。
   */
  timestamp?: string | null
  /**
   * Suppress the avatar + name header. The gutter is preserved with an
   * invisible placeholder so child bubbles stay aligned with the previous
   * (header-bearing) row. Used in interleaved mode for the 2nd+ assistant
   * "runs" that follow a tool card — the avatar already stamped above.
   */
  hideHeader?: boolean
  children: ReactNode
}

export function ChatRow({ role, name, avatarUrl, avatarVariant, colorSeed, timestamp, hideHeader, children }: ChatRowProps) {
  const isUser = role === 'user'
  // We anchor the header (avatar+name) at the top so multi-line bubbles
  // don't drift the avatar to the middle. `items-start` does that.
  const rowDir = isUser ? 'flex-row-reverse' : 'flex-row'
  const nameAlign = isUser ? 'text-right' : 'text-left'
  const rowInset = isUser ? '' : 'pr-9'
  return (
    <div
      data-testid="chat-row"
      data-role={role}
      className={`flex w-full items-start gap-2 ${rowDir} ${rowInset}`.trim()}
    >
      <div className="flex shrink-0 flex-col items-center gap-1 pt-1" aria-hidden={hideHeader || undefined}>
        {hideHeader ? (
          // Invisible spacer keeps the bubble aligned with the previous
          // header-bearing row. Matches ChatAvatar's intrinsic size (32px).
          <div className="h-8 w-8" />
        ) : (
          <ChatAvatar
            name={name}
            src={avatarUrl ?? null}
            variant={avatarVariant}
            colorSeed={colorSeed}
          />
        )}
      </div>
      <div data-testid="chat-row-content" className="flex min-w-0 flex-1 flex-col gap-1">
        {hideHeader ? null : (
          <div
            data-testid="chat-row-name"
            className={`flex items-baseline gap-2 text-xs font-medium text-muted-foreground ${
              isUser ? 'flex-row-reverse justify-start' : 'justify-start'
            }`}
          >
            <span className={nameAlign}>{name}</span>
            {timestamp ? (
              <time
                data-testid="chat-row-time"
                dateTime={timestamp}
                title={formatFullDateTime(timestamp)}
                className="font-normal text-muted-foreground/70 tabular-nums"
              >
                {formatChatTime(timestamp)}
              </time>
            ) : null}
          </div>
        )}
        <div
          className={
            isUser
              ? 'flex w-full justify-end'
              // assistant rows can stack multiple children (tool trace + AI
              // bubble + …). gap-3 keeps the trace card and the bubble from
              // glueing visually so the row still reads as one "AI turn".
              : 'flex flex-col gap-3'
          }
        >{children}</div>
      </div>
    </div>
  )
}
