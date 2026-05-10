/**
 * @designSource design.pen#1JNrw bubble/adaptive-max-80
 * @sizing r-16 padding [8,12] bg primary fg primary-foreground; align right; max-w 80%
 */
import { Blocks } from 'lucide-react'
import { UserBubbleMarkdown } from './markdown/UserBubbleMarkdown'
import type { FileAttachment, SkillCommandBreadcrumb } from '@/types/message'

interface UserMessageBubbleProps {
  text: string
  commandText?: string
  skillCommand?: SkillCommandBreadcrumb
  files?: FileAttachment[]
  conversationId?: string
}

export function UserMessageBubble({
  text,
  commandText,
  skillCommand,
  files,
  conversationId,
}: UserMessageBubbleProps) {
  const command = skillCommand?.command ?? commandText?.split(/\s+/)[0]
  const tokenLabel = skillCommand?.label ?? skillCommand?.id ?? command?.replace(/^\//, '')

  if (!text && !tokenLabel) return null

  return (
    <div className="flex w-full flex-col items-end gap-1.5">
      <div
        data-testid="user-bubble"
        className="max-w-[80%] rounded-xl rounded-br-[4px] bg-primary px-3 py-2 text-sm leading-relaxed text-primary-foreground"
      >
        {tokenLabel ? (
          <span
            data-testid="user-skill-token"
            // bubble 外层是 bg-primary，内嵌 token 用半透明 primary-foreground 维持文字/图标对比度
            className="mr-2 inline-flex translate-y-[1px] items-center gap-1.5 rounded-lg bg-primary-foreground/20 px-2 py-1 text-xs font-semibold leading-none text-primary-foreground shadow-[inset_0_0_0_1px_rgba(255,255,255,0.24)]"
            title={command}
          >
            <Blocks
              aria-hidden="true"
              className="shrink-0"
              style={{ width: '0.75rem', height: '0.75rem', transform: 'translateY(1px)' }}
            />
            <span>{tokenLabel}</span>
          </span>
        ) : null}
        {text ? (
          <UserBubbleMarkdown text={text} files={files} conversationId={conversationId} />
        ) : null}
      </div>
    </div>
  )
}
