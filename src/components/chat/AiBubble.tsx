/**
 * AiBubble — AI message that renders MessageContent fields
 * in the fixed MESSAGE_CONTENT_RENDER_ORDER.
 * Based on visual-prototype-zh.html .msg-body styles.
 */
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

export function AiBubble({ message, isStreaming }: AiBubbleProps) {
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
    <div className="animate-[fadeUp_0.3s_ease]">
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
      </div>
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
