/**
 * AiBubble — AI message that renders MessageContent fields
 * in the fixed MESSAGE_CONTENT_RENDER_ORDER.
 * Based on visual-prototype-zh.html .msg-body styles.
 */
import { memo } from 'react'
import { AlertCircle } from 'lucide-react'
import type {
  Message,
  MessageContent,
  MessageError,
  DataTable,
  SubAgentEnvelopeContent,
} from '@/types/message'
import { MESSAGE_CONTENT_RENDER_ORDER } from '@/types/message'
import {
  SubAgentResultCard,
} from '@/components/rich-content'
import { TableView } from '@/components/data-table'
import { mapDataTableColumns, mapDataTableRows, toTableMeta } from '@/components/data-table/mapDataTable'
import { TypingIndicator } from '@/components/chat-scene/TypingIndicator'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'

interface AiBubbleProps {
  message: Message
  isStreaming?: boolean
}

const AI_BUBBLE_RENDER_FIELDS = MESSAGE_CONTENT_RENDER_ORDER.filter((field) =>
  ['text', 'tables', 'subagentEnvelope'].includes(field),
)

function AiBubbleImpl({ message, isStreaming }: AiBubbleProps) {
  const { content, error } = message

  // Skip rendering if no meaningful content (prevents blank bubbles from
  // historical empty messages or tool-call-only iterations)
  const hasContent = AI_BUBBLE_RENDER_FIELDS.some((field) => {
    const value = content[field]
    if (value === undefined || value === null) return false
    if (field === 'text' && typeof value === 'string' && !value.trim()) return false
    if (Array.isArray(value) && value.length === 0) return false
    return true
  })
  if (!hasContent && !error && !isStreaming) return null

  return (
    <div data-aijia-ai-bubble data-aijia-message-id={message.id}>
      <div className="group relative">
        {/* error 存在时只渲染 ErrorCallout，跳过 content 字段渲染（避免 content.text
            与 error.message 同字面量时的重复展示；落盘 content 仍保留 text 字段
            供 history 兼容）. */}
        {!error &&
          AI_BUBBLE_RENDER_FIELDS.map((field) => {
            const value = content[field]
            if (value === undefined || value === null) return null
            return (
              <ContentRenderer
                key={field}
                field={field}
                value={value}
              />
            )
          })}

        {error && <ErrorCallout error={error} />}
        {isStreaming && <TypingIndicator variant="default" />}
      </div>
    </div>
  )
}

export const AiBubble = memo(AiBubbleImpl)

/**
 * ContentRenderer dispatches each MessageContent field to
 * the appropriate rich content component.
 */
function ContentRenderer({
  field,
  value,
}: {
  field: keyof MessageContent
  value: NonNullable<MessageContent[keyof MessageContent]>
}) {
  switch (field) {
    case 'text':
      return <AssistantMarkdown text={value as string} />

    case 'tables':
      return (
        <>
          {(value as DataTable[]).map((table) => (
            <div key={table.id} className="my-4">
              <TableView
                columns={mapDataTableColumns(table.columns)}
                rows={mapDataTableRows(table.rows)}
                meta={toTableMeta(table)}
                enableCopy
                truncateRows={50}
              />
            </div>
          ))}
        </>
      )

    case 'subagentEnvelope':
      return <SubAgentResultCard envelope={value as SubAgentEnvelopeContent} />

    default:
      return null
  }
}

/**
 * 红色错误 callout 子组件（PR2）。
 * - 挂在 AiBubble 内（保持 React.memo 命中）
 * - 颜色用 text-destructive / border-destructive 主题变量
 * - 图标 lucide-react AlertCircle
 * - 无按钮（D' 原则：用户重试 = 输入框再发，spec §2.1）
 *
 * 文案规则：title 按 kind 切换；message 来自后端兜底（i18n 缺失也能展示）；
 * raw 默认不显示，cmd+C/复制按钮可能在未来透出（本期不做）。
 */
function ErrorCallout({ error }: { error: MessageError }) {
  const title = errorTitleByKind(error.kind)
  return (
    <div
      role="alert"
      className="border border-destructive/40 bg-destructive/5 rounded-lg p-3 my-2 flex items-start gap-2"
    >
      <AlertCircle className="text-destructive shrink-0 mt-0.5" size={18} aria-hidden="true" />
      <div className="flex-1 min-w-0">
        <div className="text-destructive font-medium text-sm">{title}</div>
        <div className="text-foreground/80 text-sm mt-1 whitespace-pre-line">{error.message}</div>
      </div>
    </div>
  )
}

/**
 * 按 ErrorKind 返回简短标题。本期硬编码中文（i18n 升级留到下一期）。
 */
function errorTitleByKind(kind: MessageError['kind']): string {
  switch (kind) {
    case 'chunk_timeout':
    case 'network':
      return '响应超时'
    case 'prompt_too_long':
      return '对话过长'
    case 'auth_failed':
      return '登录已失效'
    case 'rate_limited':
      return '请求过于频繁'
    case 'max_iterations':
      return '分析步骤超限'
    case 'budget_exceeded':
      return '预算已用尽'
    case 'execution_error':
      return '处理出错'
    case 'unknown':
    default:
      return '响应失败'
  }
}
