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
  children: ReactNode
}

export function ChatRow({ role, name, avatarUrl, avatarVariant, colorSeed, children }: ChatRowProps) {
  const isUser = role === 'user'
  // We anchor the header (avatar+name) at the top so multi-line bubbles
  // don't drift the avatar to the middle. `items-start` does that.
  const rowDir = isUser ? 'flex-row-reverse' : 'flex-row'
  const nameAlign = isUser ? 'text-right' : 'text-left'
  return (
    <div
      data-testid="chat-row"
      data-role={role}
      className={`flex w-full items-start gap-2 ${rowDir}`}
    >
      <div className="flex shrink-0 flex-col items-center gap-1 pt-1">
        <ChatAvatar
          name={name}
          src={avatarUrl ?? null}
          variant={avatarVariant}
          colorSeed={colorSeed}
        />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div
          data-testid="chat-row-name"
          className={`text-xs font-medium text-muted-foreground ${nameAlign}`}
        >
          {name}
        </div>
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
