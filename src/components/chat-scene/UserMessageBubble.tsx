/**
 * @designSource design.pen#1JNrw bubble/adaptive-max-80
 * @sizing r-16 padding [8,12] bg sidebar fg foreground; align right; max-w 80%
 */
import { Blocks, BrainCircuit, Check, Copy } from 'lucide-react'
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { UserBubbleMarkdown } from './markdown/UserBubbleMarkdown'
import { DispatchBanner } from './DispatchBanner'
import { parseDispatchHeader } from './parseDispatchHeader'
import type { FileAttachment, ReasoningMode, SkillCommandBreadcrumb } from '@/types/message'
import { Button } from '@/components/ui/button'

// Team event XML patterns — rendered by PeerMessageBanner instead
const TEAM_EVENT_RE = /^(?:<peer-messages>[\s\S]*<\/peer-messages>|<task-notification[\s\S]*<\/task-notification>)$/
const COLLAPSED_CONTENT_MASK_CLASS =
  '[mask-image:linear-gradient(to_bottom,black_calc(100%_-_28px),transparent_100%)] [-webkit-mask-image:linear-gradient(to_bottom,black_calc(100%_-_28px),transparent_100%)]'

interface UserMessageBubbleProps {
  text: string
  commandText?: string
  skillCommand?: SkillCommandBreadcrumb
  reasoningMode?: ReasoningMode
  files?: FileAttachment[]
  conversationId?: string
}

export function UserMessageBubble({
  text,
  commandText,
  skillCommand,
  reasoningMode,
  files,
  conversationId,
}: UserMessageBubbleProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const [copied, setCopied] = useState<'idle' | 'ok' | 'fail'>('idle')

  const handleCopy = useCallback(() => {
    if (!text) return
    navigator.clipboard
      .writeText(text)
      .then(() => {
        setCopied('ok')
        setTimeout(() => setCopied('idle'), 1600)
      })
      .catch(() => {
        setCopied('fail')
        setTimeout(() => setCopied('idle'), 1600)
      })
  }, [text])

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
  const showDeepReasoning = reasoningMode === 'deep'
  // 折叠阈值用换行数衡量，不再用字符数：中文短文本 320 字 ≠ 视觉上很长，
  // 但贴一条长 URL（~250 chars）+ 几句话就误触发折叠 + clip，把正文遮住。
  const shouldCollapse = text.split(/\n/).length > 8

  if (!text && !tokenLabel) return null

  return (
    <div className="group flex w-full flex-col items-end gap-1.5">
      <div
        data-testid="user-bubble"
        // px-3 = 12px:之前是 16px(走 --user-bubble-padding-x 变量),气泡里
        // 只夹一个附件 chip 时显得空,12px 视觉更紧凑且对纯文字也够呼吸。
        // 该变量只此一处使用,已从 globals.css 移除。
        className="max-w-[80%] overflow-hidden rounded-md bg-sidebar px-3 py-2 text-sm leading-relaxed text-foreground"
      >
        <div
          data-testid="user-bubble-content"
          className={
            shouldCollapse && !expanded
              ? `relative max-h-[220px] overflow-hidden ${COLLAPSED_CONTENT_MASK_CLASS}`
              : 'relative'
          }
        >
          {tokenLabel ? (
            <span
              data-testid="user-skill-token"
              // bubble 外层是 bg-sidebar，内嵌 token 用前景色低透明底维持层次。
              className="mr-2 inline-flex translate-y-[1px] items-center gap-1.5 rounded-md bg-foreground/10 px-2 py-1 text-xs font-semibold leading-none text-foreground shadow-[inset_0_0_0_1px_var(--border)]"
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
          {showDeepReasoning ? (
            <span
              data-testid="user-reasoning-token"
              className="mr-2 inline-flex translate-y-[1px] items-center gap-1.5 rounded-md bg-foreground/10 px-2 py-1 text-xs font-semibold leading-none text-foreground shadow-[inset_0_0_0_1px_var(--border)]"
              title={t('composer.reasoningModeDeepLong')}
            >
              <BrainCircuit
                aria-hidden="true"
                className="shrink-0"
                style={{ width: '0.75rem', height: '0.75rem', transform: 'translateY(1px)' }}
              />
              <span>{t('composer.reasoningModeDeep')}</span>
            </span>
          ) : null}
          {text ? (
            <UserBubbleMarkdown text={text} files={files} conversationId={conversationId} />
          ) : null}
        </div>
        {shouldCollapse ? (
          <Button unstyled
            type="button"
            onClick={() => setExpanded((next) => !next)}
            className="mt-1 text-xs font-semibold text-foreground/70 underline-offset-2 hover:text-foreground hover:underline"
          >
            {expanded ? t('userMessage.collapse') : t('userMessage.expandAll')}
          </Button>
        ) : null}
      </div>
      {text ? (
        <div className="flex h-6 items-center justify-end text-xs">
          <Button
            type="button"
            link
            onClick={handleCopy}
            className="gap-1 text-muted-foreground opacity-0 focus-visible:opacity-100 group-hover:opacity-100 group-focus-within:opacity-100"
            icon={copied === 'ok'
              ? <Check className="h-3.5 w-3.5" aria-hidden="true" />
              : <Copy className="h-3.5 w-3.5" aria-hidden="true" />
            }
            aria-label={t('userMessage.copy', '复制用户消息')}
            title={t('userMessage.copy', '复制用户消息')}
            data-testid="user-message-copy-button"
          >
            <span>
              {copied === 'ok'
                ? t('common.copied')
                : copied === 'fail'
                  ? t('common.copyFailed')
                  : t('common.copy')}
            </span>
          </Button>
        </div>
      ) : null}
    </div>
  )
}
