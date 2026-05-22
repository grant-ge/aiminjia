/**
 * @designSource design.pen#1JNrw bubble/adaptive-max-80
 * @sizing r-16 padding [8,12] bg primary fg primary-foreground; align right; max-w 80%
 */
import { Blocks } from 'lucide-react'
import { useState } from 'react'
import { UserBubbleMarkdown } from './markdown/UserBubbleMarkdown'
import { DispatchBanner } from './DispatchBanner'
import { parseDispatchHeader } from './parseDispatchHeader'
import type { FileAttachment, SkillCommandBreadcrumb } from '@/types/message'

// Team event XML patterns — rendered by PeerMessageBanner instead
const TEAM_EVENT_RE = /^(?:<peer-messages>[\s\S]*<\/peer-messages>|<task-notification[\s\S]*<\/task-notification>)$/

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
  const [expanded, setExpanded] = useState(false)

  // If this is a team event XML message, skip rendering (PeerMessageBanner handles it)
  if (TEAM_EVENT_RE.test((text ?? '').trim())) return null

  // If this user message is actually a dispatch prompt synthesized by
  // `build_dispatch_prompt` (employee派活 path), render the centered banner
  // instead of the right-aligned bubble. Parser returns null for normal user
  // messages, so old conversations + non-dispatch turns are unaffected.
  const dispatchHeader = parseDispatchHeader(text)
  if (dispatchHeader) {
    return <DispatchBanner header={dispatchHeader} />
  }

  const command = skillCommand?.command ?? commandText?.split(/\s+/)[0]
  const tokenLabel = skillCommand?.label ?? skillCommand?.id ?? command?.replace(/^\//, '')
  // 折叠阈值用换行数衡量，不再用字符数：中文短文本 320 字 ≠ 视觉上很长，
  // 但贴一条长 URL（~250 chars）+ 几句话就误触发折叠 + clip，把正文遮住。
  const shouldCollapse = text.split(/\n/).length > 8

  if (!text && !tokenLabel) return null

  return (
    <div className="flex w-full flex-col items-end gap-1.5">
      <div
        data-testid="user-bubble"
        // 头像在右上方紧挨着 bubble，bubble 右上角去尖（rounded-tr-none）能形
        // 成一个"指向头像"的小切角，让 user / avatar 是一组的视觉关系更明确。
        // px-3 = 12px:之前是 16px(走 --user-bubble-padding-x 变量),气泡里
        // 只夹一个附件 chip 时显得空,12px 视觉更紧凑且对纯文字也够呼吸。
        // 该变量只此一处使用,已从 globals.css 移除。
        className="max-w-[80%] overflow-hidden rounded-xl rounded-tr-none bg-primary px-3 py-2 text-sm leading-relaxed text-primary-foreground"
      >
        <div
          data-testid="user-bubble-content"
          className={
            shouldCollapse && !expanded
              ? 'relative max-h-[220px] overflow-hidden'
              : 'relative'
          }
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
          {shouldCollapse && !expanded ? (
            <div className="pointer-events-none absolute inset-x-0 bottom-0 h-12 bg-gradient-to-t from-primary to-transparent" />
          ) : null}
        </div>
        {shouldCollapse ? (
          <button
            type="button"
            onClick={() => setExpanded((next) => !next)}
            className="mt-1 text-xs font-semibold text-primary-foreground/80 underline-offset-2 hover:text-primary-foreground hover:underline"
          >
            {expanded ? '收起' : '展开全部'}
          </button>
        ) : null}
      </div>
    </div>
  )
}
