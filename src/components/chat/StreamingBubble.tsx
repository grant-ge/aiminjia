/**
 * StreamingBubble — shows the AI response as it streams in,
 * with a typing indicator when waiting for the first token.
 */
import { useChatStore } from '@/stores/chatStore'
import { TypingIndicator } from '@/components/chat-scene/TypingIndicator'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { stripHallucinatedXml } from '@/lib/sanitize'
import { useTranslation } from 'react-i18next'

interface StreamingBubbleProps {
  content: string
}

const EMPTY_TOOL_EXECUTIONS: Array<{
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
}> = []

export function StreamingBubble({ content }: StreamingBubbleProps) {
  const { t } = useTranslation()
  const toolExecutions = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId
      ? (s.streamStates[activeId]?.toolExecutions ?? s.toolExecutions)
      : (s.toolExecutions ?? EMPTY_TOOL_EXECUTIONS)
  })
  const agentPhase = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? s.streamStates[activeId]?.agentPhase : undefined
  })
  const activeTool = toolExecutions.find((t) => t.status === 'executing')

  // Strip hallucinated XML blocks that some models emit in text content
  const cleanContent = stripHallucinatedXml(content)

  const toolLabel = activeTool ? t('streaming.tools.' + activeTool.toolName, activeTool.toolName) : ''
  const phaseLabel = agentPhase ? t('streaming.phases.' + agentPhase) : ''

  // Phase-aware status text: use TAOR phase if available, otherwise fall back
  const statusText = activeTool
    ? toolLabel
    : agentPhase
      ? phaseLabel
      : (cleanContent ? '' : t('streaming.phases.think'))

  return (
    <div className="mb-7">
      <div>
        {cleanContent ? (
          <AssistantMarkdown text={cleanContent} />
        ) : null}
        {activeTool ? (
          <div
            className="mt-2 flex items-center gap-2 text-xs"
            style={{ color: 'var(--color-text-muted)' }}
          >
            <svg className="h-3.5 w-3.5 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
              <circle cx="12" cy="12" r="10" strokeDasharray="50" strokeDashoffset="20" strokeLinecap="round" />
            </svg>
            <span>{statusText}</span>
          </div>
        ) : (
          <div className={cleanContent ? 'mt-2' : ''}>
            <TypingIndicator variant={agentPhase === 'observe' ? 'organize' : 'default'} />
            {statusText && cleanContent ? (
              <span className="sr-only">{statusText}</span>
            ) : null}
          </div>
        )}
      </div>
    </div>
  )
}
