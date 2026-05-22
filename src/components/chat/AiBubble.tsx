/**
 * AiBubble — AI message that renders MessageContent fields
 * in the fixed MESSAGE_CONTENT_RENDER_ORDER.
 * Based on visual-prototype-zh.html .msg-body styles.
 */
import { memo } from 'react'
import type {
  Message,
  MessageContent,
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
  const { content } = message

  // Skip rendering if no meaningful content (prevents blank bubbles from
  // historical empty messages or tool-call-only iterations)
  const hasContent = AI_BUBBLE_RENDER_FIELDS.some((field) => {
    const value = content[field]
    if (value === undefined || value === null) return false
    if (field === 'text' && typeof value === 'string' && !value.trim()) return false
    if (Array.isArray(value) && value.length === 0) return false
    return true
  })
  if (!hasContent && !isStreaming) return null

  return (
    <div>
      <div className="group relative">
        {AI_BUBBLE_RENDER_FIELDS.map((field) => {
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

        {isStreaming && <TypingIndicator variant="default" />}
        <StreamStatusHint status={content.streamStatus} />
      </div>
    </div>
  )
}

export const AiBubble = memo(AiBubbleImpl)

/**
 * 在 assistant bubble 末尾渲染流式状态 hint：partial 被打断 / 失败 / 用户中止。
 * 缺省（'final' 或 undefined）不渲染。详见 spec
 * `~/lotus/docs/superpowers/specs/2026-05-22-streaming-partial-preservation.md`。
 */
function StreamStatusHint({ status }: { status?: MessageContent['streamStatus'] }) {
  if (!status || status === 'final') return null
  const text =
    status === 'incomplete'
      ? '— 输出在此处中断，下方为重新生成的内容 —'
      : status === 'aborted'
        ? '— 已被中止 —'
        : '— 输出失败 —'
  const tone = status === 'failed' ? 'text-destructive' : 'text-muted-foreground'
  return (
    <div className={`mt-1 text-xs italic ${tone}`}>
      {text}
    </div>
  )
}

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
