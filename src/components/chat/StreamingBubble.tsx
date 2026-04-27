/**
 * StreamingBubble — shows the AI response as it streams in,
 * with a typing indicator when waiting for the first token.
 */
import { useChatStore } from '@/stores/chatStore'
import { TypingIndicator } from '@/components/chat-scene/TypingIndicator'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { TaskStatusList } from './TaskStatusList'
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

const EMPTY_TASK_STATES: Array<{
  taskId: string
  status: string
  runId: string
  subject: string
}> = []

export function StreamingBubble({ content }: StreamingBubbleProps) {
  const { t } = useTranslation()
  const toolExecutions = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId
      ? (s.streamStates[activeId]?.toolExecutions ?? s.toolExecutions)
      : (s.toolExecutions ?? EMPTY_TOOL_EXECUTIONS)
  })
  const tasks = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.taskStates[activeId] ?? EMPTY_TASK_STATES) : EMPTY_TASK_STATES
  })
  const agentPhase = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? s.streamStates[activeId]?.agentPhase : undefined
  })
  const activeTool = toolExecutions.find((t) => t.status === 'executing')
  const errorTools = toolExecutions.filter((t) => t.status === 'error')

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
    <div className="mb-7 animate-[fadeUp_0.3s_ease]">
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
        {errorTools.length > 0 && (
          <div className="mt-2 flex flex-col gap-1">
            {errorTools.map((tool) => {
              const label = t('streaming.tools.' + tool.toolName, tool.toolName)
              const rawSummary = tool.summary ?? ''
              const summary = rawSummary.length > 80 ? rawSummary.slice(0, 80) + '…' : rawSummary
              return (
                <div
                  key={tool.toolId}
                  className="flex items-start gap-1.5 text-xs"
                  style={{ color: 'var(--color-semantic-red, #ef4444)' }}
                >
                  <span aria-label="tool error" className="mt-px shrink-0">❌</span>
                  <span>
                    <span className="font-medium">{label}</span>
                    {summary ? <span className="opacity-80">: {summary}</span> : null}
                  </span>
                </div>
              )
            })}
          </div>
        )}
        <TaskStatusList tasks={tasks} />
      </div>
    </div>
  )
}
