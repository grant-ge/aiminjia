/**
 * AiBubble — AI message that renders MessageContent fields
 * in the fixed MESSAGE_CONTENT_RENDER_ORDER.
 * Based on visual-prototype-zh.html .msg-body styles.
 */
import type {
  Message,
  MessageContent,
  CodeBlock,
  DataTable,
  MetricCard,
  OptionGroup,
  AnomalyItem,
  InsightBlock as InsightBlockType,
  RootCauseBlock as RootCauseBlockType,
  ConfirmBlock as ConfirmBlockType,
  SearchSource,
  ExecSummary,
  ReportCard,
  GeneratedFile,
  SubAgentEnvelopeContent,
} from '@/types/message'
import { MESSAGE_CONTENT_RENDER_ORDER } from '@/types/message'
import {
  RichCodeBlock,
  MetricCards,
  OptionCards,
  AnomalyList,
  InsightBlock,
  RootCauseBlock,
  ConfirmBlock,
  SearchSourceBlock,
  ExecSummaryCard,
  ReportCards,
  GeneratedFileCard,
  SubAgentResultCard,
} from '@/components/rich-content'
import { TableView } from '@/components/data-table'
import { mapDataTableColumns, mapDataTableRows, toTableMeta } from '@/components/data-table/mapDataTable'
import { TypingIndicator } from '@/components/chat-scene/TypingIndicator'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { useChatStore } from '@/stores/chatStore'
import { openGeneratedFile, revealFileInFolder } from '@/lib/tauri'
import { useCallback } from 'react'
import { useNotificationStore } from '@/stores/notificationStore'
import { useTranslation } from 'react-i18next'

interface AiBubbleProps {
  message: Message
  isStreaming?: boolean
  onUserResponse?: (text: string) => void
}

export function AiBubble({ message, isStreaming, onUserResponse }: AiBubbleProps) {
  const { t } = useTranslation()
  const { content } = message
  const conversationId = useChatStore((s) => s.activeConversationId)

  // Skip rendering if no meaningful content (prevents blank bubbles from
  // historical empty messages or tool-call-only iterations)
  /** Send a user choice back to the agent loop as a message. */
  const handleUserResponse = useCallback(
    (responseText: string) => {
      onUserResponse?.(responseText)
    },
    [onUserResponse],
  )

  /** Open a generated report file via system default app. */
  const handleOpenFile = useCallback((fileId: string) => {
    if (!conversationId) return
    openGeneratedFile(fileId, conversationId).catch((err) => {
      console.error('[AiBubble] Failed to open file:', err)
      useNotificationStore.getState().push({
        level: 'error',
        title: t('aiBubble.openFileFailed'),
        message: t('aiBubble.openFileFailedDesc'),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    })
  }, [conversationId, t])

  /** Reveal a file in the OS file manager (Finder / Explorer). */
  const handleRevealFile = useCallback((fileId: string) => {
    if (!conversationId) return
    revealFileInFolder(fileId, conversationId).catch((err) => {
      console.error('[AiBubble] Failed to reveal file:', err)
      useNotificationStore.getState().push({
        level: 'error',
        title: t('aiBubble.revealFileFailed'),
        message: t('aiBubble.revealFileFailedDesc'),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    })
  }, [conversationId, t])

// Skip rendering if no meaningful content (prevents blank bubbles from
  // historical empty messages or tool-call-only iterations)
  const hasContent = MESSAGE_CONTENT_RENDER_ORDER.some((field) => {
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
{MESSAGE_CONTENT_RENDER_ORDER.map((field) => {
          const value = content[field]
          if (value === undefined || value === null) return null
          return (
            <ContentRenderer
              key={field}
              field={field}
              value={value}
              content={content}
              onUserResponse={handleUserResponse}
              onOpenFile={handleOpenFile}
              onRevealFile={handleRevealFile}
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
  content,
  onUserResponse,
  onOpenFile,
  onRevealFile,
}: {
  field: keyof MessageContent
  value: NonNullable<MessageContent[keyof MessageContent]>
  content: MessageContent
  onUserResponse: (text: string) => void
  onOpenFile: (fileId: string) => void
  onRevealFile: (fileId: string) => void
}) {
  const { t } = useTranslation()
  switch (field) {
    case 'text':
      return <AssistantMarkdown text={value as string} />

    case 'codeBlocks':
      return (
        <>
          {(value as CodeBlock[]).map((block) => (
            <RichCodeBlock
              key={block.id}
              block={block}
              result={content.codeResults?.find((r) => r.codeBlockId === block.id)}
            />
          ))}
        </>
      )

    case 'codeResults':
      // Rendered inline with codeBlocks above
      return null

    case 'tables':
      return (
        <>
          {(value as DataTable[]).map((table) => (
            <div key={table.id} className="my-3">
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

    case 'metrics':
      return <MetricCards metrics={value as MetricCard[]} />

    case 'options':
      return (
        <>
          {(value as OptionGroup[]).map((group) => (
            <OptionCards
              key={group.id}
              group={group}
              onSelect={(optionId) => {
                const opt = group.options.find((o) => o.id === optionId)
                if (opt) onUserResponse(`${t('aiBubble.selectAction')} ${opt.title}`)
              }}
            />
          ))}
        </>
      )

    case 'anomalies':
      return <AnomalyList anomalies={value as AnomalyItem[]} />

    case 'insights':
      return (
        <>
          {(value as InsightBlockType[]).map((insight) => (
            <InsightBlock key={insight.id} insight={insight} />
          ))}
        </>
      )

    case 'rootCauses':
      return (
        <>
          {(value as RootCauseBlockType[]).map((rc) => (
            <RootCauseBlock key={rc.id} rootCause={rc} />
          ))}
        </>
      )

    case 'generatedFiles':
      return (
        <>
          {(value as GeneratedFile[]).map((file) => (
            <GeneratedFileCard
              key={file.id}
              file={file}
              onAction={(fileId, action) => {
                if (action === 'open') onOpenFile(fileId)
                if (action === 'reveal') onRevealFile(fileId)
              }}
            />
          ))}
        </>
      )

    case 'reports':
      return (
        <ReportCards
          reports={value as ReportCard[]}
          onOpen={(reportId) => onOpenFile(reportId)}
        />
      )

    case 'searchSources':
      return (
        <>
          {(value as SearchSource[]).map((source) => (
            <SearchSourceBlock key={source.id} source={source} />
          ))}
        </>
      )

    case 'execSummary':
      return <ExecSummaryCard summary={value as ExecSummary} />

    case 'confirmations':
      return (
        <>
          {(value as ConfirmBlockType[]).map((c) => (
            <ConfirmBlock
              key={c.id}
              confirm={c}
              onConfirm={(action) => onUserResponse(`${t('aiBubble.confirmAction')} ${action}`)}
              onReject={(action) => onUserResponse(`${t('aiBubble.rejectAction')} ${action}`)}
            />
          ))}
        </>
      )

    case 'subagentEnvelope':
      return <SubAgentResultCard envelope={value as SubAgentEnvelopeContent} />

    default:
      return null
  }
}
