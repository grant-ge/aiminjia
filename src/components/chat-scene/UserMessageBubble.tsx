/**
 * @designSource design.pen#1JNrw bubble/adaptive-max-80
 * @sizing r-16 padding [8,12] bg primary fg primary-foreground; align right; max-w 80%
 */
import type { SkillCommandBreadcrumb } from '@/types/message'

interface UserMessageBubbleProps {
  text: string
  commandText?: string
  skillCommand?: SkillCommandBreadcrumb
}

export function UserMessageBubble({ text, commandText, skillCommand }: UserMessageBubbleProps) {
  const command = skillCommand?.command ?? commandText?.split(/\s+/)[0]
  const tokenLabel = skillCommand?.label ?? skillCommand?.id ?? command?.replace(/^\//, '')

  return (
    <div className="flex w-full justify-end">
      <div
        data-testid="user-bubble"
        className="max-w-[80%] rounded-2xl bg-primary px-3 py-2 text-sm leading-relaxed text-primary-foreground"
      >
        {tokenLabel ? (
          <span
            data-testid="user-skill-token"
            className="mr-2 inline-flex translate-y-[-1px] items-center gap-1.5 rounded-lg bg-white/24 px-2 py-1 text-xs font-semibold leading-none text-white shadow-[inset_0_0_0_1px_rgba(255,255,255,0.24)]"
            title={command}
          >
            <span aria-hidden="true" className="text-[0.8125rem] leading-none">✦</span>
            <span>{tokenLabel}</span>
          </span>
        ) : null}
        <span className="whitespace-pre-wrap break-words">{text}</span>
      </div>
    </div>
  )
}
