/**
 * MessageList — renders the full message sequence for a conversation,
 * including step dividers between analysis phases and in-progress streaming.
 * Based on visual-prototype-zh.html chat-area layout.
 */
import type { Message } from '@/types/message'
import { useChatStore } from '@/stores/chatStore'
import { MessageItem } from './MessageItem'
import { StepDivider } from './StepDivider'
import { StreamingBubble } from './StreamingBubble'
import { TurnSummaryBadge } from './TurnSummaryBadge'
import { useTranslation } from 'react-i18next'

/**
 * Detect analysis step transitions from progress state.
 * Returns the step number if this message starts a new step.
 */
function getStepTransition(message: Message, prevMessage?: Message): number | null {
  const currentStep = message.content.progress?.currentStep
  const prevStep = prevMessage?.content.progress?.currentStep

  if (currentStep && currentStep !== prevStep) {
    return currentStep
  }
  return null
}

export function MessageList() {
  const { t } = useTranslation()
  const messages = useChatStore((s) => s.messages)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => s.streamingContent)
  const lastTurnSummary = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? s.streamStates[activeId]?.lastTurnSummary : undefined
  })

  return (
    <div>
      {messages.map((msg, idx) => {
        const stepTransition = getStepTransition(msg, messages[idx - 1])

        return (
          <div key={msg.id}>
            {stepTransition && (
              <StepDivider
                stepNumber={stepTransition}
                title={t('messageList.steps.' + stepTransition, 'Step ' + stepTransition)}
              />
            )}
            <MessageItem message={msg} />
          </div>
        )
      })}

      {/* Show streaming assistant response in real-time */}
      {isStreaming && <StreamingBubble content={streamingContent} />}
      <TurnSummaryBadge summary={lastTurnSummary} />
    </div>
  )
}
