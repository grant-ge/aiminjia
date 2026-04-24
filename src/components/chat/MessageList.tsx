/**
 * @designSource design.pen#F8ixG flow
 * @sizing padding [24,40] gap 18
 */
import { useState } from 'react'
import { FileSpreadsheet } from 'lucide-react'

import { AiBubble } from '@/components/chat/AiBubble'
import { StreamingBubble } from '@/components/chat/StreamingBubble'
import { GeneratedFileCard } from '@/components/chat-scene/GeneratedFileCard'
import { SuggestChipGroup } from '@/components/chat-scene/SuggestChipGroup'
import { ToolGroupCard } from '@/components/chat-scene/ToolGroupCard'
import { UserMessageBubble } from '@/components/chat-scene/UserMessageBubble'
import { useChatStore } from '@/stores/chatStore'
import { useChat } from '@/hooks/useChat'
import { useTurnRenderModel } from '@/hooks/useTurnRenderModel'

export function MessageList() {
  const turns = useTurnRenderModel()
  const { sendUserMessage } = useChat()
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.streamingContent ?? '') : ''
  })
  const [expansion, setExpansion] = useState<
    Record<number, { expanded: boolean; stepIndex: number | null }>
  >({})

  return (
    <div className="flex flex-col gap-5 px-10 py-6">
      {turns.map((t, i) => {
        const e = expansion[i] ?? { expanded: true, stepIndex: null }
        return (
          <div key={i} className="flex flex-col gap-4">
            {t.userMessage ? <UserMessageBubble text={t.userMessage.text} /> : null}
            {t.toolGroup ? (
              <ToolGroupCard
                status={t.toolGroup.status}
                steps={t.toolGroup.steps}
                durationMs={t.toolGroup.durationMs}
                expanded={e.expanded}
                expandedStepIndex={e.stepIndex}
                onToggle={() =>
                  setExpansion((prev) => ({ ...prev, [i]: { ...e, expanded: !e.expanded } }))
                }
                onToggleStep={(index) =>
                  setExpansion((prev) => ({
                    ...prev,
                    [i]: { ...e, stepIndex: e.stepIndex === index ? null : index },
                  }))
                }
              />
            ) : null}
            {t.aiSegments.map((s) => (
              <AiBubble key={s.id} message={s.message} hideHeader onUserResponse={sendUserMessage} />
            ))}
            {t.generatedFiles.map((f) => (
              <GeneratedFileCard
                key={f.id}
                title={f.title}
                sub={f.sub}
                appName={f.appName}
                fileIcon={<FileSpreadsheet className="h-4 w-4 text-muted-foreground" />}
                onOpen={() => {}}
              />
            ))}
            {t.suggestions.length > 0 ? (
              <SuggestChipGroup
                items={t.suggestions.map((s) => ({ label: s, onClick: () => {} }))}
              />
            ) : null}
          </div>
        )
      })}
      {isStreaming ? <StreamingBubble content={streamingContent} /> : null}
    </div>
  )
}
