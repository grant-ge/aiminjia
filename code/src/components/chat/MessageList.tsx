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

const STEP_TITLES: Record<number, string> = {
  1: '数据清洗与理解',
  2: '岗位体系',
  3: '职级框架',
  4: '公平性诊断 + 根因分析',
  5: '行动方案 + 管理层材料',
}

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
  const messages = useChatStore((s) => s.messages)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => s.streamingContent)

  return (
    <div>
      {messages.map((msg, idx) => {
        const stepTransition = getStepTransition(msg, messages[idx - 1])

        return (
          <div key={msg.id}>
            {stepTransition && (
              <StepDivider
                stepNumber={stepTransition}
                title={STEP_TITLES[stepTransition] ?? `Step ${stepTransition}`}
              />
            )}
            <MessageItem message={msg} />
          </div>
        )
      })}

      {/* Show streaming assistant response in real-time */}
      {isStreaming && <StreamingBubble content={streamingContent} />}
    </div>
  )
}
